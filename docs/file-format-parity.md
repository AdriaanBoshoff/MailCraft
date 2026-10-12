# File format parity

**Status:** revision 7 · 2026-10-12 · the node database and the property lists and tables of a `.pst` are written (M3.1, M3.2), not yet anything Outlook can open

What each format carries in each direction, and how it was checked. "Outlook" means Outlook Classic
(version 16.0) driven through its automation interface on the dev machine: it opens the file and reports
the item's properties. No window is driven and nothing in a mailbox is changed. Control files Outlook
writes for these checks stay local and are never committed.

## `.msg` and `.oft` ([MS-OXMSG])

| What | OpenMailClient → Outlook | Outlook → OpenMailClient | Outlook → OpenMailClient → Outlook |
|---|---|---|---|
| Message class, subject | read back the same | read | same |
| Sender name, address, address type | read back the same | not checked (control items have no sender) | not checked |
| To and Cc recipients, with type | read back the same | not checked | not checked |
| Received and sent times | read back the same | read | same |
| Read state, importance, follow-up flag | read back the same | read | same |
| Categories (named property `Keywords`) | read back the same, after the fix below | read | same |
| Plain text body | read back the same | read | same |
| HTML body | Outlook reports an HTML item with the same text | not checked (Outlook stored its HTML as RTF) | not checked |
| RTF body (compressed, [MS-OXRTFCP]) | an RTF-only message (stored form, not compressed by us) is shown as rich text with its formatting, once PidTagRtfInSync is set; without it Outlook shows an empty plain-text body | decoded for reading (text, preview, formatted body with its pictures and the places of its attachments; wrapped HTML or text taken back out); the bytes themselves are kept untouched | preserved: Outlook shows the HTML again |
| File attachment: name, size, by-value | read back the same | not checked | not checked |
| Inline picture (attachment with a content id, `cid:` in the HTML) | drawn in the body at the size asked for | not checked | not checked |
| Attached message | not checked in Outlook | not checked | not checked |
| Attached object of another program (attach method 6, a storage: [MS-OXMSG] 2.2.2.2) | written as the storage it was; not checked in Outlook | read whole, every stream and storage with classes and state (our own round trip is pinned by a test) | not checked |
| Properties OpenMailClient does not interpret | n/a | kept | preserved (8 named properties survive) |
| Template (`.oft`) opened as a new item | subject, body, recipients, attachment, categories arrive | read | not checked |

Pinned by `crates/msg/src/tests.rs` (round trip through our own reader, layout, hostile input, and the
named-property lookup checksum).

### Found by this check

- **Named property lookup checksum.** Outlook locates a named property through the lookup stream chosen
  by a checksum of its name ([MS-OXMSG] 2.2.3.2.1). That checksum is the one in [MS-OXRTFCP] 3.1.3.2,
  which starts from zero and is not inverted at the end. OpenMailClient used the common CRC-32, so Outlook
  opened the file but showed no categories. Fixed; `named_property_lookup_uses_the_zero_seeded_checksum`.

### Not yet checked

Messages with an attached message, inline pictures, non-Latin text in every field, very large
attachments, meeting requests, contacts and tasks saved as `.msg`, and opening by double-click in the
Outlook window (only the automation interface was used).

## `.pst` and `.ost` ([MS-PST])

Read: ANSI, Unicode and 4K-page files. See [gaps.md](gaps.md) items 2, 7 and 8.

Written so far (M3.1, M3.2), Unicode with 512-byte pages only:

| Part ([MS-PST]) | Written | Checked by |
|---|---|---|
| Header, both checksums (2.2.2.6) | yes | our reader, our structure check; the checksum against the specification's sample header |
| Allocation maps, page maps, density list (2.2.2.7.2 to 2.2.2.7.4) | yes | our structure check |
| Free map and free page map pages (2.2.2.7.5, 2.2.2.7.6) | yes; their places are worked out, the specification gives none | a 33 MB file of ours; no Outlook-written file |
| Blocks, plain, permuted and cyclic (2.2.2.8) | yes | read back in each encoding |
| Data trees, one and two levels (2.2.2.8.3.2) | yes | read back, 33 MB item |
| Subnode trees with an index block (2.2.2.8.3.3) | yes | read back, 800 subnodes |
| Block and node B-trees (2.2.2.7.7) | yes | read back, 1,000 nodes |
| Heaps over many blocks, with fill levels (2.3.1) | yes; which half of a byte holds the earlier block's level is our reading | read back, more than eight blocks |
| Index in a heap, any depth (2.3.2) | yes | read back, 3,000 records |
| Property list; large values in subnodes (2.3.3) | yes | read back, every value type of the model |
| Table; rows in the heap or in a subnode (2.3.4) | yes | read back, 6,000 rows |
| Folders, messages, the nodes every file must have (2.4, 2.7) | no | M3.3 |

No file from this writer has been opened in Outlook: there is nothing above the node layer to
open yet. `openmailclient-cli inspect file.pst --check` holds a Unicode file's structure against
the format (ANSI and 4K-page files are not checked yet).

## `.eml` (RFC 5322, MIME)

Read and written; checked only through our own reader so far.
