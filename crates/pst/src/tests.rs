//! Tests against data files built here, byte by byte, from the layouts in [MS-PST]. The builder
//! writes only what the reader looks at (no block trailers, no allocation maps), so its output is a
//! fixture for this crate and not a file Outlook would open.

use super::*;
use mailcraft_model::codepage::to_utf16le;
use mailcraft_model::{RECIP_TO, pset};

struct Builder {
    wide: bool,
    crypt: u8,
    blocks: Vec<(u64, Vec<u8>)>,
    nodes: Vec<(u32, u64, u64, u32)>,
    next_bid: u64,
}

impl Builder {
    fn new(wide: bool, crypt: u8) -> Self {
        Self { wide, crypt, blocks: Vec::new(), nodes: Vec::new(), next_bid: 4 }
    }

    fn bid(&mut self, internal: bool) -> u64 {
        let bid = self.next_bid | if internal { 2 } else { 0 };
        self.next_bid += 4;
        bid
    }

    /// An external (data) block, encoded the way the file says its blocks are.
    fn data(&mut self, mut bytes: Vec<u8>) -> u64 {
        let bid = self.bid(false);
        match self.crypt {
            1 => crypt::permute_encode(&mut bytes),
            2 => crypt::cyclic(&mut bytes, bid as u32),
            _ => {}
        }
        self.blocks.push((bid, bytes));
        bid
    }

    fn id(&self, out: &mut Vec<u8>, v: u64) {
        if self.wide { out.extend(v.to_le_bytes()) } else { out.extend((v as u32).to_le_bytes()) }
    }

    /// A data tree: an XBLOCK over one data block per chunk.
    fn xblock(&mut self, chunks: &[&[u8]]) -> u64 {
        let kids: Vec<u64> = chunks.iter().map(|c| self.data(c.to_vec())).collect();
        let total: usize = chunks.iter().map(|c| c.len()).sum();
        let mut b = vec![1u8, 1];
        b.extend((kids.len() as u16).to_le_bytes());
        b.extend((total as u32).to_le_bytes());
        for k in kids {
            self.id(&mut b, k);
        }
        let bid = self.bid(true);
        self.blocks.push((bid, b));
        bid
    }

    /// A subnode tree: one SLBLOCK of (nid, data, sub).
    fn subnodes(&mut self, entries: &[(u32, u64, u64)]) -> u64 {
        let mut b = vec![2u8, 0];
        b.extend((entries.len() as u16).to_le_bytes());
        if self.wide {
            b.extend([0u8; 4]);
        }
        for &(nid, data, sub) in entries {
            self.id(&mut b, u64::from(nid));
            self.id(&mut b, data);
            self.id(&mut b, sub);
        }
        let bid = self.bid(true);
        self.blocks.push((bid, b));
        bid
    }

    fn node(&mut self, nid: u32, data: u64, sub: u64, parent: u32) {
        self.nodes.push((nid, data, sub, parent));
    }

    /// B-tree pages for `entries`: leaves, and one page above them when there are several.
    fn pages(&self, file: &mut Vec<u8>, entries: &[Vec<u8>]) -> u64 {
        let area = if self.wide { 488 } else { 496 };
        let size = entries.first().map_or(1, Vec::len);
        let page = |file: &mut Vec<u8>, level: u8, items: &[Vec<u8>]| -> u64 {
            let at = file.len() as u64;
            let mut p: Vec<u8> = items.concat();
            p.resize(area, 0);
            let item = items.first().map_or(size, Vec::len);
            p.extend([items.len() as u8, (area / item) as u8, item as u8, level]);
            p.resize(512, 0);
            file.extend(p);
            at
        };
        let leaves: Vec<u64> = entries.chunks(area / size).map(|c| page(file, 0, c)).collect();
        if leaves.len() == 1 {
            return leaves.first().copied().unwrap();
        }
        // BTENTRY: key, bid, ib.
        let upper: Vec<Vec<u8>> = leaves
            .iter()
            .map(|ib| {
                let mut e = Vec::new();
                self.id(&mut e, 0);
                self.id(&mut e, 0);
                self.id(&mut e, *ib);
                e
            })
            .collect();
        page(file, 1, &upper)
    }

    fn finish(self) -> Vec<u8> {
        let mut file = vec![0u8; 1024];
        let mut bbt = Vec::new();
        for (bid, bytes) in &self.blocks {
            let at = file.len() as u64;
            file.extend(bytes);
            file.resize(file.len().next_multiple_of(64), 0);
            let mut e = Vec::new();
            self.id(&mut e, *bid);
            self.id(&mut e, at);
            e.extend((bytes.len() as u16).to_le_bytes());
            e.extend(2u16.to_le_bytes());
            if self.wide {
                e.extend([0u8; 4]);
            }
            bbt.push(e);
        }
        let nbt: Vec<Vec<u8>> = self
            .nodes
            .iter()
            .map(|&(nid, data, sub, parent)| {
                let mut e = Vec::new();
                self.id(&mut e, u64::from(nid));
                self.id(&mut e, data);
                self.id(&mut e, sub);
                e.extend(parent.to_le_bytes());
                if self.wide {
                    e.extend([0u8; 4]);
                }
                e
            })
            .collect();
        let bbt_root = self.pages(&mut file, &bbt);
        let nbt_root = self.pages(&mut file, &nbt);
        file[..4].copy_from_slice(b"!BDN");
        file[8..10].copy_from_slice(b"SM");
        if self.wide {
            file[10..12].copy_from_slice(&23u16.to_le_bytes());
            file[224..232].copy_from_slice(&nbt_root.to_le_bytes());
            file[240..248].copy_from_slice(&bbt_root.to_le_bytes());
            file[513] = self.crypt;
        } else {
            file[10..12].copy_from_slice(&14u16.to_le_bytes());
            file[188..192].copy_from_slice(&(nbt_root as u32).to_le_bytes());
            file[196..200].copy_from_slice(&(bbt_root as u32).to_le_bytes());
            file[461] = self.crypt;
        }
        file
    }
}

/// A one-block heap-on-node; allocation n (1-based) has HID n << 5 and the first is the user root.
fn heap(sig: u8, allocs: &[Vec<u8>]) -> Vec<u8> {
    let mut body = vec![0u8, 0, 0xEC, sig, 0x20, 0, 0, 0, 0, 0, 0, 0];
    let mut offsets = Vec::new();
    for a in allocs {
        offsets.push(body.len() as u16);
        body.extend(a);
    }
    offsets.push(body.len() as u16);
    let map = body.len() as u16;
    body[..2].copy_from_slice(&map.to_le_bytes());
    body.extend((allocs.len() as u16).to_le_bytes());
    body.extend(0u16.to_le_bytes());
    for o in offsets {
        body.extend(o.to_le_bytes());
    }
    body
}

fn hid(index: usize) -> u32 {
    (index as u32) << 5
}

fn multi(values: &[Vec<u8>]) -> Vec<u8> {
    let mut out = (values.len() as u32).to_le_bytes().to_vec();
    let mut at = 4 + values.len() * 4;
    for v in values {
        out.extend((at as u32).to_le_bytes());
        at += v.len();
    }
    out.extend(values.concat());
    out
}

/// A value's bytes for the heap, or `None` when it sits in the 4-byte value field.
fn heap_bytes(v: &PropValue) -> Option<Vec<u8>> {
    Some(match v {
        PropValue::String(s) => to_utf16le(s),
        PropValue::Binary(b) => b.clone(),
        PropValue::Time(t) | PropValue::Int64(t) => t.to_le_bytes().to_vec(),
        PropValue::MultiString(list) => multi(&list.iter().map(|s| to_utf16le(s)).collect::<Vec<_>>()),
        PropValue::Other { raw, .. } => raw.clone(),
        _ => return None,
    })
}

/// A Property Context. `in_subnodes` are values kept in a subnode: (id, type, subnode id).
fn pc(props: &[(u16, PropValue)], in_subnodes: &[(u16, u16, u32)]) -> Vec<u8> {
    let mut allocs = vec![vec![0xB5, 2, 6, 0, 0x40, 0, 0, 0], Vec::new()];
    let mut records: Vec<(u16, u16, u32)> = in_subnodes.to_vec();
    for (id, v) in props {
        let value = match heap_bytes(v) {
            Some(bytes) => {
                allocs.push(bytes);
                hid(allocs.len())
            }
            None => u32::from_le_bytes(v.fixed_bytes().unwrap()[..4].try_into().unwrap()),
        };
        records.push((*id, v.ty(), value));
    }
    records.sort_unstable();
    allocs[1] =
        records.iter().flat_map(|(id, ty, v)| [id.to_le_bytes().to_vec(), ty.to_le_bytes().to_vec(), v.to_le_bytes().to_vec()].concat()).collect();
    heap(0xBC, &allocs)
}

/// A Table Context with its row matrix in the heap.
fn tc(wide: bool, columns: &[(u16, u16)], rows: &[(u32, Vec<Option<PropValue>>)]) -> Vec<u8> {
    let cell_size = |ty: u16| match ty {
        ptype::BOOL => 1,
        ty => ptype::fixed_size(ty).unwrap_or(4),
    };
    // Columns are laid out widest first: 8- and 4-byte cells, then 2-byte, then 1-byte.
    let mut order: Vec<usize> = (0..columns.len()).collect();
    order.sort_by_key(|i| std::cmp::Reverse(cell_size(columns[*i].1)));
    let mut offsets = vec![0usize; columns.len()];
    let (mut at, mut ends) = (0usize, [0usize; 3]);
    for &i in &order {
        offsets[i] = at;
        at += cell_size(columns[i].1);
        match cell_size(columns[i].1) {
            1 => {}
            2 => ends[1] = at,
            _ => ends[0] = at,
        }
    }
    ends[1] = ends[1].max(ends[0]);
    ends[2] = at;
    let row_size = at + columns.len().div_ceil(8);
    let mut info = vec![0x7C, columns.len() as u8];
    for v in [ends[0], ends[1], ends[2], row_size] {
        info.extend((v as u16).to_le_bytes());
    }
    info.extend(hid(2).to_le_bytes());
    info.extend(hid(4).to_le_bytes());
    info.extend(0u32.to_le_bytes());
    for (i, (id, ty)) in columns.iter().enumerate() {
        info.extend((u32::from(*id) << 16 | u32::from(*ty)).to_le_bytes());
        info.extend((offsets[i] as u16).to_le_bytes());
        info.extend([cell_size(*ty) as u8, i as u8]);
    }
    let mut allocs = vec![info, vec![0xB5, 4, if wide { 4 } else { 2 }, 0, 0x60, 0, 0, 0], Vec::new(), Vec::new()];
    let mut index: Vec<(u32, usize)> = Vec::new();
    let mut matrix = Vec::new();
    for (n, (id, cells)) in rows.iter().enumerate() {
        let mut row = vec![0u8; row_size];
        for (i, cell) in cells.iter().enumerate() {
            let Some(v) = cell else { continue };
            row[ends[2] + i / 8] |= 1 << (7 - i % 8);
            let bytes = match heap_bytes(v) {
                Some(b) if ptype::fixed_size(v.ty()).is_none() => {
                    allocs.push(b);
                    hid(allocs.len()).to_le_bytes().to_vec()
                }
                Some(b) => b,
                None => v.fixed_bytes().unwrap()[..cell_size(v.ty())].to_vec(),
            };
            row[offsets[i]..offsets[i] + bytes.len()].copy_from_slice(&bytes);
        }
        matrix.extend(row);
        index.push((*id, n));
    }
    index.sort_unstable();
    allocs[2] = index
        .iter()
        .flat_map(|(id, n)| {
            if wide {
                [id.to_le_bytes(), (*n as u32).to_le_bytes()].concat()
            } else {
                [id.to_le_bytes().to_vec(), (*n as u16).to_le_bytes().to_vec()].concat()
            }
        })
        .collect();
    allocs[3] = matrix;
    heap(0x7C, &allocs)
}

fn s(v: &str) -> PropValue {
    PropValue::String(v.to_string())
}

fn entry_id(nid: u32) -> PropValue {
    let mut b = vec![0u8; 20];
    b.extend(nid.to_le_bytes());
    PropValue::Binary(b)
}

const ROOT: u32 = 0x122;
const IPM: u32 = 0x8022;
const INBOX: u32 = 0x8042;
const DELETED: u32 = 0x8062;
const PROJECTS: u32 = 0x8082;
const MSG_HELLO: u32 = 0x20_0024;
const MSG_BIG: u32 = 0x20_0044;
const MSG_LOOSE: u32 = 0x20_0064;
const DELIVERED: i64 = 133_500_000_000_000_000;

fn big_body() -> String {
    "Quarterly numbers follow. ".repeat(400)
}

/// A small mailbox: Inbox (two messages, one subfolder), Deleted Items, and a subfolder whose
/// tables are missing so the reader has to fall back to the node index.
fn sample(wide: bool, crypt: u8) -> Vec<u8> {
    let mut b = Builder::new(wide, crypt);
    let folder_cols = [
        (pid::DISPLAY_NAME, ptype::STRING),
        (pid::CONTENT_COUNT, ptype::INT32),
        (pid::CONTENT_UNREAD_COUNT, ptype::INT32),
        (pid::SUBFOLDERS, ptype::BOOL),
        (pid::CONTAINER_CLASS, ptype::STRING),
    ];
    let folder_row = |nid: u32, name: &str, total: i32, unread: i32, kids: bool| {
        (
            nid,
            vec![
                Some(s(name)),
                Some(PropValue::Int32(total)),
                Some(PropValue::Int32(unread)),
                Some(PropValue::Bool(kids)),
                (nid == INBOX).then(|| s("IPF.Note")),
            ],
        )
    };

    // Message store and named property map.
    let store = pc(&[(pid::DISPLAY_NAME, s("Test Mailbox")), (pid::IPM_SUBTREE_ENTRY_ID, entry_id(IPM)), (0x35E3, entry_id(DELETED))], &[]);
    let d = b.data(store);
    b.node(NID_MESSAGE_STORE, d, 0, 0);
    let keywords = to_utf16le("Keywords");
    let strings = [(keywords.len() as u32).to_le_bytes().to_vec(), keywords].concat();
    // One entry: a string name at offset 0, in PS_PUBLIC_STRINGS (GUID index 2), property index 0.
    let entries = [0u32.to_le_bytes().to_vec(), 5u16.to_le_bytes().to_vec(), 0u16.to_le_bytes().to_vec()].concat();
    let names = pc(
        &[(0x0002, PropValue::Binary(pset::PSETID_COMMON.to_vec())), (0x0003, PropValue::Binary(entries)), (0x0004, PropValue::Binary(strings))],
        &[],
    );
    let d = b.data(names);
    b.node(NID_NAME_TO_ID_MAP, d, 0, 0);

    // Folders: property node, hierarchy table, contents table.
    for (nid, parent, name, kids) in [
        (ROOT, ROOT, "", vec![folder_row(IPM, "Top of Outlook data file", 0, 0, true)]),
        (IPM, ROOT, "Top of Outlook data file", vec![folder_row(INBOX, "Inbox", 2, 1, true), folder_row(DELETED, "Recycle", 0, 0, false)]),
        (INBOX, IPM, "Inbox", vec![folder_row(PROJECTS, "Projects", 1, 0, false)]),
        (DELETED, IPM, "Recycle", vec![]),
    ] {
        let d = b.data(pc(&[(pid::DISPLAY_NAME, s(name))], &[]));
        b.node(nid, d, 0, parent);
        let d = b.data(tc(wide, &folder_cols, &kids));
        b.node((nid & !0x1F) | NID_TYPE_HIERARCHY_TABLE, d, 0, 0);
    }
    let d = b.data(pc(&[(pid::DISPLAY_NAME, s("Projects"))], &[]));
    b.node(PROJECTS, d, 0, INBOX);

    // Inbox contents.
    let cols = [
        (pid::SUBJECT, ptype::STRING),
        (pid::SENT_REPRESENTING_NAME, ptype::STRING),
        (pid::MESSAGE_DELIVERY_TIME, ptype::TIME),
        (pid::MESSAGE_FLAGS, ptype::INT32),
        (pid::IMPORTANCE, ptype::INT32),
    ];
    let rows = vec![
        (
            MSG_HELLO,
            vec![
                Some(s("\u{1}\u{1}Hello from the fixture")),
                Some(s("Ana Costa")),
                Some(PropValue::Time(DELIVERED)),
                Some(PropValue::Int32(0x11)),
                Some(PropValue::Int32(2)),
            ],
        ),
        (MSG_BIG, vec![Some(s("Numbers")), Some(s("Bo Lind")), Some(PropValue::Time(DELIVERED + 10)), Some(PropValue::Int32(0)), None]),
    ];
    let d = b.data(tc(wide, &cols, &rows));
    b.node((INBOX & !0x1F) | NID_TYPE_CONTENTS_TABLE, d, 0, 0);

    // "Hello": a recipient, a file attachment, an attached message, a category.
    let recipients = tc(
        wide,
        &[(pid::RECIPIENT_TYPE, ptype::INT32), (pid::DISPLAY_NAME, ptype::STRING), (pid::EMAIL_ADDRESS, ptype::STRING), (0x67F2, ptype::INT32)],
        &[(1, vec![Some(PropValue::Int32(RECIP_TO)), Some(s("Alex Morgan")), Some(s("alex.morgan@example.org")), Some(PropValue::Int32(1))])],
    );
    let attachments = tc(
        wide,
        &[(pid::ATTACH_SIZE, ptype::INT32), (pid::ATTACH_METHOD, ptype::INT32)],
        &[(0x8025, vec![Some(PropValue::Int32(5)), Some(PropValue::Int32(1))]), (0x8045, vec![Some(PropValue::Int32(0)), Some(PropValue::Int32(5))])],
    );
    let file = pc(
        &[
            (pid::ATTACH_LONG_FILENAME, s("notes.txt")),
            (pid::ATTACH_METHOD, PropValue::Int32(1)),
            (pid::ATTACH_DATA, PropValue::Binary(b"hello".to_vec())),
        ],
        &[],
    );
    let inner = pc(&[(pid::SUBJECT, s("Inner message")), (pid::BODY, s("Attached text"))], &[]);
    let object = [0x8064u32.to_le_bytes(), 0u32.to_le_bytes()].concat();
    let embedded = pc(&[(pid::ATTACH_METHOD, PropValue::Int32(5)), (pid::ATTACH_DATA, PropValue::Other { ty: ptype::OBJECT, raw: object })], &[]);
    let (recipients, attachments, file, inner, embedded) = (b.data(recipients), b.data(attachments), b.data(file), b.data(inner), b.data(embedded));
    let embedded_subs = b.subnodes(&[(0x8064, inner, 0)]);
    let subs = b.subnodes(&[
        (NID_RECIPIENT_TABLE, recipients, 0),
        (NID_ATTACHMENT_TABLE, attachments, 0),
        (0x8025, file, 0),
        (0x8045, embedded, embedded_subs),
    ]);
    let hello = pc(
        &[
            (pid::MESSAGE_CLASS, s("IPM.Note")),
            (pid::SUBJECT, s("\u{1}\u{1}Hello from the fixture")),
            (pid::SENT_REPRESENTING_NAME, s("Ana Costa")),
            (pid::MESSAGE_DELIVERY_TIME, PropValue::Time(DELIVERED)),
            (pid::MESSAGE_FLAGS, PropValue::Int32(0x11)),
            (pid::BODY, s("Short body.")),
            (pid::HTML, PropValue::Binary(b"<p>caf\xE9</p>".to_vec())),
            (pid::INTERNET_CODEPAGE, PropValue::Int32(1252)),
            (0x8000, PropValue::MultiString(vec!["Red category".to_string(), "Travel".to_string()])),
        ],
        &[],
    );
    let d = b.data(hello);
    b.node(MSG_HELLO, d, subs, INBOX);

    // "Numbers": the body lives in a subnode whose data is a tree of three blocks.
    let body = to_utf16le(&big_body());
    let third = (body.len() / 3) & !1;
    let tree = b.xblock(&[&body[..third], &body[third..2 * third], &body[2 * third..]]);
    let subs = b.subnodes(&[(0x1_0041, tree, 0)]);
    let d = b.data(pc(&[(pid::SUBJECT, s("Numbers")), (pid::MESSAGE_FLAGS, PropValue::Int32(0))], &[(pid::BODY, ptype::STRING, 0x1_0041)]));
    b.node(MSG_BIG, d, subs, INBOX);

    // A message in the folder that has no contents table.
    let d = b.data(pc(&[(pid::SUBJECT, s("Loose")), (pid::MESSAGE_FLAGS, PropValue::Int32(1))], &[]));
    b.node(MSG_LOOSE, d, 0, PROJECTS);
    b.finish()
}

fn check(wide: bool, crypt: u8) {
    let mut store = PstStore::from_bytes(sample(wide, crypt)).unwrap();
    assert_eq!(store.display_name(), "Test Mailbox");
    assert!(!store.writable());
    let folders = store.folders().unwrap();
    let names: Vec<&str> = folders.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, ["Inbox", "Recycle", "Projects"]);
    let inbox = &folders[0];
    assert_eq!((inbox.kind, inbox.total, inbox.unread, inbox.has_children, inbox.parent), (FolderKind::Inbox, 2, 1, true, None));
    assert_eq!(folders[1].kind, FolderKind::Deleted, "the store names its Deleted Items folder, whatever it is called");
    assert_eq!(folders[2].parent, Some(inbox.id));

    let list = store.list(inbox.id).unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(
        (list[0].subject.as_str(), list[0].from.as_str(), list[0].time, list[0].read, list[0].has_attachments, list[0].importance),
        ("Hello from the fixture", "Ana Costa", DELIVERED, true, true, 2)
    );
    assert_eq!((list[1].subject.as_str(), list[1].read, list[1].importance), ("Numbers", false, 1));

    let hello = store.load(inbox.id, ItemId(list[0].id)).unwrap();
    assert_eq!(hello.props.str(pid::SUBJECT), Some("Hello from the fixture"));
    assert_eq!(hello.body_text(), "Short body.");
    assert_eq!(hello.body_html().as_deref(), Some("<p>caf\u{e9}</p>"));
    assert_eq!(hello.categories, ["Red category", "Travel"]);
    assert_eq!(hello.recipients.len(), 1);
    assert_eq!(hello.display_recipients(RECIP_TO), "Alex Morgan");
    assert!(hello.recipients[0].get(0x67F2).is_none(), "table bookkeeping columns are not recipient properties");
    assert_eq!(hello.attachments.len(), 2);
    assert_eq!((hello.attachments[0].name().as_str(), hello.attachments[0].data.as_slice()), ("notes.txt", b"hello".as_slice()));
    let inner = hello.attachments[1].embedded.as_ref().unwrap();
    assert_eq!((inner.subject().as_str(), inner.body_text().as_str()), ("Inner message", "Attached text"));

    let big = store.load(inbox.id, ItemId(list[1].id)).unwrap();
    assert_eq!(big.body_text(), big_body());

    // No contents table: the node index still finds the message.
    let loose = store.list(folders[2].id).unwrap();
    assert_eq!(loose.iter().map(|s| s.subject.as_str()).collect::<Vec<_>>(), ["Loose"]);
    assert!(matches!(store.list(FolderId(0xDEAD_BEEF)), Err(StoreError::NoFolder)));
    assert!(matches!(store.load(inbox.id, ItemId(0x7777)), Err(StoreError::NoItem)));
    assert!(matches!(store.add(inbox.id, Item::new_mail()), Err(StoreError::ReadOnly)));
}

#[test]
fn unicode_file_reads() {
    check(true, 0);
}

#[test]
fn encoded_files_read() {
    check(true, 1);
    check(true, 2);
}

#[test]
fn ansi_file_reads() {
    check(false, 0);
    check(false, 2);
}

#[test]
fn not_a_data_file() {
    for bytes in [Vec::new(), b"!BDN".to_vec(), vec![0u8; 4096], b"From: a@example.org\r\n\r\nhi".to_vec()] {
        assert!(matches!(PstStore::from_bytes(bytes), Err(PstError::NotPst)));
    }
    let mut encrypted = sample(true, 0);
    encrypted[513] = 0x10;
    assert!(matches!(PstStore::from_bytes(encrypted), Err(PstError::Unsupported(_))));
}

/// Open, list and load everything; the result does not matter, only that it returns.
fn exercise(bytes: Vec<u8>) {
    let Ok(mut store) = PstStore::from_bytes(bytes) else { return };
    let Ok(folders) = store.folders() else { return };
    for f in folders {
        for row in store.list(f.id).unwrap_or_default() {
            let _ = store.load(f.id, ItemId(row.id));
        }
    }
}

#[test]
fn truncated_files_never_panic() {
    for wide in [true, false] {
        let file = sample(wide, 2);
        for len in (0..file.len()).step_by(37) {
            exercise(file[..len].to_vec());
        }
    }
}

#[test]
fn corrupted_files_never_panic() {
    let mut seed = 0x2545_F491_4F6C_DD1Du64;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    for wide in [true, false] {
        let file = sample(wide, 0);
        for _ in 0..1500 {
            let mut bad = file.clone();
            for _ in 0..1 + next() % 6 {
                let at = (next() % bad.len() as u64) as usize;
                bad[at] = next() as u8;
            }
            exercise(bad);
        }
    }
}

#[test]
fn trees_that_point_at_themselves_end() {
    // A B-tree page that is its own child, and a data tree block that lists itself.
    let mut file = sample(true, 0);
    let bbt_root = u64::from_le_bytes(file[240..248].try_into().unwrap()) as usize;
    let nbt_root = u64::from_le_bytes(file[224..232].try_into().unwrap()) as usize;
    for root in [bbt_root, nbt_root] {
        file[root + 491] = 1;
        for i in 0..20 {
            file[root + i * 24 + 16..root + i * 24 + 24].copy_from_slice(&(root as u64).to_le_bytes());
        }
    }
    exercise(file);

    let mut b = Builder::new(true, 0);
    let mut x = vec![1u8, 2, 2, 0, 0, 0, 0, 0];
    x.extend(6u64.to_le_bytes());
    x.extend(6u64.to_le_bytes());
    b.blocks.push((6, x));
    b.node(NID_ROOT_FOLDER, 6, 6, 0);
    assert!(PstStore::from_bytes(b.finish()).is_ok());
}
