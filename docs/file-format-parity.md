# File format parity

**Status:** revision 14 · 2026-10-13 · the structure check looks inside nodes (M3b.13); an attachment keeps what is stored on it when a message's attachments change, and search-folder rows of changed messages are brought up to date (M3b.11); the journal is kept in a folder of our own when the data file's folder cannot be written (M3b.7); `file.export` copies a Unicode `.pst` as it is stored (M3b.2); unsaved changes are journalled beside the file (M3b.1); nothing our writer made has been opened in Outlook yet

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
| Heaps over many blocks, with fill levels (2.3.1) | yes; which half of a byte holds the earlier block's level is our reading | read back, more than eight blocks; our structure check (page maps; not the fill levels) |
| Index in a heap, any depth (2.3.2) | yes | read back, 3,000 records; our structure check |
| Property list; large values in subnodes (2.3.3) | yes; a value typed as an object that names no subnode is left out | read back, every value type of the model; our structure check |
| Table; rows in the heap or in a subnode (2.3.4) | yes | read back, 6,000 rows; our structure check |
| The nodes every file must have: message store, name-to-id map, root, search folders, table templates (2.7.1) | yes | read back; structure check clean |
| Folders with their hierarchy, contents and associated-contents tables (2.4.4) | name, class, counts; the associated table is written empty | read back, 1,500 messages in a folder |
| Messages with recipients and attachments (2.4.5, 2.4.6) | yes; properties we do not understand go out as they were read | read back, sample mailbox and three local files (counts only) |
| A message attached to a message (2.4.6.2.3) | yes, to 32 deep; where it is kept is our reading | read back |
| Named properties (2.4.7) | yes; the bucket of a name is our reading | read back, 600 names |
| Hidden (associated) items; folder properties beyond name, class and counts | kept by `file.save` and by `file.export` of a Unicode `.pst` (copied as stored); not carried by `file.export` from other sources | a hidden message and a folder property our writer never makes survive a save and a copy |
| Which folder is the Calendar, Contacts, Tasks (ids on the root and the Inbox) | kept by `file.save` and by `file.export` of a Unicode `.pst`; not written by `file.export` from other sources | M3b |
| Changing a Unicode `.pst` that is open (`file.save`) | yes: nodes not touched are copied block for block; a changed message keeps every stored property that was not changed, also those under ids no name stands for; when its attachments change, each attachment the file holds stays in its subnode with every stored property (matched by being the same, or by its contents), and only new ones are written from the model; a search folder's table loses the rows of deleted messages and shows changed ones as they are now | read back: messages changed, moved, deleted and added, folders made, renamed, moved and deleted; attachments reordered, renamed, added and removed with a property no name stands for on each; a search table with rows written as another program would; damaged files; three local copies (counts only) |
| Changing an ANSI `.pst`, a 4K-page file, an `.ost` | no: read-only | M3b (an `.ost` never) |

No file from this writer has been opened in Outlook: that adds a data file to the owner's
profile and waits for their say. `file.export` writes the new file beside the target, holds it
against the structure check and only then puts it in the target's place. `file.save` does the
same for an open file, reads the new file back and compares it with what was shown, refuses
when another program wrote the file since it was opened or has it open, and on the first save
keeps the file as it was beside it (`name.before-openmailclient.pst`). Until a save, each change is journalled in a file of our own beside the data file (`name.pst.openmailclient-journal`: our format, not Outlook's, and never read by it) and played back when the file is opened again; when the folder cannot be written the journal is kept in a folder of our own beside the settings instead (M3b.7); the data file is written only by a save. `openmailclient-cli inspect file.pst --check` holds a Unicode file's structure against
the format (ANSI and 4K-page files are not checked yet).

## `.eml` (RFC 5322, MIME)

Read and written; checked only through our own reader so far.
