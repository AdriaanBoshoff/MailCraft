//! Writing the Node Database layer ([MS-PST] 2.2): a whole data file from its blocks and nodes,
//! front to back, Unicode or ANSI (ids and offsets of four bytes, and what follows from that),
//! or Unicode with 4K pages (a layout in no specification, measured on files Outlook wrote:
//! see `Big`).
//!
//! A file is written whole, to a place of the caller's choosing (a temporary file that is
//! swapped in when it is complete), or into the free end of one that is there (`write_onto.rs`).
//! Space is handed out in
//! file order, so the allocation maps are simply "everything up to here": each 253,952-byte
//! section ([MS-PST] 2.2.2.7.2) is built in memory and written once, with its map in front.
//!
//! A block that is byte for byte the same as one already written is not written again: the first
//! one gains a reference ([MS-PST] 2.2.2.7.7.3.1). The same attachment in fifty messages is one
//! set of blocks.
//!
//! An id the writer hands back (`data`, `stream`, `subnodes`) stands for one reference: it goes
//! into one node or one subnode entry. For a second place, ask again with the same bytes; the
//! writer finds the blocks and counts the reference.

use crate::ndb::Ver;
use crate::{PstError, crypt};
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom, Write};

/// The first allocation map is at 0x4400 and one follows every 496 * 8 * 64 bytes ([MS-PST] 2.2.2.7.2).
pub(crate) const FIRST_AMAP: u64 = 0x4400;
pub(crate) const SECTION: usize = 496 * 8 * 64;
/// The density list's fixed place ([MS-PST] 2.2.2.7.4).
pub(crate) const DLIST: u64 = 0x4200;
pub(crate) const PAGE: usize = 512;
/// A page's bytes before its trailer.
pub(crate) const PAGE_BODY: usize = PAGE - TRAILER;
/// Blocks are multiples of 64 bytes, at most 8 KB with their trailer ([MS-PST] 2.2.2.8).
pub(crate) const SLOT: usize = 64;
pub(crate) const TRAILER: usize = 16;
#[cfg(test)]
pub(crate) const MAX_DATA: usize = 8192 - TRAILER;
/// The entry area of a B-tree page ([MS-PST] 2.2.2.7.7.1).
pub(crate) const BT_AREA: usize = 488;

/// Page types ([MS-PST] 2.2.2.7.1).
pub(crate) const PT_BBT: u8 = 0x80;
pub(crate) const PT_NBT: u8 = 0x81;
pub(crate) const PT_FMAP: u8 = 0x82;
pub(crate) const PT_PMAP: u8 = 0x83;
pub(crate) const PT_AMAP: u8 = 0x84;
pub(crate) const PT_FPMAP: u8 = 0x85;
pub(crate) const PT_DLIST: u8 = 0x86;

/// A file with 4K pages (wVer 36), measured on files Outlook wrote; no specification has it.
/// Pages are 4096 bytes and end in a trailer of 24, the first sixteen as in a Unicode file and
/// then 1 (u16) and six bytes of zero. A bit of an allocation map stands for 512 bytes, so one
/// map covers 4072 * 8 * 512 bytes; the first is at 0x22000, and only the allocation map was
/// seen at the head of a section. Blocks are at most 64 KB with their trailer of 24: the first
/// sixteen as in a Unicode file, then 2 (u16), the size of the data before it was compressed
/// (u16) and four bytes of zero. A block-tree page has 4056 bytes of entries, then cEnt and
/// cEntMax as u16, cbEnt and cLevel. A block-tree leaf has the size before compression
/// between the size and the references. The density list's trailer ends where the header's
/// space does, at 0x22000, and the page is empty.
pub(crate) mod big {
    pub(crate) const PAGE: usize = 4096;
    pub(crate) const BODY: usize = PAGE - TRAILER;
    pub(crate) const TRAILER: usize = 24;
    pub(crate) const SLOT: usize = 512;
    pub(crate) const SECTION: usize = BODY * 8 * SLOT;
    pub(crate) const FIRST_AMAP: u64 = 0x22000;
    pub(crate) const DLIST: u64 = FIRST_AMAP - PAGE as u64;
    pub(crate) const AREA: usize = 4056;
    pub(crate) const MAX_DATA: usize = 65536 - TRAILER;
}

/// An ANSI data file is addressed with 32 bits, and the programs that made such files stop
/// at 2 GB.
const ANSI_END: u64 = 1 << 31;
/// A block's reference count is 16 bits; past this a new copy is written instead.
const MAX_REFS: u16 = 0xFFF0;

/// Which map pages stand at the head of section `index`, after its allocation map: a page map
/// before every eighth ([MS-PST] 2.2.2.7.3), a free map where the 128 bytes in the header and
/// then each 496-byte page run out ([MS-PST] 1.3.2.7), a free page map likewise ([MS-PST]
/// 1.3.2.8). The specification gives the coverage of the last two, not their offsets: these are
/// worked out from the coverage and have not been compared with a file Outlook wrote.
pub(crate) fn maps(index: u64) -> (bool, bool, bool) {
    const FMAP_FIRST: u64 = 128;
    const FMAP_EVERY: u64 = 496;
    const FPMAP_FIRST: u64 = 128 * 8 * 8;
    const FPMAP_EVERY: u64 = 496 * 8 * 8;
    let pmap = index.is_multiple_of(8);
    let fmap = index >= FMAP_FIRST && (index - FMAP_FIRST).is_multiple_of(FMAP_EVERY);
    let fpmap = index >= FPMAP_FIRST && (index - FPMAP_FIRST).is_multiple_of(FPMAP_EVERY);
    (pmap, fmap, fpmap)
}

pub(crate) fn section_base(index: u64) -> u64 {
    FIRST_AMAP.saturating_add(index.saturating_mul(SECTION as u64))
}

/// How data blocks are encoded in a file of the kind `ver`: permuted, as in the data files
/// Outlook makes ([MS-PST] 2.2.2.6), and not at all with 4K pages, as in the files with 4K
/// pages Outlook was seen to make, whose Inbox Repair Tool reads the blocks of such a file as
/// they are stored.
pub(crate) fn crypt_of(ver: Ver) -> u8 {
    if ver == Ver::Unicode4k { 0 } else { 1 }
}

const fn crc_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            k += 1;
        }
        table[i] = c;
        i += 1;
    }
    table
}

static CRC: [u32; 256] = crc_table();

/// The checksum of [MS-PST] 5.3: the reflected CRC-32 polynomial, started at zero and not
/// inverted at the end (the specification's eight tables are the one-table sum taken eight
/// bytes at a time).
pub(crate) fn crc(data: &[u8]) -> u32 {
    let mut c = 0u32;
    for b in data {
        c = CRC.get(((c ^ u32::from(*b)) & 0xFF) as usize).copied().unwrap_or(0) ^ (c >> 8);
    }
    c
}

/// The signature of a block or page ([MS-PST] 5.5).
pub(crate) fn sig(ib: u64, bid: u64) -> u16 {
    let x = (ib ^ bid) as u32;
    ((x >> 16) ^ x) as u16
}

fn cannot(what: &str) -> PstError {
    PstError::Write(what.to_string())
}

fn put(buf: &mut [u8], at: usize, bytes: &[u8]) {
    if let Some(slot) = at.checked_add(bytes.len()).and_then(|end| buf.get_mut(at..end)) {
        slot.copy_from_slice(bytes);
    }
}

/// A 512-byte page: its body, then the trailer ([MS-PST] 2.2.2.7.1).
pub(crate) fn page(body: &[u8], ptype: u8, wsig: u16, bid: u64) -> Vec<u8> {
    page_of(false, body, ptype, wsig, bid)
}

/// A page of either kind. An ANSI page has 500 bytes before a trailer of twelve, in which the
/// id (four bytes) comes before the checksum.
fn page_of(ansi: bool, body: &[u8], ptype: u8, wsig: u16, bid: u64) -> Vec<u8> {
    if ansi {
        const BODY: usize = PAGE - 12;
        let mut out = vec![0u8; PAGE];
        put(&mut out, 0, body.get(..BODY.min(body.len())).unwrap_or_default());
        let sum = crc(out.get(..BODY).unwrap_or_default());
        put(&mut out, BODY, &[ptype, ptype]);
        put(&mut out, BODY + 2, &wsig.to_le_bytes());
        put(&mut out, BODY + 4, &(bid as u32).to_le_bytes());
        put(&mut out, BODY + 8, &sum.to_le_bytes());
        return out;
    }
    let mut out = vec![0u8; PAGE];
    put(&mut out, 0, body.get(..PAGE_BODY.min(body.len())).unwrap_or_default());
    let sum = crc(out.get(..PAGE_BODY).unwrap_or_default());
    put(&mut out, PAGE_BODY, &[ptype, ptype]);
    put(&mut out, PAGE_BODY + 2, &wsig.to_le_bytes());
    put(&mut out, PAGE_BODY + 4, &sum.to_le_bytes());
    put(&mut out, PAGE_BODY + 8, &bid.to_le_bytes());
    out
}

/// A page of a file with 4K pages (see `big`).
fn big_page(body: &[u8], ptype: u8, wsig: u16, bid: u64) -> Vec<u8> {
    let mut out = vec![0u8; big::PAGE];
    put(&mut out, 0, body.get(..big::BODY.min(body.len())).unwrap_or_default());
    let sum = crc(out.get(..big::BODY).unwrap_or_default());
    put(&mut out, big::BODY, &[ptype, ptype]);
    put(&mut out, big::BODY + 2, &wsig.to_le_bytes());
    put(&mut out, big::BODY + 4, &sum.to_le_bytes());
    put(&mut out, big::BODY + 8, &bid.to_le_bytes());
    put(&mut out, big::BODY + 16, &1u16.to_le_bytes());
    out
}

/// A map page over 496 bytes of `bits`: its id is its own offset and it carries no signature
/// ([MS-PST] 2.2.2.7.1). In an ANSI file four bytes of padding come before the bits
/// ([MS-PST] 2.2.2.7.2.1).
fn map_page(ansi: bool, bits: &[u8], ptype: u8, ib: u64) -> Vec<u8> {
    if ansi {
        let mut body = vec![0u8; 4];
        body.extend(bits.get(..PAGE_BODY.min(bits.len())).unwrap_or_default());
        return page_of(true, &body, ptype, 0, ib);
    }
    page(bits, ptype, 0, ib)
}

fn hash(data: &[u8], internal: bool) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325 ^ u64::from(internal);
    for b in data {
        h = (h ^ u64::from(*b)).wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

#[derive(Clone, Copy)]
struct Block {
    bid: u64,
    ib: u64,
    cb: u16,
    refs: u16,
}

/// One entry of a subnode tree: a node inside a node.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Sub {
    pub nid: u32,
    pub data: u64,
    pub sub: u64,
}

pub(crate) struct NdbWriter<W: Read + Write + Seek> {
    out: W,
    crypt: u8,
    /// An ANSI file: ids and offsets of four bytes.
    ansi: bool,
    /// A file with 4K pages.
    big: bool,
    /// The code page of the message whose parts are being written, for the text of an ANSI
    /// file; none outside a message.
    text: Option<u32>,
    /// The section being filled, from its allocation map on, and its number.
    section: Vec<u8>,
    index: u64,
    /// Free 64-byte slots of every section already written.
    free: Vec<u16>,
    /// Sections that carry a free map, filled in at the end.
    fmaps: Vec<u64>,
    /// Every block, in the order of its id.
    blocks: Vec<Block>,
    /// What a block's bytes hash to, to find one written before.
    seen: HashMap<u64, u32>,
    nodes: Vec<(u32, u64, u64, u32)>,
    /// How many bytes of node data were given to `data` so far.
    counted: u64,
    next_block: u64,
    next_page: u64,
    /// The highest index used so far for each of the 32 kinds of node id.
    last: [u32; 32],
    /// dwUnique of the file this one takes the place of.
    unique: u32,
    /// The file this writes into, when it is not a new one.
    onto: Option<Box<onto::Onto>>,
}

#[path = "write_onto.rs"]
mod onto;
pub(crate) use onto::Onto;

#[cfg(test)]
thread_local! {
    /// Nodes a test has the writer leave out: a file as an older version of this program,
    /// or another program, wrote it.
    pub(crate) static LEAVE_OUT: std::cell::RefCell<Vec<u32>> = const { std::cell::RefCell::new(Vec::new()) };
}

impl<W: Read + Write + Seek> NdbWriter<W> {
    /// `crypt` is how data blocks are encoded: 0 not at all, 1 permuted, 2 cyclic
    /// ([MS-PST] 2.2.2.6 bCryptMethod).
    #[cfg(test)]
    pub(crate) fn new(out: W, crypt: u8) -> Result<Self, PstError> {
        Self::of(out, crypt, Ver::Unicode)
    }

    /// A writer of a file of the kind `ver`.
    pub(crate) fn of(out: W, crypt: u8, ver: Ver) -> Result<Self, PstError> {
        if crypt > 2 {
            return Err(cannot("an encoding this version does not write"));
        }
        let big = ver == Ver::Unicode4k;
        let mut w = Self {
            out,
            crypt,
            ansi: ver == Ver::Ansi,
            big,
            text: None,
            section: Vec::with_capacity(if big { big::SECTION } else { SECTION }),
            index: 0,
            free: Vec::new(),
            fmaps: Vec::new(),
            blocks: Vec::new(),
            seen: HashMap::new(),
            nodes: Vec::new(),
            next_block: 1,
            next_page: 1,
            last: [0; 32],
            unique: 0,
            counted: 0,
            onto: None,
        };
        w.open_section();
        Ok(w)
    }

    fn base(&self) -> u64 {
        if self.big { big::FIRST_AMAP.saturating_add(self.index.saturating_mul(big::SECTION as u64)) } else { section_base(self.index) }
    }

    pub(crate) fn ansi(&self) -> bool {
        self.ansi
    }

    /// Whether the file has 4K pages.
    pub(crate) fn big(&self) -> bool {
        self.big
    }

    fn page_len(&self) -> usize {
        if self.big { big::PAGE } else { PAGE }
    }

    fn page_body(&self) -> usize {
        if self.big { big::BODY } else { PAGE_BODY }
    }

    fn slot(&self) -> usize {
        if self.big { big::SLOT } else { SLOT }
    }

    fn section_len(&self) -> usize {
        if self.big { big::SECTION } else { SECTION }
    }

    fn first_amap(&self) -> u64 {
        if self.big { big::FIRST_AMAP } else { FIRST_AMAP }
    }

    fn page_bytes(&self, body: &[u8], ptype: u8, wsig: u16, bid: u64) -> Vec<u8> {
        if self.big { big_page(body, ptype, wsig, bid) } else { page_of(self.ansi, body, ptype, wsig, bid) }
    }

    fn map_bytes(&self, bits: &[u8], ptype: u8, ib: u64) -> Vec<u8> {
        if self.big { big_page(bits, ptype, 0, ib) } else { map_page(self.ansi, bits, ptype, ib) }
    }

    /// Say whose parts are written from here on: a message with this code page, or none.
    /// Gives what was said before, to be put back.
    pub(crate) fn within(&mut self, text: Option<u32>) -> Option<u32> {
        std::mem::replace(&mut self.text, text)
    }

    pub(crate) fn text(&self) -> Option<u32> {
        self.text
    }

    /// How many bytes of node data were written so far: the difference between two readings
    /// is the size of what was written between them.
    pub(crate) fn counted(&self) -> u64 {
        self.counted
    }

    /// Count `n` bytes of node data that are kept where they are stored, not written again:
    /// they are part of the item's size all the same.
    pub(crate) fn count(&mut self, n: u64) {
        self.counted = self.counted.saturating_add(n);
    }

    /// A block's trailer: sixteen bytes, twelve in an ANSI file ([MS-PST] 2.2.2.8.1), 24 in
    /// one with 4K pages.
    fn trailer(&self) -> usize {
        if self.big {
            big::TRAILER
        } else if self.ansi {
            12
        } else {
            TRAILER
        }
    }

    /// The most bytes one block holds.
    pub(crate) fn max_data(&self) -> usize {
        if self.big { big::MAX_DATA } else { 8192 - self.trailer() }
    }

    /// The bytes of an id in a block that names others.
    fn id(&self) -> usize {
        if self.ansi { 4 } else { 8 }
    }

    /// How many block ids fit an XBLOCK, and how many entries a subnode leaf and a subnode
    /// index block ([MS-PST] 2.2.2.8.3.2.1, 2.2.2.8.3.3.1.2, 2.2.2.8.3.3.2.2). The entries of
    /// a subnode block begin after eight bytes, after four in an ANSI file.
    fn per_x(&self) -> usize {
        (self.max_data() - 8) / self.id()
    }

    fn per_sl(&self) -> usize {
        (self.max_data() - self.id()) / (3 * self.id())
    }

    fn per_si(&self) -> usize {
        (self.max_data() - self.id()) / (2 * self.id())
    }

    fn push_id(&self, body: &mut Vec<u8>, id: u64) {
        if self.ansi {
            body.extend((id as u32).to_le_bytes());
        } else {
            body.extend(id.to_le_bytes());
        }
    }

    fn open_section(&mut self) {
        self.section.clear();
        // The allocation map is filled in when the section is full.
        self.section.resize(self.page_len(), 0);
        if self.big {
            return;
        }
        let (pmap, fmap, fpmap) = maps(self.index);
        // The page maps are kept for older readers only: every bit set says "no page free here"
        // ([MS-PST] 2.2.2.7.3.1, 2.2.2.7.6.1).
        if pmap {
            let ib = self.base() + self.section.len() as u64;
            self.section.extend(map_page(self.ansi, &[0xFF; PAGE_BODY], PT_PMAP, ib));
        }
        if fmap {
            self.fmaps.push(self.index);
            self.section.resize(self.section.len() + PAGE, 0);
        }
        if fpmap {
            let ib = self.base() + self.section.len() as u64;
            self.section.extend(map_page(self.ansi, &[0xFF; PAGE_BODY], PT_FPMAP, ib));
        }
    }

    /// Write the section out with its allocation map. Space was handed out in order, so the map
    /// is a run of set bits; the run is rounded up to a whole byte (eight slots, one page), so
    /// that up to 448 bytes at the end of the used part count as used.
    fn flush(&mut self) -> Result<(), PstError> {
        let (section, slot) = (self.section_len(), self.slot());
        // Eight slots are one page in a file of either kind.
        let used = self.section.len().next_multiple_of(slot * 8).min(section);
        let mut bits = vec![0u8; self.page_body()];
        for b in bits.iter_mut().take(used / slot / 8) {
            *b = 0xFF;
        }
        let base = self.base();
        let amap = self.map_bytes(&bits, PT_AMAP, base);
        put(&mut self.section, 0, &amap);
        self.section.resize(section, 0);
        self.out.seek(SeekFrom::Start(base))?;
        self.out.write_all(&self.section)?;
        self.free.push(u16::try_from((section - used) / slot).unwrap_or(0));
        Ok(())
    }

    /// Room for `size` bytes at a multiple of `align`, in this section or the next.
    fn room(&mut self, size: usize, align: usize) -> Result<usize, PstError> {
        let section = self.section_len();
        let mut at = self.section.len().next_multiple_of(align);
        if at.saturating_add(size) > section {
            self.flush()?;
            self.index = self.index.checked_add(1).ok_or_else(|| cannot("the file would be too large"))?;
            if self.ansi && self.base().saturating_add(SECTION as u64) > ANSI_END {
                return Err(cannot("the mailbox is too large for an ANSI data file (2 GB)"));
            }
            self.open_section();
            at = self.section.len().next_multiple_of(align);
            if at.saturating_add(size) > section {
                return Err(cannot("a block that does not fit a section"));
            }
        }
        self.section.resize(at + size, 0);
        Ok(at)
    }

    /// Bytes written earlier: from the section in memory, or back from the file.
    fn stored(&mut self, ib: u64, len: usize) -> Result<Vec<u8>, PstError> {
        let base = self.base();
        if ib >= base {
            let at = usize::try_from(ib - base).map_err(|_| cannot("an offset out of range"))?;
            return at.checked_add(len).and_then(|end| self.section.get(at..end)).map(<[u8]>::to_vec).ok_or_else(|| cannot("an offset out of range"));
        }
        let mut buf = vec![0u8; len];
        self.out.seek(SeekFrom::Start(ib))?;
        self.out.read_exact(&mut buf)?;
        Ok(buf)
    }

    /// One block ([MS-PST] 2.2.2.8): its id, and whether it was there already.
    fn block(&mut self, data: &[u8], internal: bool) -> Result<(u64, bool), PstError> {
        if data.is_empty() || data.len() > self.max_data() {
            return Err(cannot("a block of a size the format does not have"));
        }
        let key = hash(data, internal);
        if let Some(&i) = self.seen.get(&key)
            && let Some(b) = self.blocks.get(i as usize).copied()
            && usize::from(b.cb) == data.len()
            && (b.bid & 2 != 0) == internal
            && b.refs < MAX_REFS
        {
            let mut old = self.stored(b.ib, data.len())?;
            if !internal {
                match self.crypt {
                    1 => crypt::permute_decode(&mut old),
                    2 => crypt::cyclic(&mut old, b.bid as u32),
                    _ => {}
                }
            }
            if old == data {
                if let Some(b) = self.blocks.get_mut(i as usize) {
                    b.refs += 1;
                }
                return Ok((b.bid, true));
            }
        }
        if let Some(bid) = self.known(key, data, internal)? {
            return Ok((bid, true));
        }
        // Block ids go up by four; bit 1 marks an internal block ([MS-PST] 2.2.2.2).
        let bid = self.next_block << 2 | if internal { 2 } else { 0 };
        let most: u64 = if self.ansi { 1 << 30 } else { 1 << 61 };
        self.next_block = self.next_block.checked_add(1).filter(|n| *n < most).ok_or_else(|| cannot("too many blocks"))?;
        let tail = self.trailer();
        let slot = self.slot();
        let size = (data.len() + tail).next_multiple_of(slot);
        let at = self.room(size, slot)?;
        let ib = self.base() + at as u64;
        let mut body = data.to_vec();
        if !internal {
            // Only data blocks are encoded ([MS-PST] 2.2.2.8.3.1.1).
            match self.crypt {
                1 => crypt::permute_encode(&mut body),
                2 => crypt::cyclic(&mut body, bid as u32),
                _ => {}
            }
        }
        let cb = body.len() as u16;
        // BLOCKTRAILER ([MS-PST] 2.2.2.8.1): cb, wSig, dwCRC of the cb bytes as stored, bid.
        // In an ANSI file the id, four bytes, comes before the checksum.
        let mut trailer = [0u8; big::TRAILER];
        put(&mut trailer, 0, &cb.to_le_bytes());
        put(&mut trailer, 2, &sig(ib, bid).to_le_bytes());
        if self.ansi {
            put(&mut trailer, 4, &(bid as u32).to_le_bytes());
            put(&mut trailer, 8, &crc(&body).to_le_bytes());
        } else {
            put(&mut trailer, 4, &crc(&body).to_le_bytes());
            put(&mut trailer, 8, &bid.to_le_bytes());
        }
        if self.big {
            // Then 2 and the size before compression; nothing written here is compressed.
            put(&mut trailer, 16, &2u16.to_le_bytes());
            put(&mut trailer, 18, &cb.to_le_bytes());
        }
        put(&mut self.section, at, &body);
        put(&mut self.section, at + size - tail, trailer.get(..tail).unwrap_or_default());
        // A block starts with two references: its entry in the block tree and whoever asked for it.
        self.seen.insert(key, u32::try_from(self.blocks.len()).map_err(|_| cannot("too many blocks"))?);
        self.blocks.push(Block { bid, ib, cb, refs: 2 });
        Ok((bid, false))
    }

    /// Give back a reference that was taken for a block which turned out not to be needed.
    fn release(&mut self, bid: u64) {
        if bid == 0 {
            return;
        }
        // Into a file that is there, the block may be one that only its finder asked for: it
        // is then named by nothing and gets no entry.
        let least = if self.onto.is_some() { 1 } else { 2 };
        match self.blocks.binary_search_by_key(&bid, |b| b.bid) {
            Ok(i) => {
                if let Some(b) = self.blocks.get_mut(i)
                    && b.refs > least
                {
                    b.refs -= 1;
                }
            }
            Err(_) => self.release_old(bid),
        }
    }

    /// An internal block that points at others. When the same block exists already, the
    /// references just taken on its children are given back: they are held once, by the one block.
    fn internal(&mut self, body: &[u8], kids: impl Iterator<Item = u64>) -> Result<u64, PstError> {
        let (bid, shared) = self.block(body, true)?;
        if shared {
            for kid in kids {
                self.release(kid);
            }
        }
        Ok(bid)
    }

    /// XBLOCK (level 1) or XXBLOCK (level 2) over `kids`, each with the bytes below it
    /// ([MS-PST] 2.2.2.8.3.2.1, 2.2.2.8.3.2.2).
    fn xblock(&mut self, level: u8, kids: &[(u64, u64)]) -> Result<u64, PstError> {
        let total: u64 = kids.iter().map(|k| k.1).sum();
        let total = u32::try_from(total).map_err(|_| cannot("an item larger than 4 GB"))?;
        let mut body = Vec::with_capacity(8 + kids.len() * 8);
        body.extend([1, level]);
        body.extend((kids.len() as u16).to_le_bytes());
        body.extend(total.to_le_bytes());
        for (bid, _) in kids {
            self.push_id(&mut body, *bid);
        }
        self.internal(&body, kids.iter().map(|k| k.0))
    }

    /// A node's data from its blocks, each at most `max_data` bytes: one block, or a data tree over
    /// them ([MS-PST] 2.2.2.8.3.2). No data at all is block id 0.
    pub(crate) fn data<I, B>(&mut self, chunks: I) -> Result<u64, PstError>
    where
        I: IntoIterator<Item = B>,
        B: AsRef<[u8]>,
    {
        let per_x = self.per_x();
        let mut leaves: Vec<(u64, u64)> = Vec::new();
        for chunk in chunks {
            let chunk = chunk.as_ref();
            if chunk.is_empty() {
                continue;
            }
            if leaves.len() >= per_x * per_x {
                return Err(cannot("an item with more blocks than a data tree holds"));
            }
            let (bid, _) = self.block(chunk, false)?;
            self.counted = self.counted.saturating_add(chunk.len() as u64);
            leaves.push((bid, chunk.len() as u64));
        }
        match leaves.as_slice() {
            [] => Ok(0),
            [(bid, _)] => Ok(*bid),
            all if all.len() <= per_x => self.xblock(1, all),
            all => {
                let mut tops = Vec::with_capacity(all.len() / per_x + 1);
                for group in all.chunks(per_x) {
                    tops.push((self.xblock(1, group)?, group.iter().map(|k| k.1).sum()));
                }
                self.xblock(2, &tops)
            }
        }
    }

    /// A node's data from one run of bytes, cut where blocks end.
    #[cfg(test)]
    pub(crate) fn stream(&mut self, bytes: &[u8]) -> Result<u64, PstError> {
        self.data(bytes.chunks(self.max_data()))
    }

    /// The subnode tree over `subs`: one SLBLOCK, or an SIBLOCK over several
    /// ([MS-PST] 2.2.2.8.3.3). No subnodes is block id 0.
    pub(crate) fn subnodes(&mut self, mut subs: Vec<Sub>) -> Result<u64, PstError> {
        subs.sort_by_key(|s| s.nid);
        if subs.windows(2).any(|w| w.first().map(|s| s.nid) == w.get(1).map(|s| s.nid)) {
            return Err(cannot("two subnodes with one id"));
        }
        if subs.len() > self.per_sl() * self.per_si() {
            return Err(cannot("more subnodes than a subnode tree holds"));
        }
        // The four bytes of padding after cEnt are not there in an ANSI file.
        let padding: &[u8] = if self.ansi { &[] } else { &[0; 4] };
        let mut leaves: Vec<(u32, u64)> = Vec::new();
        for group in subs.chunks(self.per_sl()) {
            // btype 2, cLevel 0, cEnt, padding; then SLENTRY: nid (as wide as an id), bidData, bidSub.
            let mut body = Vec::with_capacity(8 + group.len() * 24);
            body.extend([2, 0]);
            body.extend((group.len() as u16).to_le_bytes());
            body.extend(padding);
            for s in group {
                self.note(s.nid);
                self.push_id(&mut body, u64::from(s.nid));
                self.push_id(&mut body, s.data);
                self.push_id(&mut body, s.sub);
            }
            let bid = self.internal(&body, group.iter().flat_map(|s| [s.data, s.sub]))?;
            leaves.push((group.first().map_or(0, |s| s.nid), bid));
        }
        match leaves.as_slice() {
            [] => Ok(0),
            [(_, bid)] => Ok(*bid),
            all => {
                // btype 2, cLevel 1, cEnt, padding; then SIENTRY: nid (as wide as an id), bid.
                let mut body = Vec::with_capacity(8 + all.len() * 16);
                body.extend([2, 1]);
                body.extend((all.len() as u16).to_le_bytes());
                body.extend(padding);
                for (nid, bid) in all {
                    self.push_id(&mut body, u64::from(*nid));
                    self.push_id(&mut body, *bid);
                }
                self.internal(&body, all.iter().map(|l| l.1))
            }
        }
    }

    /// Carry on from the counters of the file this one takes the place of: no node id that
    /// file gave out, to a node since deleted or not, is given out again, and the header's
    /// count of its writings goes on.
    pub(crate) fn carry(&mut self, counters: (u32, [u32; 32])) {
        self.unique = counters.0;
        for (last, next) in self.last.iter_mut().zip(counters.1) {
            // An index has 27 bits; a counter beyond that is not one.
            *last = (*last).max(next.saturating_sub(1).min((1 << 27) - 1));
        }
    }

    fn note(&mut self, nid: u32) {
        if let Some(last) = self.last.get_mut((nid & 0x1F) as usize) {
            *last = (*last).max(nid >> 5);
        }
    }

    /// A top-level node ([MS-PST] 2.2.2.7.7.4). `data` and `sub` are ids this writer gave out.
    pub(crate) fn node(&mut self, nid: u32, parent: u32, data: u64, sub: u64) {
        #[cfg(test)]
        if LEAVE_OUT.with(|l| l.borrow().contains(&nid)) {
            return;
        }
        self.note(nid);
        self.nodes.push((nid, data, sub, parent));
    }

    fn put_page(&mut self, ptype: u8, body: &[u8]) -> Result<(u64, u64), PstError> {
        let len = self.page_len();
        let at = self.room(len, len)?;
        let ib = self.base() + at as u64;
        // Pages take their ids from a counter of their own ([MS-PST] 2.2.2.6 bidNextP).
        let bid = self.next_page;
        self.next_page += 1;
        let bytes = self.page_bytes(body, ptype, sig(ib, bid), bid);
        put(&mut self.section, at, &bytes);
        Ok((bid, ib))
    }

    /// A B-tree of pages over `leaves` (key, entry bytes of `width`), which are in key order
    /// ([MS-PST] 2.2.2.7.7). Returns the root's BREF.
    fn tree(&mut self, ptype: u8, width: usize, leaves: Vec<(u64, [u8; 32])>) -> Result<(u64, u64), PstError> {
        let mut items = leaves;
        let mut width = width;
        let mut level = 0u8;
        // An ANSI page has 496 bytes of entries and its counts straight after them, 500 bytes
        // before the trailer; its BTENTRY is three times four bytes.
        let (area, body_size) = if self.big {
            (big::AREA, big::BODY)
        } else if self.ansi {
            (496, PAGE - 12)
        } else {
            (BT_AREA, PAGE_BODY)
        };
        loop {
            let per = area / width;
            let mut up: Vec<(u64, [u8; 32])> = Vec::with_capacity(items.len() / per + 1);
            let mut last = (0u64, 0u64);
            let mut groups: Vec<&[(u64, [u8; 32])]> = items.chunks(per).collect();
            if groups.is_empty() {
                groups.push(&[]);
            }
            for group in groups {
                let mut body = vec![0u8; body_size];
                for (i, (_, entry)) in group.iter().enumerate() {
                    put(&mut body, i * width, entry.get(..width).unwrap_or_default());
                }
                // cEnt, cEntMax, cbEnt, cLevel (then four bytes of padding in a Unicode file);
                // with 4K pages the two counts are u16 (measured).
                if self.big {
                    put(&mut body, area, &(group.len() as u16).to_le_bytes());
                    put(&mut body, area + 2, &(per as u16).to_le_bytes());
                    put(&mut body, area + 4, &[width as u8, level]);
                } else {
                    put(&mut body, area, &[group.len() as u8, per as u8, width as u8, level]);
                }
                let (bid, ib) = self.put_page(ptype, &body)?;
                last = (bid, ib);
                // BTENTRY ([MS-PST] 2.2.2.7.7.2): the first key below, then the page's BREF.
                let key = group.first().map_or(0, |g| g.0);
                let mut entry = [0u8; 32];
                if self.ansi {
                    put(&mut entry, 0, &(key as u32).to_le_bytes());
                    put(&mut entry, 4, &(bid as u32).to_le_bytes());
                    put(&mut entry, 8, &(ib as u32).to_le_bytes());
                } else {
                    put(&mut entry, 0, &key.to_le_bytes());
                    put(&mut entry, 8, &bid.to_le_bytes());
                    put(&mut entry, 16, &ib.to_le_bytes());
                }
                up.push((key, entry));
            }
            if up.len() == 1 {
                return Ok(last);
            }
            items = up;
            width = if self.ansi { 12 } else { 24 };
            level = level.checked_add(1).ok_or_else(|| cannot("an index that is too deep"))?;
        }
    }

    /// Write the two B-trees, the maps, the density list and the header, and hand the file back.
    pub(crate) fn finish(mut self) -> Result<W, PstError> {
        self.nodes.sort_by_key(|n| n.0);
        if self.nodes.windows(2).any(|w| w.first().map(|n| n.0) == w.get(1).map(|n| n.0)) {
            return Err(cannot("two nodes with one id"));
        }
        let (ansi, big) = (self.ansi, self.big);
        // BBTENTRY ([MS-PST] 2.2.2.7.7.3): BREF, cb, cRef, padding; twelve bytes without
        // padding in an ANSI file.
        let blocks: Vec<(u64, [u8; 32])> = self
            .blocks
            .iter()
            .map(|b| {
                let mut e = [0u8; 32];
                if ansi {
                    put(&mut e, 0, &(b.bid as u32).to_le_bytes());
                    put(&mut e, 4, &(b.ib as u32).to_le_bytes());
                    put(&mut e, 8, &b.cb.to_le_bytes());
                    put(&mut e, 10, &b.refs.to_le_bytes());
                } else if big {
                    // With 4K pages the size before compression comes before the references.
                    put(&mut e, 0, &b.bid.to_le_bytes());
                    put(&mut e, 8, &b.ib.to_le_bytes());
                    put(&mut e, 16, &b.cb.to_le_bytes());
                    put(&mut e, 18, &b.cb.to_le_bytes());
                    put(&mut e, 20, &b.refs.to_le_bytes());
                } else {
                    put(&mut e, 0, &b.bid.to_le_bytes());
                    put(&mut e, 8, &b.ib.to_le_bytes());
                    put(&mut e, 16, &b.cb.to_le_bytes());
                    put(&mut e, 18, &b.refs.to_le_bytes());
                }
                (b.bid, e)
            })
            .collect();
        let (bbt_bid, bbt_ib) = self.tree(PT_BBT, if ansi { 12 } else { 24 }, blocks)?;
        // NBTENTRY ([MS-PST] 2.2.2.7.7.4): nid (in 8 bytes), bidData, bidSub, nidParent, padding;
        // four times four bytes in an ANSI file.
        let nodes: Vec<(u64, [u8; 32])> = self
            .nodes
            .iter()
            .map(|(nid, data, sub, parent)| {
                let mut e = [0u8; 32];
                if ansi {
                    put(&mut e, 0, &nid.to_le_bytes());
                    put(&mut e, 4, &(*data as u32).to_le_bytes());
                    put(&mut e, 8, &(*sub as u32).to_le_bytes());
                    put(&mut e, 12, &parent.to_le_bytes());
                } else {
                    put(&mut e, 0, &u64::from(*nid).to_le_bytes());
                    put(&mut e, 8, &data.to_le_bytes());
                    put(&mut e, 16, &sub.to_le_bytes());
                    put(&mut e, 24, &parent.to_le_bytes());
                }
                (u64::from(*nid), e)
            })
            .collect();
        let (nbt_bid, nbt_ib) = self.tree(PT_NBT, if ansi { 16 } else { 32 }, nodes)?;
        self.flush()?;
        let last_amap = self.base();
        let eof = last_amap + self.section_len() as u64;
        if ansi && eof > ANSI_END {
            return Err(cannot("the mailbox is too large for an ANSI data file (2 GB)"));
        }

        // Free maps: one byte for each allocation map from this section on, the longest run of
        // free slots there, at most 255 ([MS-PST] 2.2.2.7.5.1).
        for &at in &self.fmaps.clone() {
            let mut body = vec![0u8; PAGE_BODY];
            for (i, b) in body.iter_mut().enumerate() {
                let free = usize::try_from(at).ok().and_then(|at| self.free.get(at + i)).copied().unwrap_or(0);
                *b = free.min(255) as u8;
            }
            let ib = section_base(at) + if maps(at).0 { 2 * PAGE as u64 } else { PAGE as u64 };
            self.out.seek(SeekFrom::Start(ib))?;
            self.out.write_all(&map_page(ansi, &body, PT_FMAP, ib))?;
        }

        let mut head = vec![0u8; usize::try_from(self.first_amap()).unwrap_or(0)];
        let free_bytes: u64 = self.free.iter().map(|f| u64::from(*f) * self.slot() as u64).sum();
        if big {
            // The density list of the files measured was empty, with the header's bidNextP.
            let dlist = big_page(&[], PT_DLIST, sig(big::DLIST, self.next_page), self.next_page);
            put(&mut head, big::DLIST as usize, &dlist);
            self.head_unicode(&mut head, true, eof, last_amap, free_bytes, (nbt_bid, nbt_ib), (bbt_bid, bbt_ib));
        } else if ansi {
            self.head_ansi(&mut head, true, eof, last_amap, free_bytes, (nbt_bid, nbt_ib), (bbt_bid, bbt_ib))?;
        } else {
            let roomy = self.free.iter().copied().enumerate().filter_map(|(i, f)| Some((u32::try_from(i).ok()?, f))).collect();
            let dlist = self.dlist_page(1, roomy);
            put(&mut head, DLIST as usize, &dlist);
            self.head_unicode(&mut head, true, eof, last_amap, free_bytes, (nbt_bid, nbt_ib), (bbt_bid, bbt_ib));
        }
        self.out.seek(SeekFrom::Start(0))?;
        self.out.write_all(&head)?;
        self.out.flush()?;
        Ok(self.out)
    }

    /// Density list ([MS-PST] 2.2.2.7.4.2): the allocation maps with room (a map's number and
    /// its free slots), the emptiest first. An entry is the number in 20 bits and the slots in 12.
    fn dlist_page(&mut self, flags: u8, mut roomy: Vec<(u32, u16)>) -> Vec<u8> {
        roomy.retain(|(index, free)| *free > 0 && *index < 1 << 20);
        roomy.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        roomy.truncate(476 / 4);
        let mut dlist = vec![0u8; PAGE_BODY];
        // DFL_BACKFILL_COMPLETE, the count, padding, then the map the next allocation tries.
        put(&mut dlist, 0, &[flags, roomy.len() as u8]);
        put(&mut dlist, 4, &roomy.first().map_or(0, |r| r.0).to_le_bytes());
        for (i, (index, free)) in roomy.iter().enumerate() {
            let entry = (*index & 0xF_FFFF) | u32::from(*free & 0xFFF) << 20;
            put(&mut dlist, 8 + i * 4, &entry.to_le_bytes());
        }
        let bid = self.next_page;
        self.next_page += 1;
        page(&dlist, PT_DLIST, sig(DLIST, bid), bid)
    }

    /// rgnid: for each kind of node id, where the next one starts; never before where the
    /// header in `head` had it.
    fn head_nids(&self, head: &mut [u8], at: usize) {
        for kind in 0..32usize {
            let start: u32 = match kind {
                0x03 => 0x4000,
                0x04 => 0x1_0000,
                0x08 => 0x8000,
                _ => 0x400,
            };
            let had = crate::ndb::u32_at(head, at + kind * 4).unwrap_or(0);
            let next = self.last.get(kind).copied().unwrap_or(0).saturating_add(1).max(start).max(had);
            put(head, at + kind * 4, &next.to_le_bytes());
        }
    }

    /// HEADER, Unicode ([MS-PST] 2.2.2.6), with ROOT at 180 ([MS-PST] 2.2.2.5), into `head`:
    /// all of it for a `fresh` file, and what a writing changes for one that is there.
    #[allow(clippy::too_many_arguments)]
    fn head_unicode(&self, head: &mut [u8], fresh: bool, eof: u64, last_amap: u64, free_bytes: u64, nbt: (u64, u64), bbt: (u64, u64)) {
        let mut unique = self.unique;
        if fresh && self.big {
            // wVer 36, and the two maps of the header left zero (measured). The `.ost` files
            // measured have wVerClient 12; Outlook's Inbox Repair Tool asks 19 of a `.pst`.
            put(head, 0, b"!BDN");
            put(head, 8, b"SM");
            put(head, 10, &36u16.to_le_bytes());
            put(head, 12, &19u16.to_le_bytes());
            put(head, 14, &[1, 1]);
            put(head, 512, &[0x80, self.crypt]);
        } else if fresh {
            put(head, 0, b"!BDN");
            put(head, 8, b"SM");
            put(head, 10, &23u16.to_le_bytes());
            put(head, 12, &19u16.to_le_bytes());
            put(head, 14, &[1, 1]);
            // The two maps kept in the header are no longer used and are all ones.
            put(head, 256, &[0xFF; 256]);
            put(head, 512, &[0x80, self.crypt]);
        } else {
            unique = crate::ndb::u32_at(head, 40).unwrap_or(unique);
        }
        put(head, 32, &self.next_page.to_le_bytes());
        put(head, 40, &unique.wrapping_add(1).to_le_bytes());
        self.head_nids(head, 44);
        put(head, 184, &eof.to_le_bytes());
        put(head, 192, &last_amap.to_le_bytes());
        put(head, 200, &free_bytes.to_le_bytes());
        put(head, 216, &nbt.0.to_le_bytes());
        put(head, 224, &nbt.1.to_le_bytes());
        put(head, 232, &bbt.0.to_le_bytes());
        put(head, 240, &bbt.1.to_le_bytes());
        // fAMapValid: VALID_AMAP2.
        put(head, 248, &[2]);
        put(head, 516, &(self.next_block << 2).to_le_bytes());
        // dwCRCPartial: 471 bytes from offset 8; dwCRCFull: 516 bytes from offset 8.
        let partial = crc(head.get(8..479).unwrap_or_default());
        put(head, 4, &partial.to_le_bytes());
        let full = crc(head.get(8..524).unwrap_or_default());
        put(head, 524, &full.to_le_bytes());
    }

    /// HEADER, ANSI ([MS-PST] 2.2.2.6): the ids and offsets are four bytes, ROOT is at 164,
    /// there is one checksum and no density list.
    #[allow(clippy::too_many_arguments)]
    fn head_ansi(
        &self,
        head: &mut [u8],
        fresh: bool,
        eof: u64,
        last_amap: u64,
        free_bytes: u64,
        nbt: (u64, u64),
        bbt: (u64, u64),
    ) -> Result<(), PstError> {
        let small = |v: u64| u32::try_from(v).map_err(|_| cannot("the mailbox is too large for an ANSI data file (2 GB)"));
        let mut unique = self.unique;
        if fresh {
            put(head, 0, b"!BDN");
            put(head, 8, b"SM");
            put(head, 10, &14u16.to_le_bytes());
            put(head, 12, &19u16.to_le_bytes());
            put(head, 14, &[1, 1]);
            // The two maps kept in the header are no longer used and are all ones.
            put(head, 204, &[0xFF; 256]);
            put(head, 460, &[0x80, self.crypt]);
        } else {
            unique = crate::ndb::u32_at(head, 32).unwrap_or(unique);
        }
        put(head, 24, &small(self.next_block << 2)?.to_le_bytes());
        put(head, 28, &small(self.next_page)?.to_le_bytes());
        put(head, 32, &unique.wrapping_add(1).to_le_bytes());
        self.head_nids(head, 36);
        put(head, 168, &small(eof)?.to_le_bytes());
        put(head, 172, &small(last_amap)?.to_le_bytes());
        put(head, 176, &small(free_bytes)?.to_le_bytes());
        put(head, 184, &small(nbt.0)?.to_le_bytes());
        put(head, 188, &small(nbt.1)?.to_le_bytes());
        put(head, 192, &small(bbt.0)?.to_le_bytes());
        put(head, 196, &small(bbt.1)?.to_le_bytes());
        // fAMapValid: VALID_AMAP2.
        put(head, 200, &[2]);
        // dwCRCPartial: 471 bytes from offset 8.
        let partial = crc(head.get(8..479).unwrap_or_default());
        put(head, 4, &partial.to_le_bytes());
        Ok(())
    }
}
