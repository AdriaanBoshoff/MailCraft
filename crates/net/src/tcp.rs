//! Connections to real servers: TCP, made secure with TLS 1.2 or 1.3 ([RFC 8446], and [RFC 8314]
//! for mail) by rustls, the server's certificate checked against the roots this system trusts
//! and against the name the account gives. Not on the web build.

use crate::{Connector, Endpoint, NetError, Security, Stream, one_line, said};
use rustls::pki_types::{CertificateDer, ServerName};
use rustls::{ClientConfig, ClientConnection, RootCertStore, StreamOwned};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::time::Duration;

/// Addresses of one name tried before giving up.
const MAX_ADDRESSES: usize = 8;

/// Opens TCP connections and puts TLS over them.
pub struct TcpConnector {
    config: Arc<ClientConfig>,
    /// How long one address is tried, and how long a read or a write waits.
    wait: Duration,
}

impl TcpConnector {
    /// A connector that trusts what this system trusts.
    pub fn new() -> Result<Self, NetError> {
        let mut roots = RootCertStore::empty();
        // A store that cannot be read in part still gives what it could read.
        for cert in rustls_native_certs::load_native_certs().certs {
            let _ = roots.add(cert);
        }
        if roots.is_empty() {
            return Err(NetError::Unsupported("no trusted certificates were found on this system, so no server can be checked".into()));
        }
        Self::with_roots(roots)
    }

    /// A connector that trusts only these certificates: for a server with a certificate of its
    /// own making, and for tests.
    pub fn trusting(certs: &[CertificateDer<'static>]) -> Result<Self, NetError> {
        let mut roots = RootCertStore::empty();
        for cert in certs {
            roots.add(cert.clone()).map_err(|e| NetError::Certificate(said(&e.to_string())))?;
        }
        Self::with_roots(roots)
    }

    fn with_roots(roots: RootCertStore) -> Result<Self, NetError> {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let config = ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .map_err(|e| NetError::Certificate(said(&e.to_string())))?
            .with_root_certificates(roots)
            .with_no_client_auth();
        Ok(Self { config: Arc::new(config), wait: Duration::from_secs(60) })
    }

    /// How long to wait for a server before it is said not to answer.
    pub fn waiting(mut self, wait: Duration) -> Self {
        self.wait = wait.clamp(Duration::from_secs(1), Duration::from_secs(600));
        self
    }

    fn tls(&self, plain: Box<dyn Stream>, host: &str) -> Result<Box<dyn Stream>, NetError> {
        let name = ServerName::try_from(host.to_string()).map_err(|_| NetError::Unsupported(format!("{} is not a server name", said(host))))?;
        let conn = ClientConnection::new(Arc::clone(&self.config), name).map_err(|e| NetError::Certificate(said(&e.to_string())))?;
        let mut tls = StreamOwned::new(conn, plain);
        // The handshake is made now, so a certificate that is not trusted is said at connect
        // rather than as a broken first reply.
        while tls.conn.is_handshaking() {
            tls.conn.complete_io(&mut tls.sock).map_err(handshake)?;
        }
        Ok(Box::new(tls))
    }
}

/// A failed handshake: a certificate or protocol refusal from rustls, or the connection lost.
fn handshake(e: std::io::Error) -> NetError {
    match e.get_ref().and_then(|inner| inner.downcast_ref::<rustls::Error>()) {
        Some(tls) => NetError::Certificate(said(&tls.to_string())),
        None => crate::conn::io_error(e),
    }
}

impl Connector for TcpConnector {
    fn connect(&self, to: &Endpoint) -> Result<Box<dyn Stream>, NetError> {
        one_line("server name", &to.host)?;
        if to.host.is_empty() || to.host.len() > 253 || to.port == 0 {
            return Err(NetError::Unsupported(format!("{}:{} is not a server to connect to", said(&to.host), to.port)));
        }
        let addresses = (to.host.as_str(), to.port)
            .to_socket_addrs()
            .map_err(|_| NetError::Unsupported(format!("the server {} was not found", said(&to.host))))?;
        let mut last = None;
        let mut stream = None;
        for address in addresses.take(MAX_ADDRESSES) {
            match TcpStream::connect_timeout(&address, self.wait) {
                Ok(s) => {
                    stream = Some(s);
                    break;
                }
                Err(e) => last = Some(e),
            }
        }
        let stream = match (stream, last) {
            (Some(s), _) => s,
            (None, Some(e)) => return Err(crate::conn::io_error(e)),
            (None, None) => return Err(NetError::Unsupported(format!("the server {} was not found", said(&to.host)))),
        };
        stream.set_read_timeout(Some(self.wait))?;
        stream.set_write_timeout(Some(self.wait))?;
        // Commands are short lines that wait for their answer; they go at once.
        stream.set_nodelay(true)?;
        let plain: Box<dyn Stream> = Box::new(stream);
        match to.security {
            Security::Tls => self.tls(plain, &to.host),
            Security::StartTls | Security::Plain => Ok(plain),
        }
    }

    fn secure(&self, plain: Box<dyn Stream>, host: &str) -> Result<Box<dyn Stream>, NetError> {
        self.tls(plain, host)
    }
}
