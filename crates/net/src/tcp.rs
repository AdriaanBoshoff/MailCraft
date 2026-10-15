//! Connections to real servers: TCP, made secure with TLS 1.2 or 1.3 ([RFC 8446], and [RFC 8314]
//! for mail) by rustls, the server's certificate checked against the roots this system trusts
//! and against the name the account gives. Not on the web build.
//!
//! A certificate this system does not trust is refused, and the refusal carries it
//! ([`NetError::Untrusted`]) so the owner can be shown it and may trust it. A trusted one is
//! taken for its own server only, and only when the server shows exactly that certificate;
//! the server must still prove it holds the certificate's key, as the handshake always asks.

use crate::{About, Connector, Endpoint, Fingerprint, NetError, Protocol, Security, Seen, Stream, one_line, said};
use rustls::client::WebPkiServerVerifier;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::CryptoProvider;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{CertificateError, ClientConfig, ClientConnection, DigitallySignedStruct, RootCertStore, SignatureScheme, StreamOwned};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Addresses of one name tried before giving up.
const MAX_ADDRESSES: usize = 8;

/// Opens TCP connections and puts TLS over them.
pub struct TcpConnector {
    provider: Arc<CryptoProvider>,
    /// What this system trusts, checked first for every server.
    roots: Arc<WebPkiServerVerifier>,
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
        let roots = WebPkiServerVerifier::builder_with_provider(Arc::new(roots), Arc::clone(&provider))
            .build()
            .map_err(|e| NetError::Certificate(said(&e.to_string())))?;
        Ok(Self { provider, roots, wait: Duration::from_secs(60) })
    }

    /// How long to wait for a server before it is said not to answer.
    pub fn waiting(mut self, wait: Duration) -> Self {
        self.wait = wait.clamp(Duration::from_secs(1), Duration::from_secs(600));
        self
    }

    fn tls(&self, plain: Box<dyn Stream>, to: &Endpoint) -> Result<Box<dyn Stream>, NetError> {
        let host = to.host.as_str();
        let name = ServerName::try_from(host.to_string()).map_err(|_| NetError::Unsupported(format!("{} is not a server name", said(host))))?;
        // Each connection has its own check, which knows the certificate trusted for its server
        // and keeps the one shown.
        let check = Arc::new(Check { roots: Arc::clone(&self.roots), trusted: to.trusted, shown: Mutex::new(None) });
        let verifier: Arc<dyn ServerCertVerifier> = check.clone();
        let config = ClientConfig::builder_with_provider(Arc::clone(&self.provider))
            .with_safe_default_protocol_versions()
            .map_err(|e| NetError::Certificate(said(&e.to_string())))?
            .dangerous()
            .with_custom_certificate_verifier(verifier)
            .with_no_client_auth();
        let conn = ClientConnection::new(Arc::new(config), name).map_err(|e| NetError::Certificate(said(&e.to_string())))?;
        let mut tls = StreamOwned::new(conn, plain);
        // The handshake is made now, so a certificate that is not trusted is said at connect
        // rather than as a broken first reply.
        while tls.conn.is_handshaking() {
            if let Err(e) = tls.conn.complete_io(&mut tls.sock) {
                return Err(match check.refused(to) {
                    Some(seen) => NetError::Untrusted(Box::new(seen)),
                    None => handshake(e),
                });
            }
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

/// The SHA-256 of a certificate as it was sent.
pub(crate) fn fingerprint(der: &[u8]) -> Fingerprint {
    let digest = ring::digest::digest(&ring::digest::SHA256, der);
    let mut out = [0u8; 32];
    for (slot, b) in out.iter_mut().zip(digest.as_ref()) {
        *slot = *b;
    }
    Fingerprint(out)
}

/// Why a certificate was refused, in words; and whether the owner may trust it anyway.
fn why(e: &rustls::Error) -> (String, bool) {
    let rustls::Error::InvalidCertificate(c) = e else { return (said(&e.to_string()), true) };
    let words = match c {
        CertificateError::UnknownIssuer => "it was not signed by anyone this system trusts; a server can make one of its own like this",
        CertificateError::Expired | CertificateError::ExpiredContext { .. } => "it has expired",
        CertificateError::NotValidYet | CertificateError::NotValidYetContext { .. } => "it is not valid yet; this computer's clock may be wrong",
        CertificateError::NotValidForName | CertificateError::NotValidForNameContext { .. } => "it was made for another server name",
        CertificateError::Revoked => return ("whoever signed it has revoked it".to_string(), false),
        CertificateError::BadSignature => "its signature is not right",
        _ => return (said(&e.to_string()), true),
    };
    (words.to_string(), true)
}

/// The check of one connection's certificate: what this system trusts, else the certificate
/// the owner trusted for this server.
#[derive(Debug)]
struct Check {
    roots: Arc<WebPkiServerVerifier>,
    trusted: Option<Fingerprint>,
    /// The certificate refused, and why.
    shown: Mutex<Option<(Vec<u8>, rustls::Error)>>,
}

impl Check {
    /// The certificate refused, as the owner is to be shown it.
    fn refused(&self, to: &Endpoint) -> Option<Seen> {
        let (der, e) = self.shown.lock().ok()?.take()?;
        let (reason, trustable) = why(&e);
        let about: About = crate::cert::about(&der);
        Some(Seen { host: to.host.clone(), fingerprint: fingerprint(&der), reason, trustable, changed: self.trusted.is_some(), about })
    }
}

impl ServerCertVerifier for Check {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        if self.trusted.is_some_and(|t| t == fingerprint(end_entity.as_ref())) {
            return Ok(ServerCertVerified::assertion());
        }
        self.roots.verify_server_cert(end_entity, intermediates, server_name, ocsp_response, now).inspect_err(|e| {
            if let Ok(mut shown) = self.shown.lock() {
                *shown = Some((end_entity.as_ref().to_vec(), e.clone()));
            }
        })
    }

    // The server proves it holds the certificate's key however the certificate was trusted.
    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.roots.verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.roots.verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.roots.supported_verify_schemes()
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
        // A web server a message named is not reached on this machine or the local network,
        // whatever its name leads to.
        let web = to.protocol == Protocol::Https;
        for address in addresses.take(MAX_ADDRESSES).filter(|a| !(web && crate::http::local_ip(a.ip()))) {
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
            Security::Tls => self.tls(plain, to),
            Security::StartTls | Security::Plain => Ok(plain),
        }
    }

    fn secure(&self, plain: Box<dyn Stream>, to: &Endpoint) -> Result<Box<dyn Stream>, NetError> {
        self.tls(plain, to)
    }
}
