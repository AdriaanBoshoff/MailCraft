//! Writing into a data file that is there: the changed nodes and their blocks go into the
//! free end of the file's last section and into new sections after it, the pages of the two
//! B-trees that a change reaches are written anew (the rest stay where they are), and the
//! header is patched to name the new roots. Nothing that held mail before is written over:
//! a block or a page that is no longer named stays where it is, counted as used, until the
//! file is next written whole.
//!
//! What the file holds is read first (`Onto::read`) and held against the specification as
//! it goes; a file that is anything but plainly sound is not written into, and is written
//! whole as before. The caller keeps what is written over (`Onto::undo`) before any of it.

use super::*;
use crate::ndb::{u16_at, u32_at, u64_at};
use crate::undo::Undo;
use std::collections::{BTreeMap, HashSet};

const MAX_PAGES: usize = 4_000_000;
const MAX_ENTRIES: usize = 40_000_000;
/// A B-tree is no deeper than this ([MS-PST] 2.2.2.7.7.1 cLevel is at most 8).
const MAX_LEVELS: usize = 8;
/// No more than this is read of the blocks of one save's changed nodes to find them again.
const MAX_LEARNT: usize = 64 << 20;
/// The header and the density list: everything before the first allocation map.
const HEAD: usize = FIRST_AMAP as usize;

/// A block the file has.
#[derive(Clone, Copy)]
struct Was {
    ib: u64,
    cb: u16,
    /// The references its entry counts, and how many there are now.
    had: u16,
    refs: u32,
}

/// A page of a B-tree: its id, where it is, the first key below it, and how many pages of
/// the level below (or entries, for a leaf) it holds.
#[derive(Clone, Copy)]
struct PageAt {
    bid: u64,
    ib: u64,
    key: u64,
    kids: usize,
}

/// A B-tree's pages, level for level from the leaves up, each level in key order.
struct Tree {
    levels: Vec<Vec<PageAt>>,
    root: (u64, u64),
}

/// What becomes of one entry of a B-tree.
#[derive(Clone, Copy)]
enum Change {
    Put([u8; 32]),
    /// The same block, counted another number of times.
    Refs(u16),
    Drop,
}

/// A data file as it is, for writing into.
pub(crate) struct Onto {
    ansi: bool,
    crypt: u8,
    len: u64,
    /// The file's first 0x4400 bytes.
    head: Vec<u8>,
    blocks: HashMap<u64, Was>,
    nodes: HashMap<u32, (u64, u64, u32)>,
    bbt: Tree,
    nbt: Tree,
    /// The last section: its number, its bytes, and how many of them are in use.
    first: u64,
    last: Vec<u8>,
    used: usize,
    /// The free map that covers the last section: its section, where it is, its page.
    fmap: Option<(u64, u64, Vec<u8>)>,
    /// Free bytes in the sections before the last.
    free_before: u64,
    /// The density list's flags and entries (a section and its free slots).
    dlist: Option<(u8, Vec<(u32, u16)>)>,
    next_block: u64,
    next_page: u64,
    /// Bytes that blocks and pages named by the trees take.
    live: u64,
    /// Blocks of nodes that are being written again, by what their bytes hash to.
    known: HashMap<u64, u64>,
    learnt: usize,
    /// The roots of the two trees and the sum of each: pages a save leaves as they are.
    proof: [(u64, u32); 2],
}

fn word_at(ansi: bool, b: &[u8], at: usize) -> Option<u64> {
    if ansi { u32_at(b, at).map(u64::from) } else { u64_at(b, at) }
}

/// A page's body when its trailer is sound ([MS-PST] 2.2.2.7.1): its type twice, the sum
/// of its body, and its id (a map's id is where it is).
fn sound(ansi: bool, page: &[u8], ptype: u8, bid: Option<u64>, ib: u64) -> Option<&[u8]> {
    let body = if ansi { PAGE - 12 } else { PAGE_BODY };
    if page.len() != PAGE || page.get(body..body + 2) != Some(&[ptype, ptype]) {
        return None;
    }
    let (sum, own) =
        if ansi { (u32_at(page, body + 8)?, u64::from(u32_at(page, body + 4)?)) } else { (u32_at(page, body + 4)?, u64_at(page, body + 8)?) };
    if sum != crc(page.get(..body)?) || own != bid.unwrap_or(ib) {
        return None;
    }
    if bid.is_some() && u16_at(page, body + 2) != Some(sig(ib, own)) {
        return None;
    }
    page.get(..body)
}

/// The entries of a B-tree page: its level, and each entry's bytes.
fn entries(ansi: bool, body: &[u8], leaf_width: usize) -> Option<(u8, Vec<&[u8]>)> {
    let area = if ansi { 496 } else { BT_AREA };
    let (count, max, size, level) =
        (usize::from(*body.get(area)?), usize::from(*body.get(area + 1)?), usize::from(*body.get(area + 2)?), *body.get(area + 3)?);
    let want = if level == 0 { leaf_width } else { 3 * if ansi { 4 } else { 8 } };
    if size != want || count > max || count.checked_mul(size)? > area {
        return None;
    }
    Some((level, (0..count).map(|i| body.get(i * size..(i + 1) * size)).collect::<Option<Vec<_>>>()?))
}

/// The blocks an internal block names ([MS-PST] 2.2.2.8.3.2.1, 2.2.2.8.3.3.1, 2.2.2.8.3.3.2);
/// without the subnodes of a subnode leaf's entries when `subs` is not asked for.
fn named(ansi: bool, body: &[u8], subs: bool) -> Option<Vec<u64>> {
    let word = if ansi { 4 } else { 8 };
    let count = usize::from(u16_at(body, 2)?);
    let subs_at = if ansi { 4 } else { 8 };
    let (start, words, which): (usize, usize, &[usize]) = match (body.first()?, body.get(1)?) {
        (1, 1 | 2) => (8, 1, &[0]),
        (2, 0) if subs => (subs_at, 3, &[1, 2]),
        (2, 0) => (subs_at, 3, &[1]),
        (2, 1) => (subs_at, 2, &[1]),
        _ => return None,
    };
    if start.checked_add(count.checked_mul(words * word)?)? > body.len() {
        return None;
    }
    let mut out = Vec::with_capacity(count * which.len());
    for i in 0..count {
        for n in which {
            out.push(word_at(ansi, body, start + (i * words + n) * word)? & !1);
        }
    }
    Some(out)
}

struct Reader<'a, R: Read + Seek> {
    src: &'a mut R,
    ansi: bool,
    len: u64,
    seen: HashSet<u64>,
    top_page: u64,
}

impl<R: Read + Seek> Reader<'_, R> {
    fn read(&mut self, at: u64, n: usize) -> Option<Vec<u8>> {
        if at.checked_add(n as u64)? > self.len {
            return None;
        }
        let mut buf = vec![0u8; n];
        self.src.seek(SeekFrom::Start(at)).ok()?;
        self.src.read_exact(&mut buf).ok()?;
        Some(buf)
    }

    /// The tree below the page `bid` at `ib`, its leaves' entries given to `leaf` in order.
    fn tree(&mut self, root: (u64, u64), ptype: u8, width: usize, leaf: &mut dyn FnMut(&[u8]) -> Option<()>) -> Option<Tree> {
        let mut levels: Vec<Vec<PageAt>> = Vec::new();
        let mut last: Option<u64> = None;
        let mut count = 0usize;
        self.branch(root, ptype, width, None, &mut levels, &mut last, &mut count, leaf)?;
        // A leaf with nothing in it is the whole of an empty tree and nothing else.
        if levels.first()?.len() > 1 && levels.first()?.iter().any(|p| p.kids == 0) {
            return None;
        }
        Some(Tree { levels, root })
    }

    #[allow(clippy::too_many_arguments)]
    fn branch(
        &mut self,
        (bid, ib): (u64, u64),
        ptype: u8,
        width: usize,
        level: Option<u8>,
        levels: &mut Vec<Vec<PageAt>>,
        last: &mut Option<u64>,
        count: &mut usize,
        leaf: &mut dyn FnMut(&[u8]) -> Option<()>,
    ) -> Option<u64> {
        if !ib.is_multiple_of(PAGE as u64) || ib < FIRST_AMAP || !self.seen.insert(ib) || self.seen.len() > MAX_PAGES {
            return None;
        }
        let page = self.read(ib, PAGE)?;
        let body = sound(self.ansi, &page, ptype, Some(bid), ib)?;
        self.top_page = self.top_page.max(bid);
        let (depth, all) = entries(self.ansi, body, width)?;
        if level.is_some_and(|l| l != depth) || usize::from(depth) > MAX_LEVELS {
            return None;
        }
        if levels.len() <= usize::from(depth) {
            levels.resize_with(usize::from(depth) + 1, Vec::new);
        }
        let mut first = None;
        for e in &all {
            let key = word_at(self.ansi, e, 0)?;
            if depth == 0 {
                if last.is_some_and(|l| l >= key) {
                    return None;
                }
                *last = Some(key);
                *count += 1;
                if *count > MAX_ENTRIES {
                    return None;
                }
                leaf(e)?;
                first = first.or(Some(key));
            } else {
                let word = if self.ansi { 4 } else { 8 };
                let kid = (word_at(self.ansi, e, word)?, word_at(self.ansi, e, 2 * word)?);
                let below = self.branch(kid, ptype, width, Some(depth - 1), levels, last, count, leaf)?;
                first = first.or(Some(below));
            }
        }
        // A page of pages has pages below it.
        if depth > 0 && all.is_empty() {
            return None;
        }
        let key = first.unwrap_or(0);
        levels.get_mut(usize::from(depth))?.push(PageAt { bid, ib, key, kids: all.len() });
        Some(key)
    }
}

impl Onto {
    /// What the data file `src` holds, when it can be written into: a Unicode or an ANSI
    /// file with 512-byte pages whose header, maps and trees are sound and as the
    /// specification has them. Nothing otherwise, and such a file is written whole.
    pub(crate) fn read<R: Read + Seek>(src: &mut R) -> Option<Self> {
        let len = src.seek(SeekFrom::End(0)).ok()?;
        let mut r = Reader { src, ansi: false, len, seen: HashSet::new(), top_page: 0 };
        let head = r.read(0, HEAD)?;
        if head.get(..4) != Some(b"!BDN") || head.get(8..10) != Some(b"SM") {
            return None;
        }
        let ansi = match u16_at(&head, 10)? {
            14 | 15 => true,
            23 => false,
            _ => return None,
        };
        r.ansi = ansi;
        if u32_at(&head, 4)? != crc(head.get(8..479)?) || (!ansi && u32_at(&head, 524)? != crc(head.get(8..524)?)) {
            return None;
        }
        // The fields of the header, as `check` has them ([MS-PST] 2.2.2.6).
        let at: [usize; 9] = if ansi { [168, 172, 176, 184, 192, 200, 460, 461, 24] } else { [184, 192, 200, 216, 232, 248, 512, 513, 516] };
        let field = |n: usize, k: usize| word_at(ansi, &head, at.get(n).copied()? + k * if ansi { 4 } else { 8 });
        let byte = |n: usize| head.get(at.get(n).copied()?).copied();
        let (eof, last_amap, free) = (field(0, 0)?, field(1, 0)?, field(2, 0)?);
        let crypt = byte(7)?;
        // VALID_AMAP2: the maps are as the file is.
        if byte(5)? != 2 || byte(6)? != 0x80 || crypt > 2 {
            return None;
        }
        let section = SECTION as u64;
        if eof != len || eof < FIRST_AMAP + section || !(eof - FIRST_AMAP).is_multiple_of(section) || last_amap != eof - section {
            return None;
        }
        let first = (eof - FIRST_AMAP) / section - 1;
        if ansi && eof > ANSI_END {
            return None;
        }

        // The last section and its allocation map: what is in use ends with the map's last
        // bit that is set, taken up to a whole page.
        let last = r.read(last_amap, SECTION)?;
        let bits_at = if ansi { 4 } else { 0 };
        let bits = sound(ansi, last.get(..PAGE)?, PT_AMAP, None, last_amap)?.get(bits_at..bits_at + PAGE_BODY)?.to_vec();
        let used = (bits.iter().rposition(|b| *b != 0)? + 1) * 8 * SLOT;
        let (pmap, has_fmap, fpmap) = maps(first);
        let maps_end = PAGE * (1 + usize::from(pmap) + usize::from(has_fmap) + usize::from(fpmap));
        if used < maps_end || used > SECTION {
            return None;
        }
        let free_last: u64 = bits.iter().map(|b| u64::from(b.count_zeros())).sum::<u64>() * SLOT as u64;
        let free_before = free.checked_sub(free_last)?;

        // The free map that has a byte for the last section, if the file is long enough to
        // have one.
        let fmap_place = |index: u64| section_base(index) + if maps(index).0 { 2 * PAGE as u64 } else { PAGE as u64 };
        let mut fmap = None;
        if let Some(index) = (0..=first).rev().take(PAGE_BODY).find(|i| maps(*i).1) {
            let ib = fmap_place(index);
            let page = r.read(ib, PAGE)?;
            sound(ansi, &page, PT_FMAP, None, ib)?;
            fmap = Some((index, ib, page));
        }
        // The density list, where there is one.
        let mut dlist = None;
        if !ansi
            && let Some(page) = head.get(DLIST as usize..DLIST as usize + PAGE)
            && page.get(PAGE_BODY) == Some(&PT_DLIST)
            // One that is not sound is made anew, as the specification has a reader do.
            && let Some(body) = sound(false, page, PT_DLIST, Some(u64_at(page, PAGE_BODY + 8)?), DLIST)
        {
            let count = usize::from(*body.get(1)?).min(476 / 4);
            let list = (0..count).map(|i| u32_at(body, 8 + i * 4).map(|e| (e & 0xF_FFFF, (e >> 20) as u16))).collect::<Option<Vec<_>>>()?;
            dlist = Some((*body.first()?, list));
        }

        let word = if ansi { 4 } else { 8 };
        let max_data = 8192 - if ansi { 12 } else { TRAILER };
        let trailer = if ansi { 12 } else { TRAILER };
        let mut blocks: HashMap<u64, Was> = HashMap::new();
        let mut live = 0u64;
        let mut top_block = 0u64;
        let bbt = r.tree((field(4, 0)?, field(4, 1)?), PT_BBT, 3 * word, &mut |e| {
            let (bid, ib, cb, refs) = (word_at(ansi, e, 0)?, word_at(ansi, e, word)?, u16_at(e, 2 * word)?, u16_at(e, 2 * word + 2)?);
            let stored = (usize::from(cb) + trailer).next_multiple_of(SLOT) as u64;
            let within = ib.checked_sub(FIRST_AMAP)? % section;
            // Bit 0 of an id is not part of it and is not set in a file as specified.
            if bid & 1 != 0 || cb == 0 || usize::from(cb) > max_data || refs == 0 || !ib.is_multiple_of(SLOT as u64) {
                return None;
            }
            if within < PAGE as u64 || within + stored > section || ib + stored > len {
                return None;
            }
            live += stored;
            top_block = top_block.max(bid);
            blocks.insert(bid, Was { ib, cb, had: refs, refs: u32::from(refs) }).is_none().then_some(())
        })?;
        let mut nodes: HashMap<u32, (u64, u64, u32)> = HashMap::new();
        let nbt = r.tree((field(3, 0)?, field(3, 1)?), PT_NBT, 4 * word, &mut |e| {
            let nid = u32::try_from(word_at(ansi, e, 0)?).ok()?;
            let (data, sub, parent) = (word_at(ansi, e, word)?, word_at(ansi, e, 2 * word)?, u32_at(e, 3 * word)?);
            nodes.insert(nid, (data, sub, parent)).is_none().then_some(())
        })?;
        live += (r.seen.len() * PAGE) as u64;
        // The ids the header gives out next are past every one in use.
        let next_block = field(8, 0)? >> 2;
        if next_block == 0 || top_block >> 2 >= next_block {
            return None;
        }
        let next_page = word_at(ansi, &head, if ansi { 28 } else { 32 })?.max(r.top_page.checked_add(1)?);
        let proof = [(nbt.root.1, crc(&r.read(nbt.root.1, PAGE)?)), (bbt.root.1, crc(&r.read(bbt.root.1, PAGE)?))];
        Some(Self {
            ansi,
            crypt,
            len,
            head,
            blocks,
            nodes,
            bbt,
            nbt,
            first,
            last,
            used,
            fmap,
            free_before,
            dlist,
            next_block,
            next_page,
            live,
            known: HashMap::new(),
            learnt: 0,
            proof,
        })
    }

    /// What a save writes over, to be kept before it does: the header, the last section,
    /// and the free map when it is not in that section.
    pub(crate) fn undo(&self) -> Undo {
        let mut parts = vec![(0u64, self.head.clone()), (section_base(self.first), self.last.clone())];
        if let Some((index, ib, page)) = &self.fmap
            && *index != self.first
        {
            parts.push((*ib, page.clone()));
        }
        Undo { len: self.len, proof: self.proof, next: None, parts }
    }

    /// Bytes of the file that neither hold anything the trees name nor are called free:
    /// what earlier saves left behind.
    pub(crate) fn waste(&self) -> u64 {
        let sections = self.first + 1;
        let map_pages: u64 = (0..sections.min(1 << 22))
            .map(|i| {
                let (p, f, fp) = maps(i);
                1 + u64::from(p) + u64::from(f) + u64::from(fp)
            })
            .sum();
        let free = self.free_before + (SECTION - self.used) as u64;
        self.len.saturating_sub(FIRST_AMAP).saturating_sub(self.live).saturating_sub(free).saturating_sub(map_pages * PAGE as u64)
    }

    /// How long the file is.
    pub(crate) fn size(&self) -> u64 {
        self.len
    }

    /// Where the last section begins: what is before it is not written.
    pub(crate) fn since(&self) -> u64 {
        section_base(self.first)
    }
}

impl<W: Read + Write + Seek> NdbWriter<W> {
    /// A writer that goes on in the file `out`, which holds what `onto` says.
    pub(crate) fn onto(out: W, onto: Onto) -> Self {
        let mut section = onto.last.clone();
        section.truncate(onto.used);
        Self {
            out,
            crypt: onto.crypt,
            ansi: onto.ansi,
            big: false,
            text: None,
            section,
            index: onto.first,
            free: Vec::new(),
            fmaps: onto.fmap.iter().map(|f| f.0).collect(),
            blocks: Vec::new(),
            seen: HashMap::new(),
            nodes: Vec::new(),
            next_block: onto.next_block,
            next_page: onto.next_page,
            last: [0; 32],
            unique: 0,
            counted: 0,
            onto: Some(Box::new(onto)),
        }
    }

    /// Whether this writes into a file that is there.
    pub(crate) fn in_place(&self) -> bool {
        self.onto.is_some()
    }

    /// A block the file has, as it was given to whoever wrote it.
    fn decoded(&mut self, bid: u64, was: Was) -> Result<Vec<u8>, PstError> {
        let mut data = self.stored(was.ib, usize::from(was.cb))?;
        if bid & 2 == 0 {
            match self.crypt {
                1 => crypt::permute_decode(&mut data),
                2 => crypt::cyclic(&mut data, bid as u32),
                _ => {}
            }
        }
        Ok(data)
    }

    /// Take one more reference to a block the file has: it is named once more.
    pub(crate) fn keep(&mut self, bid: u64) -> Result<u64, PstError> {
        if bid == 0 {
            return Ok(0);
        }
        let was = self.onto.as_mut().and_then(|o| o.blocks.get_mut(&(bid & !1))).ok_or_else(|| cannot("a block the file does not have"))?;
        was.refs = was.refs.checked_add(1).ok_or_else(|| cannot("a block named too often"))?;
        Ok(bid & !1)
    }

    /// Give back a reference to a block the file has.
    pub(super) fn release_old(&mut self, bid: u64) {
        if let Some(was) = self.onto.as_mut().and_then(|o| o.blocks.get_mut(&(bid & !1)))
            && was.refs > 1
        {
            was.refs -= 1;
        }
    }

    /// A block the file has with these very bytes, among those of nodes said to be `near`.
    pub(super) fn known(&mut self, key: u64, data: &[u8], internal: bool) -> Result<Option<u64>, PstError> {
        let Some(onto) = self.onto.as_ref() else { return Ok(None) };
        let Some(bid) = onto.known.get(&key).copied() else { return Ok(None) };
        let Some(was) = onto.blocks.get(&bid).copied() else { return Ok(None) };
        if usize::from(was.cb) != data.len() || (bid & 2 != 0) != internal || was.refs >= u32::from(MAX_REFS) {
            return Ok(None);
        }
        if self.decoded(bid, was)? != data {
            return Ok(None);
        }
        self.keep(bid).map(Some)
    }

    /// Say that the node with this data and these subnodes is about to be written again:
    /// a block of it (of its data, of its subnodes' data) that comes out the same is not
    /// written a second time. A table of ten thousand rows of which one changed is then one
    /// new block and not the table again.
    pub(crate) fn near(&mut self, data: u64, sub: u64) {
        if self.onto.is_none() {
            return;
        }
        let mut stack = vec![(data & !1, 0u8), (sub & !1, 0u8)];
        let mut visited = 0usize;
        while let Some((bid, depth)) = stack.pop() {
            visited += 1;
            let Some(was) = self.onto.as_ref().and_then(|o| o.blocks.get(&bid)).copied() else { continue };
            let learnt = self.onto.as_ref().map_or(usize::MAX, |o| o.learnt);
            if bid == 0 || depth > 4 || visited > 1 << 16 || learnt > MAX_LEARNT {
                continue;
            }
            let Ok(body) = self.decoded(bid, was) else { continue };
            let ansi = self.ansi;
            if let Some(onto) = self.onto.as_mut() {
                onto.learnt = onto.learnt.saturating_add(body.len());
                onto.known.insert(hash(&body, bid & 2 != 0), bid);
            }
            if bid & 2 != 0
                && let Some(kids) = named(ansi, &body, false)
            {
                stack.extend(kids.into_iter().map(|k| (k, depth + 1)));
            }
        }
    }

    /// Give up one reference to a block the file had, and with the last one those the
    /// block held itself. `dead` takes the blocks nothing names any more.
    fn drop_ref(&mut self, onto: &mut Onto, bid: u64, dead: &mut HashSet<u64>, steps: &mut usize) -> Result<(), PstError> {
        let mut stack = vec![bid & !1];
        while let Some(bid) = stack.pop() {
            if bid == 0 {
                continue;
            }
            *steps = steps.checked_sub(1).ok_or_else(|| cannot("the file's blocks name each other without end"))?;
            let was = onto.blocks.get_mut(&bid).ok_or_else(|| cannot("a node names a block the file does not have"))?;
            if was.refs <= 1 {
                return Err(cannot("a block is counted less often than it is named"));
            }
            was.refs -= 1;
            if was.refs == 1 {
                dead.insert(bid);
                if bid & 2 != 0 {
                    let was = *was;
                    let body = self.decoded(bid, was)?;
                    stack.extend(named(self.ansi, &body, true).ok_or_else(|| cannot("a block that names others is not as specified"))?);
                }
            }
        }
        Ok(())
    }

    /// Pages of one level over `items`, which are in key order: as many as they take, one
    /// (empty) for none.
    fn pages(&mut self, ptype: u8, width: usize, level: u8, items: &[(u64, [u8; 32])]) -> Result<Vec<PageAt>, PstError> {
        let (area, body_size) = if self.ansi { (496, PAGE - 12) } else { (BT_AREA, PAGE_BODY) };
        let per = area / width;
        let mut groups: Vec<&[(u64, [u8; 32])]> = items.chunks(per).collect();
        if groups.is_empty() {
            groups.push(&[]);
        }
        let mut out = Vec::with_capacity(groups.len());
        for group in groups {
            let mut body = vec![0u8; body_size];
            for (i, (_, entry)) in group.iter().enumerate() {
                put(&mut body, i * width, entry.get(..width).unwrap_or_default());
            }
            put(&mut body, area, &[group.len() as u8, per as u8, width as u8, level]);
            let (bid, ib) = self.put_page(ptype, &body)?;
            out.push(PageAt { bid, ib, key: group.first().map_or(0, |g| g.0), kids: group.len() });
        }
        Ok(out)
    }

    /// BTENTRY ([MS-PST] 2.2.2.7.7.2) for a page.
    fn bt_entry(&self, p: &PageAt) -> (u64, [u8; 32]) {
        let mut entry = [0u8; 32];
        if self.ansi {
            put(&mut entry, 0, &(p.key as u32).to_le_bytes());
            put(&mut entry, 4, &(p.bid as u32).to_le_bytes());
            put(&mut entry, 8, &(p.ib as u32).to_le_bytes());
        } else {
            put(&mut entry, 0, &p.key.to_le_bytes());
            put(&mut entry, 8, &p.bid.to_le_bytes());
            put(&mut entry, 16, &p.ib.to_le_bytes());
        }
        (p.key, entry)
    }

    /// A leaf's entries by key, read again from where it is.
    fn leaf(&mut self, at: &PageAt, ptype: u8, width: usize) -> Result<BTreeMap<u64, [u8; 32]>, PstError> {
        let gone = || cannot("a page of the file's index is no longer what it was");
        let page = self.stored(at.ib, PAGE)?;
        let body = sound(self.ansi, &page, ptype, Some(at.bid), at.ib).ok_or_else(gone)?;
        let (level, all) = entries(self.ansi, body, width).ok_or_else(gone)?;
        if level != 0 || all.len() != at.kids {
            return Err(gone());
        }
        let mut out = BTreeMap::new();
        for e in all {
            let mut entry = [0u8; 32];
            put(&mut entry, 0, e);
            out.insert(word_at(self.ansi, e, 0).ok_or_else(gone)?, entry);
        }
        Ok(out)
    }

    /// The tree with `delta` put into it: the leaves a change falls into are written anew,
    /// and so is every page above one that was; all others stay where they are. Gives the
    /// root.
    fn graft(&mut self, tree: &Tree, delta: &BTreeMap<u64, Change>, ptype: u8, width: usize) -> Result<(u64, u64), PstError> {
        if delta.is_empty() {
            return Ok(tree.root);
        }
        let odd = || cannot("the file's index is not as it was read");
        let word = self.id();
        let leaves = tree.levels.first().ok_or_else(odd)?;
        // For each page of a level, the pages that take its place; nothing when it stays.
        let mut now: Vec<Option<Vec<PageAt>>> = Vec::with_capacity(leaves.len());
        let mut changes = delta.iter().peekable();
        for (i, leaf) in leaves.iter().enumerate() {
            // A key below the next leaf's first belongs here.
            let end = leaves.get(i + 1).map(|n| n.key);
            let mut mine = Vec::new();
            while let Some((key, change)) = changes.peek().copied() {
                if end.is_some_and(|e| *key >= e) {
                    break;
                }
                mine.push((*key, *change));
                changes.next();
            }
            if mine.is_empty() {
                now.push(None);
                continue;
            }
            let mut held = self.leaf(leaf, ptype, width)?;
            for (key, change) in mine {
                match change {
                    Change::Put(entry) => {
                        held.insert(key, entry);
                    }
                    Change::Drop => {
                        held.remove(&key).ok_or_else(odd)?;
                    }
                    Change::Refs(refs) => {
                        // cRef follows cb, which follows the id and the offset ([MS-PST] 2.2.2.7.7.3).
                        put(held.get_mut(&key).ok_or_else(odd)?, 2 * word + 2, &refs.to_le_bytes());
                    }
                }
            }
            let items: Vec<(u64, [u8; 32])> = held.into_iter().collect();
            now.push(Some(if items.is_empty() { Vec::new() } else { self.pages(ptype, width, 0, &items)? }));
        }
        let mut below = leaves;
        for (level, pages) in tree.levels.iter().enumerate().skip(1) {
            let mut next = Vec::with_capacity(pages.len());
            let mut at = 0usize;
            for p in pages {
                let end = at.checked_add(p.kids).ok_or_else(odd)?;
                let (olds, news) = (below.get(at..end).ok_or_else(odd)?, now.get(at..end).ok_or_else(odd)?);
                at = end;
                if news.iter().all(Option::is_none) {
                    next.push(None);
                    continue;
                }
                let mut kids: Vec<PageAt> = Vec::new();
                for (old, new) in olds.iter().zip(news) {
                    match new {
                        None => kids.push(*old),
                        Some(pages) => kids.extend(pages),
                    }
                }
                let items: Vec<(u64, [u8; 32])> = kids.iter().map(|k| self.bt_entry(k)).collect();
                next.push(Some(if items.is_empty() { Vec::new() } else { self.pages(ptype, 3 * word, level as u8, &items)? }));
            }
            now = next;
            below = pages;
        }
        let Some(Some(mut top)) = now.pop() else { return Ok(tree.root) };
        let mut level = tree.levels.len();
        loop {
            match top.as_slice() {
                // Nothing is left: an empty leaf is the whole tree.
                [] => top = self.pages(ptype, width, 0, &[])?,
                [one] => return Ok((one.bid, one.ib)),
                many => {
                    if level > MAX_LEVELS {
                        return Err(cannot("an index that is too deep"));
                    }
                    let items: Vec<(u64, [u8; 32])> = many.iter().map(|k| self.bt_entry(k)).collect();
                    top = self.pages(ptype, 3 * word, level as u8, &items)?;
                    level += 1;
                }
            }
        }
    }

    /// End a writing into a file that is there: the two trees with what changed, the
    /// maps, and the header to be. Gives the file, its first 0x4400 bytes as they are to be
    /// (not yet written: the caller forces everything else to disk first), and the nodes
    /// that were written anew.
    pub(crate) fn finish_onto(mut self) -> Result<(W, Vec<u8>, HashSet<u32>), PstError> {
        let mut onto = *self.onto.take().ok_or_else(|| cannot("not a writing into a file that is there"))?;
        self.nodes.sort_by_key(|n| n.0);
        if self.nodes.windows(2).any(|w| w.first().map(|n| n.0) == w.get(1).map(|n| n.0)) {
            return Err(cannot("two nodes with one id"));
        }
        let ansi = self.ansi;
        // The nodes: what is new or other than it was, and what is gone.
        let mut nodes: BTreeMap<u64, Change> = BTreeMap::new();
        let mut changed: HashSet<u32> = HashSet::new();
        for (nid, data, sub, parent) in &self.nodes {
            if onto.nodes.get(nid) == Some(&(*data, *sub, *parent)) {
                continue;
            }
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
            nodes.insert(u64::from(*nid), Change::Put(e));
            changed.insert(*nid);
        }
        // Every node the file had held one reference to its data and one to its subnodes;
        // the nodes written now hold their own.
        let mut dead: HashSet<u64> = HashSet::new();
        // No more references are given up than the file counts.
        let counted = onto.blocks.values().fold(0usize, |n, b| n.saturating_add(b.refs as usize));
        let mut steps = counted.saturating_add(onto.nodes.len().saturating_mul(2)).saturating_add(64);
        let was: Vec<(u32, u64, u64)> = onto.nodes.iter().map(|(nid, n)| (*nid, n.0, n.1)).collect();
        for (nid, data, sub) in was {
            if self.nodes.binary_search_by_key(&nid, |n| n.0).is_err() {
                nodes.insert(u64::from(nid), Change::Drop);
            }
            self.drop_ref(&mut onto, data, &mut dead, &mut steps)?;
            self.drop_ref(&mut onto, sub, &mut dead, &mut steps)?;
        }
        // The blocks: the new ones, those counted otherwise, and those nothing names.
        let mut blocks: BTreeMap<u64, Change> = BTreeMap::new();
        for b in &self.blocks {
            // A block that was written and then found not to be needed is named by nothing.
            if b.refs < 2 {
                continue;
            }
            let mut e = [0u8; 32];
            if ansi {
                put(&mut e, 0, &(b.bid as u32).to_le_bytes());
                put(&mut e, 4, &(b.ib as u32).to_le_bytes());
                put(&mut e, 8, &b.cb.to_le_bytes());
                put(&mut e, 10, &b.refs.to_le_bytes());
            } else {
                put(&mut e, 0, &b.bid.to_le_bytes());
                put(&mut e, 8, &b.ib.to_le_bytes());
                put(&mut e, 16, &b.cb.to_le_bytes());
                put(&mut e, 18, &b.refs.to_le_bytes());
            }
            blocks.insert(b.bid, Change::Put(e));
        }
        for (bid, was) in &onto.blocks {
            if dead.contains(bid) && was.refs == 1 {
                blocks.insert(*bid, Change::Drop);
            } else if was.refs != u32::from(was.had) {
                let refs = u16::try_from(was.refs).ok().filter(|r| *r <= MAX_REFS).ok_or_else(|| cannot("a block named too often"))?;
                blocks.insert(*bid, Change::Refs(refs));
            }
        }
        let word = self.id();
        let bbt = self.graft(&onto.bbt, &blocks, PT_BBT, 3 * word)?;
        let nbt = self.graft(&onto.nbt, &nodes, PT_NBT, 4 * word)?;
        self.flush()?;
        let last_amap = self.base();
        let eof = last_amap + SECTION as u64;
        if ansi && eof > ANSI_END {
            return Err(cannot("the mailbox is too large for an ANSI data file (2 GB)"));
        }
        // Free maps: the bytes for the sections before the one this began in stay.
        let free_of = |free: &[u16], index: u64| index.checked_sub(onto.first).and_then(|i| free.get(usize::try_from(i).ok()?)).copied();
        for &at in &self.fmaps.clone() {
            let bits_at = if ansi { 4 } else { 0 };
            let mut body = match &onto.fmap {
                Some((index, _, page)) if *index == at => page.get(bits_at..bits_at + PAGE_BODY).unwrap_or_default().to_vec(),
                _ => Vec::new(),
            };
            body.resize(PAGE_BODY, 0);
            for (i, b) in body.iter_mut().enumerate() {
                if let Some(free) = free_of(&self.free, at + i as u64) {
                    *b = free.min(255) as u8;
                }
            }
            let ib = section_base(at) + if maps(at).0 { 2 * PAGE as u64 } else { PAGE as u64 };
            self.out.seek(SeekFrom::Start(ib))?;
            self.out.write_all(&map_page(ansi, &body, PT_FMAP, ib))?;
        }
        self.out.flush()?;

        let mut head = std::mem::take(&mut onto.head);
        let free_bytes = onto.free_before.saturating_add(self.free.iter().map(|f| u64::from(*f) * SLOT as u64).sum());
        if ansi {
            self.head_ansi(&mut head, false, eof, last_amap, free_bytes, nbt, bbt)?;
            return Ok((self.out, head, changed));
        }
        // The density list: what it had for the sections before, and what these have now.
        let (flags, mut roomy) = onto.dlist.take().unwrap_or((1, Vec::new()));
        roomy.retain(|(index, _)| u64::from(*index) < onto.first);
        for (i, free) in self.free.iter().enumerate() {
            if let Some(index) = u32::try_from(onto.first + i as u64).ok().filter(|n| *n < 1 << 20 && *free > 0) {
                roomy.push((index, *free));
            }
        }
        let dlist = self.dlist_page(flags, roomy);
        put(&mut head, DLIST as usize, &dlist);
        self.head_unicode(&mut head, false, eof, last_amap, free_bytes, nbt, bbt);
        Ok((self.out, head, changed))
    }
}
