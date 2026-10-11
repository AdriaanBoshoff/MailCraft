//! Data block encoding ([MS-PST] 5.1 permutative, 5.2 cyclic). This is obfuscation, not security.

use crate::crypt_table::MPBB_CRYPT;

fn table(offset: usize, b: u8) -> u8 {
    MPBB_CRYPT.get(offset + usize::from(b)).copied().unwrap_or(b)
}

/// NDB_CRYPT_PERMUTE, decoding direction: each byte through mpbbI.
pub(crate) fn permute_decode(data: &mut [u8]) {
    for b in data {
        *b = table(512, *b);
    }
}

/// NDB_CRYPT_PERMUTE, encoding direction: each byte through mpbbR. Only fixtures are written so far.
#[cfg(test)]
pub(crate) fn permute_encode(data: &mut [u8]) {
    for b in data {
        *b = table(0, *b);
    }
}

/// NDB_CRYPT_CYCLIC: the same routine encodes and decodes. `key` is the low 32 bits of the block id.
pub(crate) fn cyclic(data: &mut [u8], key: u32) {
    let mut w = (key ^ key >> 16) as u16;
    for b in data {
        let (lo, hi) = (w as u8, (w >> 8) as u8);
        let mut x = b.wrapping_add(lo);
        x = table(0, x);
        x = x.wrapping_add(hi);
        x = table(256, x);
        x = x.wrapping_sub(hi);
        x = table(512, x);
        *b = x.wrapping_sub(lo);
        w = w.wrapping_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodings_undo_themselves() {
        let plain: Vec<u8> = (0..=255u8).chain(0..=255).collect();
        let mut a = plain.clone();
        permute_encode(&mut a);
        assert_ne!(a, plain);
        permute_decode(&mut a);
        assert_eq!(a, plain);
        let mut b = plain.clone();
        cyclic(&mut b, 0x1234_5678);
        assert_ne!(b, plain);
        cyclic(&mut b, 0x1234_5678);
        assert_eq!(b, plain);
    }
}
