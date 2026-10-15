//! The fake web server: it reads one request and sends the answer kept for its host and path,
//! or a 404.

use super::{State, lock};
use crate::{Conn, End, NetError};
use std::sync::Mutex;

pub fn serve(end: End, state: &Mutex<State>, host: &str) -> Result<(), NetError> {
    let mut conn = Conn::new(Box::new(end));
    let request = String::from_utf8_lossy(&conn.line()?).into_owned();
    // The rest of the head.
    while !conn.line()?.is_empty() {}
    let path = request.split(' ').nth(1).unwrap_or("").to_string();
    let answer = {
        let mut state = lock(state);
        state.heard.push(request.clone());
        let wanted = format!("{host}{path}");
        state.pages.iter().find(|(at, _)| *at == wanted).map(|(_, answer)| answer.clone())
    };
    conn.send(&answer.unwrap_or_else(|| {
        b"HTTP/1.1 404 Not Found
Content-Length: 0

"
        .to_vec()
    }))
}
