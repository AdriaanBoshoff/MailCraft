# File format parity

**Status:** revision 1 · 2026-10-11

What each format carries in each direction, and how it was checked. "Outlook" means Outlook Classic
(version 16.0) driven through its automation interface on the dev machine: it opens the file and reports
the item's properties. No window is driven and nothing in a mailbox is changed. Control files Outlook
writes for these checks stay local and are never committed.

## `.msg` and `.oft` ([MS-OXMSG])

| What | MailCraft → Outlook | Outlook → MailCraft | Outlook → MailCraft → Outlook |
|---|---|---|---|
| Message class, subject | read back the same | read | same |
| Sender name, address, address type | read back the same | not checked (control items have no sender) | not checked |
| To and Cc recipients, with type | read back the same | not checked | not checked |
| Received and sent times | read back the same | read | same |
| Read state, importance, follow-up flag | read back the same | read | same |
| Categories (named property `Keywords`) | read back the same, after the fix below | read | same |
| Plain text body | read back the same | read | same |
| HTML body | Outlook reports an HTML item with the same text | not checked (Outlook stored its HTML as RTF) | not checked |
| RTF body (compressed, [MS-OXRTFCP]) | not written by us | kept as bytes, **not decoded** | preserved: Outlook shows the HTML again |
| File attachment: name, size, by-value | read back the same | not checked | not checked |
| Attached message | not checked in Outlook | not checked | not checked |
| Properties MailCraft does not interpret | n/a | kept | preserved (8 named properties survive) |
| Template (`.oft`) opened as a new item | subject, body, recipients, attachment, categories arrive | read | not checked |

Pinned by `crates/msg/src/tests.rs` (round trip through our own reader, layout, hostile input, and the
named-property lookup checksum).

### Found by this check

- **Named property lookup checksum.** Outlook locates a named property through the lookup stream chosen
  by a checksum of its name ([MS-OXMSG] 2.2.3.2.1). That checksum is the one in [MS-OXRTFCP] 3.1.3.2,
  which starts from zero and is not inverted at the end. MailCraft used the common CRC-32, so Outlook
  opened the file but showed no categories. Fixed; `named_property_lookup_uses_the_zero_seeded_checksum`.

### Not yet checked

Messages with an attached message, inline pictures, non-Latin text in every field, very large
attachments, meeting requests, contacts and tasks saved as `.msg`, and opening by double-click in the
Outlook window (only the automation interface was used).

## `.pst` and `.ost` ([MS-PST])

Read only. See [gaps.md](gaps.md) items 2, 7 and 8.

## `.eml` (RFC 5322, MIME)

Read and written; checked only through our own reader so far.
