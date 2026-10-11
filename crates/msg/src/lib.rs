//! Outlook `.msg` / `.oft` files ([MS-OXMSG]): a message's property bag, recipients and attachments
//! laid out as storages and streams of a compound file ([MS-CFB]).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod read;
mod write;

pub use read::read;
pub use write::{Kind, write, write_as};

#[derive(Debug, thiserror::Error)]
pub enum MsgError {
    #[error("this is not an Outlook message file")]
    NotMsg,
    #[error("the message file is damaged: {0}")]
    Damaged(String),
    #[error("{0}")]
    Io(#[from] std::io::Error),
}

/// Storage and stream names ([MS-OXMSG] 2.2).
pub(crate) const PROPS_STREAM: &str = "__properties_version1.0";
pub(crate) const NAMEID_STORAGE: &str = "__nameid_version1.0";
pub(crate) const RECIP_PREFIX: &str = "__recip_version1.0_#";
pub(crate) const ATTACH_PREFIX: &str = "__attach_version1.0_#";
pub(crate) const EMBEDDED_STORAGE: &str = "__substg1.0_3701000D";

/// Property stream header sizes ([MS-OXMSG] 2.4.1): top level, embedded message, recipient/attachment.
pub(crate) const HEADER_TOP: usize = 32;
pub(crate) const HEADER_EMBEDDED: usize = 24;
pub(crate) const HEADER_CHILD: usize = 8;

pub(crate) fn value_stream(id: u16, ty: u16) -> String {
    format!("__substg1.0_{id:04X}{ty:04X}")
}

/// CRC-32 (IEEE 802.3), used to pick a named property's lookup stream ([MS-OXMSG] 2.2.3.2.1).
pub(crate) fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 != 0 { crc >> 1 ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

#[cfg(test)]
mod tests;
