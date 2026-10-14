//! Two ends of an in-memory connection, for the fake server: what one end writes the other
//! reads. No socket is involved, so the tests run anywhere and touch no network.

use std::io::{self, Read, Write};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::time::Duration;

/// How long a read waits before it is said to have timed out, so a test that goes wrong ends.
const WAIT: Duration = Duration::from_secs(20);

pub struct End {
    tx: Sender<Vec<u8>>,
    rx: Receiver<Vec<u8>>,
    got: Vec<u8>,
    at: usize,
    wait: Duration,
}

pub fn pipe() -> (End, End) {
    let (a_tx, b_rx) = channel();
    let (b_tx, a_rx) = channel();
    (End { tx: a_tx, rx: a_rx, got: Vec::new(), at: 0, wait: WAIT }, End { tx: b_tx, rx: b_rx, got: Vec::new(), at: 0, wait: WAIT })
}

impl End {
    /// How long a read waits for the other end before it times out.
    pub fn set_wait(&mut self, wait: Duration) {
        self.wait = wait;
    }
}

impl Read for End {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.at >= self.got.len() {
            match self.rx.recv_timeout(self.wait) {
                Ok(bytes) => (self.got, self.at) = (bytes, 0),
                Err(RecvTimeoutError::Disconnected) => return Ok(0),
                Err(RecvTimeoutError::Timeout) => return Err(io::ErrorKind::TimedOut.into()),
            }
        }
        let rest = self.got.get(self.at..).unwrap_or_default();
        let n = rest.len().min(buf.len());
        if let (Some(to), Some(from)) = (buf.get_mut(..n), rest.get(..n)) {
            to.copy_from_slice(from);
        }
        self.at += n;
        Ok(n)
    }
}

impl Write for End {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        self.tx.send(buf.to_vec()).map_err(|_| io::Error::from(io::ErrorKind::BrokenPipe))?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
