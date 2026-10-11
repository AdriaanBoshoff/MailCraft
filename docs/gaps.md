# Gaps

**Status:** revision 9 · 2026-10-11 · ranked: the top of the list is what hurts most.

## Not built

1. **No accounts, no sending, no receiving.** IMAP, POP3, SMTP and Exchange are absent. `compose.send`
   files the message in Outbox and says so.
2. **`.pst`/`.ost` are read-only.** No writer exists; every change to such a store is refused. Changes
   made in the sample mailbox live in memory and are lost on exit.
3. **Formatted bodies are partly drawn.** HTML bodies show text styles, colours, headings, lists,
   quotations, rules and tables, and pictures the message carries as attachments (`cid:`) or
   writes into itself (`data:`; PNG, JPEG, GIF first frame, BMP) at the size the body asks for, in
   the line of text or at its side (`align`) with what follows beside them until they end or a
   break clears them. Text is drawn in the fonts the message names when they are installed, in
   their own bold and italic faces, with open fonts of the same measures standing in; no font is
   bundled. Borders set in styles are drawn round paragraphs, tables and cells, and cells span
   rows as well as columns. Not yet: other picture kinds (SVG, WebP, TIFF, EMF/WMF), which show a
   labelled placeholder; several pictures at the sides of one paragraph, or a second one while the
   first still stands; animation; a backing for see-through pictures on the dark pane; backgrounds
   on blocks other than cells and rows; borders on text within a line and rounded corners; padding
   that differs left from right; and opening a link (its address is shown on hover). Style sheets
   are read as far as Outlook follows them (rules by element name, first class and id); tables keep
   their width, place, cell fills, padding, spacing and the vertical place of what cells hold; a
   picture that is not loaded leaves a frame of the size it asks for ([ui-parity.md](ui-parity.md),
   Message bodies). RTF bodies
   ([MS-OXRTFCP], and the HTML or text they wrap, [MS-OXRTFEX]) are decoded and drawn with character
   styles, fonts by name, colours, alignment, indents, links and simple tables; not yet: RTF pictures and embedded
   objects (a placeholder), list numbering beyond the text the writer left, nested tables,
   borders per cell edge, merged cells, tab stops, paragraph spacing and Outlook's highlight colours. MailCraft never writes compressed RTF
   (only the uncompressed stored form). Printing does not exist.
4. **Calendar, People, Tasks** modules show a placeholder.
5. **MCP server and control channel** (`mailcraft-cli mcp`, `--control`) are not built.
6. **Web build.** The engine and everything under it compiles for wasm32; there is no web app.

## Built but unproven

7. **`.pst` reader has met few real files.** It is written from [MS-PST] and tested against files
   produced by a test-only builder, including truncated, corrupted and cyclic ones. Checked locally
   against a few small Outlook-written 4K-page `.ost` files (wVer 36): every folder and every item
   loaded. No Outlook-written Unicode or ANSI `.pst`, and no large mailbox, has been tried yet.
8. **No committed fixture for 4K-page files.** The real files used for the check above are private and
   stay local; the test builder only writes 512-byte-page files.
9. **`.msg`/`.oft` output is checked in Outlook Classic only for plain mail.** Outlook opens our
   messages and templates and reads back subject, sender, recipients, times, flags, categories, bodies
   and a file attachment ([file-format-parity.md](file-format-parity.md)). Attached messages, inline
   pictures, non-mail items and opening from the Outlook window are unchecked.
10. **The UI differs from Outlook Classic in measured ways.** A first side-by-side look is in
    [ui-parity.md](ui-parity.md): fixed pane widths, no Favorites, no ribbon overflow or command search.

## Known shortcomings

11. Special folders in a `.pst` are found by English name when the store does not identify them, so a
    localised Inbox may sort as an ordinary folder.
12. Message-list rows from a `.pst` have no preview line.
13. Date groups are Today, Yesterday, This Week, Last Week, This Month, Older; Outlook's are finer.
14. One message selected at a time; no multi-select, no conversations, no drag and drop.
15. Keyboard shortcuts cover the common commands only; there is no command search.
16. Password-protected and encrypted data files are refused or opened without the password check.
17. No localisation; times use a 12-hour clock and month/day/year order.
18. A long formatted body is laid out in full on every repaint; nothing skips the part off screen.

## Revision history

- 9 · 2026-10-11 — italic faces, text beside a picture across blocks, borders set in styles and cells that span rows done (item 3).
- 8 · 2026-10-11 — fonts by name, pictures in the line of text and at its side, and pictures written into the body done (item 3).
- 7 · 2026-10-11 — style sheets, cell padding and spacing, row fills, vertical places, percentage widths and picture frames done (item 3).
- 6 · 2026-10-11 — bodies compared with Outlook's; table widths, placement and cell fills done (item 3).
- 5 · 2026-10-11 — pictures attached to the message are drawn (item 3).
- 4 · 2026-10-11 — RTF bodies are decoded and drawn (item 3).
- 3 · 2026-10-11 — HTML bodies are drawn formatted (item 3 rewritten); item 18 added.
- 2 · 2026-10-11 — `.msg` checked in Outlook Classic; first UI comparison.
- 1 · 2026-10-11 — first list.
