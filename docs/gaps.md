# Gaps

**Status:** revision 37 · 2026-10-12 · ranked: the top of the list is what hurts most.

## Not built

1. **No accounts, no sending, no receiving.** IMAP, POP3, SMTP and Exchange are absent. `compose.send`
   files the message in Outbox and says so.
2. **`.pst`/`.ost` are read-only.** No writer exists; every change to such a store is refused. Changes
   made in the sample mailbox live in memory and are lost on exit.
3. **Formatted bodies are partly drawn.** Every "not yet" below has a slot in
   [ROADMAP.md](../ROADMAP.md#do-later-from-m2). HTML bodies show text styles, colours, headings, lists,
   quotations, rules and tables, and pictures the message carries as attachments (`cid:`) or
   writes into itself (`data:`; PNG, JPEG, GIF first frame, BMP) at the size the body asks for, in
   the line of text or at its side (`align`) with what follows beside them until they end or a
   break clears them. Text is drawn in the fonts the message names when they are installed, in
   their own bold and italic faces, with open fonts of the same measures standing in; no font is
   bundled. Borders set in styles are drawn round paragraphs, tables and cells, and cells span
   rows as well as columns. Not yet: other picture kinds (SVG, WebP, TIFF, EMF/WMF), which show a
   labelled placeholder; several pictures at the sides of one paragraph, or a second one while the
   first still stands; animation; a backing for see-through pictures on the dark pane; boxes
   inside one another, a fill behind a table in a block and behind the whole body; borders on
   text within a line and rounded corners. Style sheets
   are read as far as Outlook follows them (rules by element name, first class and id); tables keep
   their width, place, cell fills, padding, spacing and the vertical place of what cells hold; a
   picture that is not loaded leaves a frame of the size it asks for ([ui-parity.md](ui-parity.md),
   Message bodies). RTF bodies
   ([MS-OXRTFCP], and the HTML or text they wrap, [MS-OXRTFEX]) are decoded and drawn with character
   styles, fonts by name, colours, alignment, indents, links, pictures written into the body as hex digits or as bytes (PNG,
   JPEG, bitmaps, metafiles that hold only one bitmap, and metafiles of either kind that are drawings of
   lines, curves, filled shapes and text in an installed font, [MS-WMF] 2.3.3 and [MS-EMF] 2.3.5), attachments in their places in the text
   (`\objattph`, [MS-OXRTFEX] 2.2.3.4: an attached object by the best picture it has, [MS-OLEDS]
   2.3.4 and 2.3.6, any other as a card with its name and size that a click saves),
   paragraph borders and the room paragraphs ask for above and below,
   and tables with each cell's own lines, fill, place and merge down and across, and tables inside their cells; not yet in a metafile: a part of a bitmap, one mirrored, turned, packed in runs or not copied as it is, a patterned brush, or text that needs shaping
   (a placeholder), list numbers an override changes (`\lfolevel`), and Outlook's highlight colours. OpenMailClient never writes compressed RTF
   (only the uncompressed stored form). Printing (`file.print`) writes a PDF file in Memo Style, with
   text, lists, borders and tables; pictures the message carries are drawn, in the line or at the side with text beside them; pairs of glyphs are kerned; not yet: pictures on a server (never fetched; their place is named),
   right-to-left text outside the body and the rest of what a right-to-left paragraph moves to the right (see ROADMAP, Do later), the view's own columns and group headings in Table Style (`"style": "table"` prints the list with fixed columns), other print styles, keeping the page setup between runs, sending pages to a printer,
   a zoom in the print preview (`file.printPreview`, and the print window), and any comparison with what Outlook prints.
4. **Calendar, People, Tasks** modules show a placeholder.
5. **MCP server and control channel** (`openmailclient-cli mcp`, `--control`) are not built.
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

- 37 · 2026-10-12 — shaping from the font's own tables in print (item 1).
- 36 · 2026-10-12 — right-to-left text and joined Arabic letters (item 1).
- 35 · 2026-10-12 — what item 3 leaves is scheduled in ROADMAP.md, "Do later (from M2)".
- 34 · 2026-10-12 — M2 closed; item 3 is what it leaves.
- 33 · 2026-10-12 — bitmaps beside a metafile's drawing (item 1).
- 32 · 2026-10-12 — Table Style (item 3).
- 31 · 2026-10-12 — print preview (item 3).
- 30 · 2026-10-12 — page setup for print (item 3).
- 29 · 2026-10-12 — text beside side pictures and kerning in print (item 3).
- 28 · 2026-10-12 — opening a link after a question (item 3).
- 27 · 2026-10-12 — fills behind blocks and padding on each side (item 3).
- 26 · 2026-10-12 — RTF list numbers from the list tables (item 3).
- 25 · 2026-10-12 — RTF tab leaders and the line between paragraphs in one border (item 3).
- 24 · 2026-10-12 — Line spacing in RTF and HTML bodies (item 3).
- 23 · 2026-10-12 — RTF tab stops: left, centre, right and decimal (item 3).
- 22 · 2026-10-12 — RTF list paragraphs are list items with the writer's number or bullet (item 3).
- 21 · 2026-10-12 — paragraph borders and paragraph spacing are read (item 3).
- 20 · 2026-10-12 — nested tables and cells merged across are read (item 3).
- 19 · 2026-10-12 — pictures written as bytes (`\bin`) are read (item 3).
- 18 · 2026-10-12 — an attachment in the text is a card with its name, size and caution (item 3).
- 17 · 2026-10-12 — text in metafiles is drawn, and is the picture's own text (item 3).
- 16 · 2026-10-11 — enhanced metafiles that are drawings are drawn (item 3).
- 15 · 2026-10-11 — Windows metafiles that are drawings are drawn (item 3).
- 14 · 2026-10-11 — attached objects of other programs are kept through `.msg` and shown by their own pictures (item 3).
- 13 · 2026-10-11 — RTF bodies show attachments in their places, bitmaps and metafile bitmaps (item 3).
- 12 · 2026-10-11 — pictures are printed; what remains for them listed (item 3).
- 11 · 2026-10-11 — a message prints to a PDF file; what printing lacks listed (item 3).
- 10 · 2026-10-11 — RTF pictures (PNG, JPEG) and RTF cell lines, fills, places and merges done (item 3).
- 9 · 2026-10-11 — italic faces, text beside a picture across blocks, borders set in styles and cells that span rows done (item 3).
- 8 · 2026-10-11 — fonts by name, pictures in the line of text and at its side, and pictures written into the body done (item 3).
- 7 · 2026-10-11 — style sheets, cell padding and spacing, row fills, vertical places, percentage widths and picture frames done (item 3).
- 6 · 2026-10-11 — bodies compared with Outlook's; table widths, placement and cell fills done (item 3).
- 5 · 2026-10-11 — pictures attached to the message are drawn (item 3).
- 4 · 2026-10-11 — RTF bodies are decoded and drawn (item 3).
- 3 · 2026-10-11 — HTML bodies are drawn formatted (item 3 rewritten); item 18 added.
- 2 · 2026-10-11 — `.msg` checked in Outlook Classic; first UI comparison.
- 1 · 2026-10-11 — first list.
