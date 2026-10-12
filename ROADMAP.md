# OpenMailClient roadmap

**Status:** M2 (formatted reading) closed; M1's checks against Outlook still open; M3 (writing `.pst`) in progress: a mailbox is saved as a new `.pst` · revision 50 · 2026-10-12

## Headline numbers

| Measure | Now |
|---|---|
| Catalogued Outlook Classic features with a command | 42 of 116 (36%), see [docs/parity-checklist.md](docs/parity-checklist.md) |
| Tests | 237, all passing |
| File formats read | `.pst`/`.ost` (ANSI, Unicode, 4K pages), `.msg`, `.oft`, `.eml` |
| File formats written | `.msg`, `.oft`, `.eml`; a message prints to `.pdf` |
| Protocols | none |

## Milestones

| | Milestone | State |
|---|---|---|
| M1 | **Data files first**: open `.pst`/`.ost`/`.msg`/`.eml` in an Outlook-Classic-style window; save messages Outlook can reopen | in progress |
| M2 | Formatted reading: HTML and RTF bodies laid out and drawn, inline pictures, printing | closed: HTML and RTF text, lists, tables, pictures and named fonts are drawn; a message or a folder's list prints to a PDF file, with a page setup and a preview. See "Do later (from M2)" |
| M3 | Writing `.pst`: journalled, crash-safe writes; rules and folder changes persist | in progress: a mailbox is saved as a new `.pst` (M3.1 to M3.3); changing an open one is M3.4 |
| M3b | The do-later batch from M2 (see "Do later (from M2)") | not started |
| M4 | Accounts: IMAP, POP3 and SMTP behind an account trait, with an in-process fake server for tests | not started |
| M5 | Calendar, People and Tasks modules | not started |
| M6 | MCP server, control channel, web build | not started |

## Current focus (M1)

1. Check the `.pst` reader against more real Outlook-written files, locally (results are never
   published). Done so far: a few small 4K-page `.ost` files.
2. Have Outlook Classic open OpenMailClient-written `.msg` files and compare, property by property. Done
   for plain mail with a file attachment ([docs/file-format-parity.md](docs/file-format-parity.md));
   attached messages and inline pictures remain.
3. Side-by-side layout comparison with Outlook Classic and the interaction parity doc.

## Current focus (M2)

1. Done: HTML bodies read into a formatted document (`crates/formats`) and drawn in the reading pane,
   with nothing run and nothing fetched.
2. Done: bodies stored as compressed RTF ([MS-OXRTFCP]) are decoded; RTF that wraps HTML or text
   ([MS-OXRTFEX]) is read as what it wraps, ordinary RTF is drawn formatted.
3. Done: pictures the message carries as attachments are decoded in-process and drawn in the body.
4. Done: an HTML and an RTF message of ours compared with Outlook's display of the same `.msg`;
   text size, table widths and placement, cell fills, indents and quotations follow it
   ([docs/ui-parity.md](docs/ui-parity.md), Message bodies).
5. Done: frames for pictures that are not loaded, row backgrounds, cell padding and spacing,
   vertical places in cells, percentage widths, and style sheets as far as Outlook follows them.
6. Done: pictures stand in the line of text or at its side with text beside them, text is drawn
   in the fonts the message names (installed ones only; nothing is bundled), and pictures written
   into the body (`data:`) are shown.
7. Done: italic faces of named fonts, what follows a picture at the side standing beside it,
   borders set in styles, cells that span rows.
8. Done: PNG and JPEG pictures written into RTF bodies, and RTF cells with their own lines,
   fills, places and merges down.
9. Done: a message prints to a PDF file in Memo Style (`file.print`), text, tables and borders;
   pictures are not printed yet.
10. Done: pictures in print - attached and written-in pictures are drawn on the page.
11. Done: attachments stand in their places in an RTF body; bitmaps, and metafiles that hold
    only a bitmap, are shown.
12. Done: an attached object of another program is kept whole through `.msg`, and is shown by
    the best picture it has.
13. Done: a pass over what is already built, to make each feature better than the one it
    follows (M2.13 to M2.21).
14. Done: Windows metafiles and enhanced metafiles that are drawings of lines, curves,
    filled shapes and text are drawn.
15. Done: an attachment that stands in the text is a card there, with its name and size.
16. Done: pictures written as bytes (`\bin`) are read.
17. Done: tables inside the cells of tables, and cells merged across, in RTF bodies.
18. Done: paragraph borders and the room paragraphs ask for above and below, in RTF bodies.
19. Done: list paragraphs of RTF bodies have their numbers and bullets as markers.
20. Done: tab stops in RTF bodies (left, centre, right, decimal).
21. Done: line spacing in RTF and HTML bodies.
22. Done: tab leaders, and the line between paragraphs in one border, in RTF bodies.
23. Done: list numbers from the list tables of RTF bodies.
24. Done: fills behind blocks, and room on each side of them, in HTML and RTF bodies.
25. Done: opening a link, after being asked.
26. Done: text beside pictures, and kerning, in print.
27. Done: page setup for print.
28. Done: a print preview with the page setup in it.
29. Done: Table Style: a folder's list printed as a table.
30. Done: bitmaps beside a metafile's drawing.
31. Done: M2 is closed; what it leaves is scheduled under "Do later (from M2)".
32. Done: right-to-left text is drawn and printed in the order it is read, and Arabic letters are joined (M2.41).
33. Done: print shapes text from each font's own tables, and a PDF says what its glyphs stand for (M2.42).
34. Done: the node database of a `.pst` is written, and a data file's structure can be checked (M3.1).
35. Done: property lists and tables of a `.pst` are written (M3.2).
36. Done: a mailbox is saved as a new `.pst`: folders, messages, attachments, the nodes every data file must have (M3.3).
37. Next: M3.4 (changes to an open `.pst` kept: rewrite to a temporary file and swap, unchanged items copied as they are).

## Do later (from M2)

M2 closed with these not done. They are not dropped: each has a slot, and is struck from this
table when it lands. The slots are a plan, and an item moves up when it starts to hurt.

| Do later | When |
|---|---|
| ~~Right-to-left text (Hebrew, Arabic) in the right order, and Arabic letters joined, on screen and in print~~ | done: M2.41 |
| ~~Shaping from the font's own tables in print (ligatures, marks placed, Indic and other complex scripts; Arabic joined by the font's tables and across a change of look; copying text out of a PDF in the order it is read)~~ | done: M2.42 |
| Shaping for the text print sets itself: the name and page number at the head and foot of a page, list markers, the header's field names and values | M3b |
| Copying text out of our PDFs tried in PDF readers (the file says what its glyphs stand for; no reader was tried) | M3b |
| Right-to-left text in the message list, the header of the reading pane, folder names and the compose box (drawn there as stored, so Hebrew and Arabic read backwards) | M3b |
| For a right-to-left paragraph: list markers, indents, quotation bars and tab leaders at the right; text beside a picture at the side; columns of a right-to-left table from the right; `<bdo>` and `unicode-bidi` | M3b |
| Selecting and copying text in a paragraph with right-to-left writing; Arabic joined across a change of look on screen; other complex scripts checked on screen (egui shapes each piece from the font's tables; only Hebrew and Arabic were looked at) | with M5, when the item windows get their own text drawing |
| Keeping the page setup between runs | M3, with the first settings that are saved |
| A `.pst` from our writer opened in Outlook Classic (needs the owner's say: it adds a data file to the profile); with it, where a message attached to a message is kept and the bucket of a named property, both written from the specification alone | when the owner says so |
| Hidden (associated) items and a folder's own properties beyond name, class and counts carried into a saved `.pst` | M3.4, with the copy of unchanged items |
| The ids on the root and the Inbox that tell Outlook which folder is the Calendar, Contacts, Tasks, Notes, Journal and Drafts | M3.4 |
| Export off the UI thread, with progress and a way to stop | M3b |
| Table cells whose type differs from the column's kept in a saved `.pst`'s tables (the item keeps them; the list does not show them) | M3b |
| The structure check for ANSI and 4K-page (`.ost`) files | M3b |
| The structure check reaching into heaps, property lists and tables (it stops at blocks and nodes) | M3b |
| 32-bit floats, currency and error values keeping their type when an item is written from the model | M3.4, with the copy of unchanged items |
| A heap's fill levels compared with an Outlook-written heap of several blocks | with the first such file we may read |
| The places of free map and free page map pages compared with an Outlook-written file over 32 MB | with the first such file we may read |
| Writing ANSI and 4K-page data files (a re-saved file keeps its kind) | M3b |
| The selected rows only, the view's own columns and group headings in Table Style | M3b, the batch after M3 |
| A zoom in the print preview | M3b |
| SVG, WebP and TIFF pictures; GIF animation | M3b |
| A backing for see-through pictures on the dark pane; several pictures at the sides of one paragraph | M3b |
| Boxes inside one another; a fill behind a table in a block and behind the whole body; borders on text within a line; rounded corners | M3b |
| Outlook's highlight colours; list numbers an override changes (`\lfolevel`) | M3b |
| Metafiles: a part of a bitmap, one mirrored, turned or packed in runs, copies that are not plain, patterned brushes, regions of many boxes | M3b |
| Pictures on a server, loaded on an explicit command (never by themselves) | M4, with accounts |
| Sending pages to a printer; print styles other than Memo and Table | M5, with the calendar's print styles |
| Comparing what is printed with what Outlook prints | when its print pane can be driven without touching the shared machine's printer; until then nothing is claimed |

The ranked work list is [docs/gaps.md](docs/gaps.md).

## Progress log

- **2026-10-11** — M1.1: item model, MIME, `.msg` and `.pst` readers, engine with 48 commands, CLI.
  M1.2: the egui window (ribbon, folder pane, message list, reading pane, inline compose, File page),
  headless screenshots, the desktop app, and `cargo xtask ci`.
- **2026-10-11** — M1.3: `.ost` files are accepted (client magic "SO"); 4K-page reading confirmed
  against real offline stores, locally.
- **2026-10-11** — M1.4: Outlook Classic opens OpenMailClient-written `.msg` and `.oft` files and reads the
  same properties back; a file Outlook wrote survives a trip through OpenMailClient. Fixed the named-property
  lookup checksum, which had made Outlook drop categories. Added the `msg_dump` structure tool.
- **2026-10-11** - M1.5/M1.6: first side-by-side with Outlook Classic ([docs/ui-parity.md](docs/ui-parity.md)).
  Added the simplified single-line ribbon with the full-height module strip, and a dark colour set;
  `mail.select` takes a folder.
- **2026-10-11** - M2.1: HTML bodies are drawn formatted: text styles, colours, headings, lists,
  quotations and tables, in both colour sets. Scripts, style sheets, frames and forms are dropped;
  remote pictures and tracking pixels are counted and never fetched; hidden text stays hidden. New
  `formats` crate, and `ui.bodyFormat` to read as plain text instead.
- **2026-10-11** - M1.7: the classic ribbon was compared with Outlook's and rebuilt: both ribbon
  layouts now share one shell (title bar, full-height module strip, tab row) and the classic ribbon
  is a rounded panel 98 high. `mail.select` with only a folder goes to that folder.
- **2026-10-11** - M2.2: RTF bodies are read. PidTagRtfCompressed is decompressed (checked against
  the worked example in [MS-OXRTFCP]); a message kept only as RTF now has text, a list preview and a
  formatted body, and RTF that wraps HTML gives that HTML back (so it also survives conversion to
  `.eml`). The sample mailbox has an RTF-only receipt under Inbox/Receipts.
- **2026-10-11** - M2.3: pictures attached to the message (`cid:`) are drawn in the body. Only
  attachments the body points at are decoded, only as PNG, JPEG, GIF or BMP, with file size,
  dimensions and memory capped first; anything else stays a placeholder and nothing is fetched or
  handed to another program. The sample order mail carries a picture drawn in code.
- **2026-10-11** - M2.4: bodies follow Outlook's layout. Two sample messages were written as `.msg`,
  opened in Outlook Classic and compared with the reading pane. Unstyled HTML text is 12 pt; tables
  keep the width they ask for, stand left, centred or right, and fill their cells; a plain
  blockquote is set in and only quoted mail gets a bar; RTF cells take their widths from the row
  definition. An RTF-only message is now marked in sync, which Outlook needs to show it at all.
- **2026-10-11** - M2.5: style sheets and table details, checked against Outlook with three probe
  messages of our own. `<style>` rules by element name, first class and id are followed (and, as in
  Outlook, nothing else: no second class, no nested selectors, no `@media`). Tables have cell
  padding and spacing (1 and 2 unless set), collapsed borders, row backgrounds, percentage widths,
  and cells whose content stands in the middle unless told otherwise. A picture that is not loaded
  leaves an empty frame of the size it asks for. A link keeps the link colour whatever surrounds it.
- **2026-10-11** - M2.6: fonts and pictures in the text, checked against Outlook with a fourth probe
  message of our own. A new crate, `crates/fonts` (L1), turns the list of fonts a message names
  into installed faces: names are only ever compared with the names of installed fonts, open fonts
  of the same measures stand in for proprietary ones that are missing, an unknown font falls back
  to a serif as Outlook's does, and a font file is checked before the drawing code sees it. No font
  is bundled or fetched. Pictures now stand in the line of text on its baseline and wrap like
  words; `<img align=left|right>` puts one at the side with the text beside and then under it
  (a `float` style does not, as in Outlook). Pictures written into the body as `data:` addresses
  are decoded under the same caps as attached ones. RTF text takes its fonts from the font table.
- **2026-10-11** - M2.7: borders, cells over several rows and text beside pictures, checked against
  Outlook with a fifth probe message of our own. Italic and bold-italic text in a named font is
  drawn in the font's own italic face when it is installed (slanted only when it is not). The
  paragraphs, tables and lists after a picture at the side stand beside it until it ends, and
  `<br clear>` or a `clear` style starts below it. A `border` style is drawn round its paragraphs
  (once round all the paragraphs of one block), its table or its cell, per side, solid, dashed,
  dotted or double, with its padding; `bordercolor` colours a table's lines and a `border` wider
  than 1 draws a thick line round the table. `rowspan` is followed: cells under a cell that
  reaches down move aside (HTML 4.01 11.2.6.1) and rows grow to hold a tall one. Border widths,
  spans and padding are capped where they are read.
- **2026-10-11** - M2.8: RTF pictures and RTF tables, checked against Outlook with a sixth probe
  message of our own (RTF 1.9.1, "Pictures" and "Table Definitions"). A `\pict` that holds PNG or
  JPEG data is kept as its bytes and shown in the line of text, at the size its goal and scale ask
  for; metafiles and bitmaps leave a place of that size. A picture's hex digits and skipped
  destinations no longer count towards the cap on text, which used to cut a message short after a
  large picture. Cells draw only the lines the row names, per side, with their width, colour and
  kind (solid, double, dashed, dotted); lines between cells that touch are drawn once. Cell fills
  (`\clcbpat`), places (`\clvertal*`), side room (`\trgaph`) and cells merged down (`\clvmgf`,
  `\clvmrg`) are followed, and an RTF table stands right under the paragraph before it. A new
  example, `msg_with_rtf`, gives a `.msg` of ours an RTF body for such comparisons.
- **2026-10-11** - M2.9: printing. `file.print` (Ctrl+P, File > Print) lays the open message out in
  Memo Style - the owner's name over a line, the fields, the body, a number at the foot of each
  page - and writes a PDF file, on A4 or Letter. Two new crates: `layout` flows the document the
  reading pane draws down pages (paragraphs, lists, quotes, borders, tables with spans whose rows
  go on over a page end) and `pdf` writes the file (PDF 1.7, ISO 32000-1: compressed streams,
  Type 0 fonts with only the glyphs used, text that can be searched and copied). Fonts are the
  installed ones; a font whose licence bits forbid embedding (OpenType `OS/2` fsType) is passed
  over. Not yet: pictures (their place is named), kerning, shaping and right-to-left text, a
  printer other than a PDF file, and a comparison with what Outlook prints. Tests use a font
  built in code, so nothing is bundled.
- **2026-10-11** - M2.10: pictures in print. A picture a message carries (an attachment the body
  names by content id, or one written into the body) is drawn on the printed page at the size the
  body asks for, made smaller to fit the column and the page; what shows through it is kept (a
  soft mask in the PDF file). Each picture goes into the file once. Decoding moved from the
  window into the engine (`engine::pictures`), so the reading pane and print share one set of
  limits. A picture on a server is never fetched and stays named in words. Not yet: a picture at
  the side with text beside it (it is printed in the line), and a picture cut over a page end.
- **2026-10-11** - M2.11: attachments in RTF bodies, bitmaps and metafile bitmaps. Each place an
  RTF body keeps for an attachment (`\objattph`) is filled as [MS-OXRTFEX] 2.2.3.4 says: the
  attachments that are not hidden and have a rendering position, in the order of that position;
  when their number and the number of places differ, the places are given up. An attached object
  is shown as its own picture ([MS-OXCMSG] 2.2.2.17) when that can be read; any other attachment
  is named where it stands. A picture written into RTF as a bitmap (`\dibitmap`), or as a metafile
  (`\wmetafile`, `\emfblip`) that draws one bitmap and nothing else, is now shown and printed
  ([MS-WMF], [MS-EMF]); a metafile that is a drawing still leaves a place. The sample receipt has
  an attachment in its text. Not yet: metafiles that draw lines, shapes and text, the picture
  inside an attached object's own storage, an icon beside an attachment's name, `\bin` data.
- **2026-10-11** - M2.12: attached objects of other programs. An attachment with attach method 6
  ([MS-OXCMSG] 2.2.2.9) is a storage the owning program lays out ([MS-OXMSG] 2.2.2.2). Reading a
  `.msg` used to take that storage for an attached message and writing dropped it; now it is kept
  whole, as a compound file of its own (every stream and storage, with classes and state), and
  written back as the storage it was, its entry marked as an object (2.4.2.2). Nothing in an
  object is run or handed to another program. In an RTF body the object is shown by the best
  picture that can be read: the pictures it keeps of itself ([MS-OLEDS] 2.3.4), before the small
  one the message keeps (2.2.2.17). Beyond Outlook: when the object's own data is a picture file
  (2.3.6) that picture is shown, at its full quality, in place of the copy kept for showing; and an
  object that cannot be shown is named with what it calls itself (2.3.8) and not left as a blank
  frame. The sample receipt carries such an object, a permit card drawn in code.
- **2026-10-11** - M2.13: search, the first of the pass over what is already built. The search
  box matched one run of letters; it now understands words (all of them), quoted words, `-word`
  and `NOT`, `OR`, the fields `from:` `to:` `subject:` `category:`, and what a message is:
  `has:attachment`, `is:unread`, `is:flagged`, `is:important`, `before:` `after:` `on:` with a day,
  a month or a year, and `size:>2mb`; Outlook's own keywords (`hasattachments:yes`, `read:no`,
  `isflagged:yes`, `received:`) mean the same. Beyond Outlook: letters match without their accents
  ("zoe" finds "Zöe"), categories are searched with the rest, nothing needs an index, a keyword
  that is not understood is looked for as the text it is (so a query never fails and never quietly
  finds everything), and the command answers with the count. The owner asked (2026-10-11) that
  every feature, those already built too, be made better than the one it follows where it can be;
  the pass goes on with links, saving attachments and print.
- **2026-10-11** - M2.14: links that are not what they say, the second of the pass. Outlook shows
  a link's address when the pointer rests on it and leaves the comparing to the reader. OpenMailClient
  compares: a link whose text names one place and goes to another, one with a name before an `@`
  in its address, one that goes to a number, one written in letters that can pass for others, and
  one that is no web address at all (`javascript:`, `file:`) is underlined in the warning colour
  before anyone points at it, and says what is wrong in a sentence when they do ([RFC 3986] 3.2).
  A name and its own sub-sites count as one place, and a file's name is not taken for a site.
  Nothing is opened, fetched or resolved to tell. The sample's Junk Email folder has a message
  with each kind.
- **2026-10-11** - M2.15: saving attachments, the third of the pass. Outlook saves an attachment
  under the name the message gave it and asks before replacing a file. OpenMailClient has Save All
  Attachments (`file.saveAllAttachments`, a folder; from the File menu and beside the
  attachments) and goes further: every name is made safe first (one name and never a path, no
  device names, no hidden or direction-reversing characters, runs of spaces closed up, a length
  every file system takes, the ending kept), no file is ever replaced (a taken name gets a
  number), and an attached message is saved as `.msg`. An attachment a reader should think twice
  about is marked before it is pointed at and says why in a sentence: a name that reads as one
  kind of file and is another (`invoice.pdf.exe`), a program or script, a document that carries
  macros, a page that opens in a browser, a name with reversing characters, and content that is
  a program under another name. A long name is cut in its middle, so its ending always shows.
  Nothing is opened or run to tell.
- **2026-10-11** - M2.16: print, the fourth of the pass. On paper no pointer can rest on a link,
  so a printed message from Outlook does not say where its links go. OpenMailClient's does: a link
  that is not what its text says gets a number where it stands and a line after the body with
  where it goes and what is wrong with it; `"links": "all"` lists every link whose text is not
  its own address, and `"links": "none"` prints the body alone. Each page's foot says which page
  of how many it is, so a loose page tells whether the rest is with it, and the header lists
  each attachment under the name it would be saved as, with its size.
- **2026-10-11** - M2.17: arranging the list, the fifth of the pass. The list now stands in
  groups however it is arranged, as Outlook's does (by sender, recipient, subject, size,
  importance, flag and attachments, not only by date), and goes further: names and subjects are
  ordered without their case and accents (`Élodie` beside `Elodie`, not after `Zoe`), numbers in
  them by their value (`Invoice 9` before `Invoice 10`), each group's heading says how many
  messages it holds and how many of them are unread, and mail older than two weeks is grouped by
  its month and then its year, not under one "Older". Messages that are equal by the
  arrangement stand newest first whichever way it runs (they stood oldest first in an ascending
  one). The groups are part of `session.inspect`.
- **2026-10-11** - M2.18: answering and sending, the sixth of the pass. A message is looked
  over before it goes, where Outlook asks only about a missing subject and a forgotten
  attachment, and only once Send is pressed: OpenMailClient says, while the message is still being
  written, that it has no subject, that its new text speaks of an attachment and none is
  attached (the quoted original is not read), that something in To, Cc or Bcc is not an address
  mail can be sent to, that an address is named more than once, and that a reply to all would
  show the others the writer had the message as a hidden (Bcc) recipient. `compose.send` refuses
  with those words and keeps the message open unless it is given `"anyway": true`;
  `compose.check` and `session.inspect` tell them. Reply All names each address once however
  the original listed it, and an answer to a message one sent oneself goes to those it went to,
  not back to oneself.
- **2026-10-11** - M2.19: folders, the seventh of the pass. Delete Folder removed the folder
  and all it held for good; it now puts the folder, whole, in Deleted Items as Outlook does
  (under a numbered name when one of that name is there), and only a folder already there, or
  `"permanently": true`, is removed. Move Folder (`folder.move`) is new, and takes a deleted
  folder back. Beyond Outlook: deleting says what went with the folder (messages, unread,
  folders); Empty Folder takes the folders inside with the mail and can remove only what is
  older than a number of days (`"olderThanDays"`); Mark All as Read can take the folders inside
  too (`"subfolders": true`) and says how many it marked; Folder Properties tells the path, the
  size, the oldest and newest message and the totals with the folders inside in one call; a
  rename onto a neighbour's name is refused, and names differing only in case are one name in
  any alphabet, not only in ASCII.
- **2026-10-11** - M2.20: undo, the eighth of the pass. What is done to a message can be taken
  back with `edit.undo` (Ctrl+Z), as in Outlook: marking, flagging, categories, importance,
  moving, archiving, junk and deleting. Beyond Outlook: a hundred steps are kept, not one; a
  message deleted for good can be taken back for as long as the session lasts; the message is
  shown where it stands again, as unread as it was; the result says what was undone and how many
  steps remain, and `session.inspect` what the next one is; a step whose message has since gone
  says so and is dropped without touching anything else. Found on the way and fixed: a move
  whose second half failed left the message in neither folder; it now goes back where it was.
- **2026-10-11** - M2.21: writing and files, the ninth of the pass. Starting a new message or
  a reply while one was being written wrote over it, and Discard dropped it; what a writer has
  put into a message is now set aside instead (twenty are kept), Undo brings a discarded one
  back, and `compose.restore` any of them, which Outlook cannot do once its prompt is answered.
  Attaching refuses a folder and the same file a second time and says what the message now
  carries; the checks before sending also tell of attachments over about 20 MB together, an
  empty one, and a program or script that mail servers refuse. Save As takes a folder and saves
  the message there under its subject made safe, replacing nothing (`"format"` picks msg, eml
  or oft). Opening a data file that is open already goes to it instead of opening it twice.
- **2026-10-11** - M2.22: Windows metafiles that are drawings. A picture kept as a metafile that
  draws lines and filled shapes left only its place; it is now drawn ([MS-WMF] 2.3.3): lines,
  polylines, polygons and sets of them (filled every other part or all that is wound round),
  rectangles, rounded ones, ellipses, arcs, pies and chords, single points, with pens and solid
  brushes that are made, selected and deleted, the window that is set, moved and scaled, states
  saved and put back, and a clipping box. It works for a picture written into an RTF body, for an
  attached object's picture of itself and for the small picture a message keeps of an
  attachment, on screen and in print. Better than the one it follows: edges are smoothed, where
  Windows plays a metafile back with stepped ones, and the page is made at twice the size it is
  shown at, so it stays sharp on a dense screen and on paper. A metafile that asks for anything
  not drawn (text, a bitmap beside the drawing, a patterned or hatched brush, a region, a pen
  that inverts) is not drawn at all and keeps its place: half a picture is worse. A drawing is
  held to 800 pixels a side, 50,000 records, 500,000 points and a fixed amount of work, so a
  hostile one costs little. The sample receipt has a small map drawn this way. Not yet: text in
  metafiles, and enhanced metafiles (EMF) that are drawings.
- **2026-10-11** - M2.23: enhanced metafiles that are drawings ([MS-EMF] 2.3.5). The newer kind
  of metafile, which is what Office writes for a chart or a shape, is drawn on the same page of
  pixels as the older: lines, polylines, polygons and sets of them with long or short numbers,
  curves, rectangles, rounded ones, ellipses, arcs, pies and chords either way round, single
  points; paths that are begun, closed, filled and drawn along; pens (also the wide ones
  measured in the drawing's units), solid brushes and the ones every device has; the world's
  turn set and added to, the window onto the viewport, states saved and put back; clipping to a
  box, to a path that is a box and to a region of one box. The page is the picture's frame, so
  what a file draws outside it is cut off as it should be. As with the older kind, edges are
  smoothed and the page is made at twice the size shown; a metafile that asks for anything not
  drawn (text, bitmaps, patterned brushes and pens, regions of many boxes, a mapping mode in
  parts of an inch, right-to-left layout) keeps its place, and so does one whose drawing is all
  in its comments. Not yet: text in metafiles.
- **2026-10-12** - M2.24: text in metafiles ([MS-WMF] 2.3.3.5 and 2.3.3.20, 2.2.1.2; [MS-EMF]
  2.3.5.7 and 2.3.5.8, 2.2.5, 2.2.13). A chart or a diagram with labels on it is now drawn, not
  left as a placeholder. The letters are filled as shapes, from the outlines of an installed
  font (nothing is bundled and nothing is fetched; a name from a message is only ever looked up
  among installed fonts): the font the drawing names, or an open font of the same measures
  where that one is not installed; bold and italic faces where the family has them, the upright
  face leaning where it has no italic. Read: fonts by the height of the em or of the line,
  turned lines (a label up the side of a chart), underlining and striking out, the colour of
  the letters and the colour behind them, lines set by their left, middle or right and by
  their top, bottom or the line they stand on, lines that each start where the last ended,
  the box that is filled or cut to, text in bytes in the font's character set and text in
  UTF-16. A line that cannot be drawn right keeps the whole picture a placeholder, as before:
  no font answers, the font lacks a letter, letters that join or run right to left, numbers of
  glyphs in place of characters, a font of symbols, a world that is turned (enhanced
  metafiles). The installed fonts are now listed once for the whole program (reading, print
  and drawings shared three lists before). Beyond Outlook: the spacing the drawing gives for
  each letter is kept, so labels stay where they were meant even when another font stands in;
  letters are smoothed and made at twice the size shown, so they stay sharp on a dense screen
  and on paper; and what a drawing says becomes the picture's own text, so it is there for a
  screen reader, for the tooltip and for anything that reads the message as words. Not yet: a bitmap beside a drawing, patterned
  brushes.
- **2026-10-12** - M2.25: an attachment that stands in the text of an RTF body ([MS-OXRTFEX]
  2.2.3.4, [MS-OXCMSG] 2.2.2.16) is shown where it stands as a small card: our own drawing of
  a sheet of paper, the attachment's name and its size. Before, it was a line of words in
  brackets. The name is the one the file would be saved under, so nothing in it is hidden or
  reversed. A click on the card asks where to save the attachment, as a click in the list
  above the message does; nothing is opened or run from the text. An object of another
  program that has no picture to show is a card too, with what it calls itself, and cannot be
  clicked. Read as words (print, plain text, anything that reads the message for an agent)
  the place still says `[Attachment: name]`. Beyond Outlook: the card says how large the file
  is, and a file to think twice about (a program, a name with two endings or with hidden
  characters, a document that carries macros) is marked in the text itself, with the reason
  when the pointer rests on it - not only once it has been opened. A long name is cut in its
  middle, so the ending stays in sight.
- **2026-10-12** - M2.26: a picture written into an RTF body as bytes (`\binN`, RTF 1.9.1,
  "Pictures") is read and shown like one written as hex digits; before, it left only its
  place. A picture may be written partly one way and partly the other. The bytes are never
  read as text or as RTF, whatever they hold (braces, backslashes, line ends, a `\bin` of
  their own), inside a picture or anywhere else, and the readers that only want a body's
  words pass over them too. A picture with fewer bytes than it promised, with a byte split
  between hex digits and bytes, or with more than a body may keep, leaves its place and
  nothing is kept of it. Nothing here goes beyond Outlook: this is a case it reads and
  OpenMailClient did not.
- **2026-10-12** - M2.27: an RTF body's tables may stand inside the cells of other tables
  (RTF 1.9.1, "Nested Tables": `\itapN`, `\nestcell`, and a row defined after its cells in
  `{\*\nesttableprops ... \nestrow}`), and cells merged across (`\clmgf`, `\clmrg`) are one
  cell as wide as all of them, with the left line of the first and the right line of the last.
  Before, an inner table's cells were lost into the text of the outer cell and a merged cell
  was drawn as its parts. An inner row's settings never touch the outer row's, the paragraph
  end written for readers that know no nested tables (`\nonesttables`) is passed over, and a
  cell holds its paragraphs and tables in the order they were written. Tables stand at most 8
  deep and a cell spans at most 64 columns; an inner table nothing closes ends with the cell
  it is in. The sample receipt has a row merged across and a table in a cell. Nothing here
  goes beyond Outlook: these are cases it reads and OpenMailClient did not.
- **2026-10-12** - M2.28: a paragraph of an RTF body has its border (RTF 1.9.1, "Paragraph
  Borders": `\brdrt`, `\brdrr`, `\brdrb`, `\brdrl`, `\box`, with the line, width and colour
  words cells already had, and `\brsp` for the room inside) and the room it asks for above
  and below (`\sb`, `\sa`), on screen and in print. Paragraphs that follow one another with
  the same border stand inside one; a border stands at the paragraph's indent with the text
  just inside it, and where the body says nothing of the room the text still does not touch
  the lines. A cell's lines and its paragraphs' lines are told apart. Room above is at most
  96 pixels and room inside a border at most 64. Crash fix, found by the new hostile-input
  test: a border width near the largest number (`\brdrw2147483647`) overflowed while being
  rounded, in cells as well as paragraphs; it now saturates
  (`hostile_rtf_paragraph_borders_and_spacing_never_panic`). The sample receipt's note has a
  border at its left and room around it. Not read yet: the line between paragraphs
  (`\brdrbtw`), line spacing (`\sl`). Nothing here goes beyond Outlook.
- **2026-10-12** - M2.29: a list paragraph of an RTF body is a list item: the number or bullet
  the writer worked out for it (`\listtext`, and `\pntext` of older writers; RTF 1.9.1, "List
  Text" and "Bullets and Numbering") stands in the room its indent leaves, with the text beside
  it, on screen and in print. Before, it was text followed by a tab at the start of the line.
  A bullet written as a character of a symbol font is the bullet it stands for; a number is at
  most 12 characters; a list paragraph with nothing after its number is still one. Numbers are
  not worked out from the list tables: a writer that leaves no list text gets no number. On
  screen a list marker (of HTML bodies too) is now drawn in the letters of the text it stands
  before, where it used to be in the window's own at one size. The sample receipt has a short
  numbered list. Nothing here goes beyond Outlook.
- **2026-10-12** - M2.30: a tab in an RTF body goes to a tab stop, on screen and in print. A
  paragraph's own stops (`\tx`, with `\tqc`, `\tqr` and `\tqdec` before it; RTF 1.9.1,
  "Tabs") are kept from where its text starts, and the text after a tab starts at a left stop,
  is centred on a centre stop, ends at a right stop, or has its decimal point at a decimal stop.
  Where a paragraph has no stop ahead, a tab goes to the next half inch; before, every tab was
  the width of four spaces. A paragraph has at most 32 stops, none further than 4096 pixels
  along. Not yet: leaders (the dots up to a stop), bar tabs (they are no stop), and stops on
  the lines a long paragraph wraps onto. Preformatted text has a stop every eight characters
  in print. The sample receipt has two lines set
  with a decimal stop. Nothing here goes beyond Outlook.
- **2026-10-12** - M2.31: a paragraph's lines are as tall as it says, on screen and in print. In RTF
  (`\sl` and `\slmult`; RTF 1.9.1, "Spacing") that is a multiple of the lines' own height,
  a least height or an exact one; in HTML it is `line-height` (CSS 2.1, 10.8.1) as a number, a
  percentage or a length, and it holds for the paragraphs inside the element that says it.
  What a line gains or loses is below its text. A multiple is kept between a half and ten,
  a height to 1024 pixels, a printed line is never taller than its page (a line told to be
  taller was set off the sheet: found by the hostile test and fixed with it), and a line is
  never shorter than a picture standing in it. Not compared with Outlook's own spacing yet,
  so where the room goes within a line is ours. Nothing here goes beyond Outlook.
- **2026-10-12** - M2.32: the room a tab takes can be filled, and paragraphs in one border can have a
  line between them. A stop's leader (`\tldot`, `\tlmdot`, `\tlhyph`, `\tlul`, `\tlth`,
  `\tleq`; RTF 1.9.1, "Tabs") is drawn from the text before the tab to the text after it, on
  screen and in print, as marks drawn by us and not as characters of the font. `\brdrbtw`
  (RTF 1.9.1, "Paragraph Borders") is the line between one paragraph and the next inside one
  border, with its own width, kind and colour; before, it was read as no border at all. Not
  yet: on screen, in a paragraph whose lines are told to be taller, a leader stands at the
  foot of its line and not at the foot of its letters. The line between paragraphs is covered
  by tests and has not been looked at on screen. The sample receipt has dot leaders. Nothing
  here goes beyond Outlook.
- **2026-10-12** - M2.33: a list paragraph whose writer left its number out is counted. The lists of
  a body (`\listtable`) and which of them a paragraph is of (`\listoverridetable`, `\ls`,
  `\ilvl`) are read (RTF 1.9.1, "List Table" and "List Override Table"): each level's kind of
  number (figures, roman numerals, letters, a nought before one figure, a bullet), where it
  starts, and the words around it, with the numbers of the levels above where the words ask
  for them ("4.a"). A number the writer did write (`\listtext`) is still the one shown, and
  the paragraph is still counted. Before, such paragraphs were plain text with no number.
  Not yet: a start or a level changed in the override (`\lfolevel`), the older `\pn` lists
  without `\pntext`, kinds of number other than those named, and the indent a level asks for
  where the paragraph gives none. The second step of the sample receipt's list is counted
  this way. Nothing here goes beyond Outlook.
- **2026-10-12** - M2.34: a block's background is behind all of the block. A background on a
  paragraph, a heading, a `div`, a list item or a quotation (CSS 2.1 section 14.2) fills the
  box its paragraphs stand in, on screen and in print; before, it was drawn as a highlight
  behind the letters only. Padding is kept for each side (`padding` with one to four lengths,
  `padding-top` and the others; CSS 2.1 section 8.4), and padding alone makes room without a
  border. In RTF, `\cbpat` (RTF 1.9.1, "Paragraph Shading") is the same fill. Text in a
  filled box is read against the fill, so text that names no colour stays readable on a dark
  one, in the light pane and the dark one. Not yet: boxes inside one another (the inner one
  is drawn alone, with the outer one's fill where it has none), a table inside a filled block
  (it stands outside the fill), a fill on the body itself, background pictures, lengths in
  `em` and percentages. The sample checklist message has a filled note. Nothing here goes
  beyond Outlook.
- **2026-10-12** - M2.35: a link can be followed. A click on a link in the reading pane (text or a
  linked picture) opens a question, never the link: the words of the link, the place it
  really goes in large letters, any concern from the link check (a name that is not the one
  the text shows, a name hidden before an `@`, a bare number), and the whole address, with
  Open in browser, Copy address and Cancel (Cancel has the focus). Only web pages (`http`,
  `https`; RFC 3986 characters, with a host) and single plain mailboxes (`mailto`, RFC 6068)
  are followed; a file, a script, a program's own scheme or an address in other characters
  is refused, and the question says why. A mailbox starts a new message to it. Commands:
  `mail.links` lists a message's links with where each goes, `mail.openLink` checks one and
  returns its address (it wants `"confirm": true` for a link with a concern, and only takes
  a link the message has), `ui.linkDialog` asks. The engine opens nothing: only the Open
  button in the window hands the address to the system's browser (one argument, no shell),
  so a command from the control channel or an agent cannot start one. That last step, the
  browser starting, was not run on the dev machine and is untested end to end. What this
  does on purpose: a link is never opened by the click alone.
- **2026-10-12** - M2.36: print sets text beside a picture at the side, and kerns. A picture a body
  puts at one side (`align`, `float`) is printed there, with the lines beside it shorter by
  its width and the lines after its foot under it; what follows its paragraph stands beside
  it too, until a paragraph asks to start below it or a table, a line or a bordered block
  comes, as on screen. It goes to the next page with its text when its page has no room,
  and stands in the line where less than 90 points would be left beside it. Pairs of
  glyphs stand as close as their face says: from the `kern` feature of the face's `GPOS`
  table (OpenType, pair adjustment, lookup type 2, both formats, through extension
  lookups; the default language system of `latn`, else `DFLT`) or else from its `kern`
  table (format 0). Lines are measured and broken with the pairs counted, and the PDF
  shows them with the `TJ` operator (PDF 32000-1, 9.4.3). A pair is held to half an em and
  never moves a glyph back past its start; tables cut short or filled with anything move
  nothing. Not yet: more than one picture at the sides at once, the reading pane's own
  kerning (it sets text as the toolkit does), shaping and right-to-left text. The printed
  pages were checked by their numbers (tests, and a sample message printed in the
  installed fonts, where 19 of 33 pieces of text carry pairs), not by eye: nothing on this
  machine draws a PDF, and the print preview is next. Nothing here goes beyond Outlook.
- **2026-10-12** - M2.37: page setup. `file.pageSetup` keeps how pages are set up for printing and
  says how they are: the sheet (A4, Letter, Legal, A3, A5, B5, Executive, Tabloid), upright
  or on its long side, a margin for each edge in millimetres, and a header and a footer of
  three parts each (left, middle, right; one line, a part longer than its share is cut
  with a mark). In them `[Page]`, `[Pages]`, `[Page of Pages]`, `[Date]`, `[Time]`,
  `[User]` and `[Subject]` are filled in; these names are our own. What a name gives is not
  looked through for names again, so a subject that says `[Page]` prints as it is. With no
  header or footer the body has their room. `file.print` prints with the setup and takes
  the same parameters for one print only. A margin is held to a third of the sheet, and
  sizes that are no numbers fall back to the least. Not yet: a window for it (it comes
  with the print preview, next), the setup kept between runs, a font or size of one's own
  for header and footer. Not compared with Outlook's page setup; nothing here is claimed
  to go beyond it.
- **2026-10-12** - M2.38: print preview. A new crate, `render` (L3), draws a laid-out page as
  pixels: fills, rules (dashed and dotted as in the PDF), pictures and the outlines of the
  glyphs, from the same pages the PDF file is written from, so what is seen is what is
  printed. `file.printPreview` says how many pages a message takes and, with a path,
  writes one page as a PNG picture (24 to 600 pixels to the inch); it takes the page setup
  for that look only and keeps nothing. In the window, File > Print and Ctrl+P now open
  a print window (`ui.printPreview`): the page beside the paper, the direction, the four
  margins, and the header and footer parts, each of which runs `file.pageSetup` and is
  seen at once; Previous and Next, and Page Up, Page Down, Home and End, turn the pages;
  "Print to PDF" asks for the file as before. Sides of a drawn page are held to 4096
  pixels. `xtask layers` now names the one dependency allowed inside a layer (render on
  layout). The hostile-parameters test no longer leaves two files in the crate's folder.
  Not yet: the setup kept between runs, a zoom, several pages side by side. Outlook's
  print pane was not observed, so nothing is compared with it and nothing here is claimed
  to go beyond it; for agents, a page as a picture from the command line is our own.
- **2026-10-12** - The project is renamed from MailCraft to OpenMailClient: crates, binaries
  (`openmailclient`, `openmailclient-cli`), the settings folder and the documents. The repository
  is public. Nothing else changes.
- **2026-10-12** - M2.39: Table Style. A folder's list is printed as it is shown (searched,
  filtered and ordered): the owner's name over a line, the folder's name, a line saying
  how many items there are, how many unread and how they are chosen, then a row to a
  message with From (To in Sent Items, Drafts and Outbox), Subject, Received, Size and
  Notes (High, Attachment, Flagged, Done). Unread rows are bold; the row of column names
  stands again at the top of every page the table goes on to (`Table::repeat`, new in the
  layout). `file.print` and `file.printPreview` take `"style": "memo"` or `"table"`,
  `file.pageSetup` keeps it, and the print window chooses between the two; printing is
  now offered whenever a folder is shown, and Memo Style with no message open says so.
  `[Folder]` can be asked for in a header or footer. A list longer than 5,000 rows is cut
  and says so; a column is never narrower than a short word. The columns and the head of
  the page are our own: Outlook's Table Style was not observed (it would need its print
  dialog), so nothing is compared with it or claimed to go beyond it. Not yet: the
  columns of the view as the person arranged them, group headings, the selected rows
  only.
- **2026-10-12** - M2.40: bitmaps beside a metafile's drawing. A Windows or enhanced metafile that
  draws shapes or text and one or more bitmaps is now drawn whole, where it used to be left as
  its place: the bitmap's pixels are read ([MS-WMF] 2.2.2.9: 1, 4, 8, 16, 24 and 32 bits to
  the pixel, a table of colours or bit fields, bottom row first or top row first, the
  twelve-byte header too) and laid over the box the record names, inside what the drawing is
  cut to and under what is drawn after it (META_STRETCHDIB, META_DIBSTRETCHBLT, META_DIBBITBLT;
  EMR_STRETCHDIBITS, in a world that is scaled and moved). Limits: a bitmap side of 4,096
  pixels and 8 million pixels for all the bitmaps of one drawing, so a file of many large
  bitmaps is given up soon. Still not drawn, and so the metafile stays a place rather than
  half a picture: a part of a bitmap, one mirrored or turned, a copy that is not a plain one,
  colours that are numbers in a palette, pixels packed in runs (RLE), EMR_BITBLT and its
  kin, META_SETDIBTODEV beside a drawing. Checked by tests on files we build; no metafile of
  Outlook's was compared, and nothing here goes beyond Outlook.
- **2026-10-12** - M2 closed. Formatted reading is done as far as this milestone takes it: HTML and
  RTF bodies are laid out and drawn with their fonts, lists, tables, borders, fills and
  pictures (PNG, JPEG, GIF's first frame, bitmaps, metafiles that are drawings), attachments
  stand in their places, links open after being asked, and a message or a folder's list
  prints to a PDF file with a page setup and a preview. What M2 does not do is listed in the
  roadmap under "What M2 leaves" and ranked in docs/gaps.md (item 3); none of it is claimed
  as done. Nothing printed was compared with Outlook's print output. M1's checks against
  Outlook-written files stay open. Next is M3, writing `.pst`.
- **2026-10-12** - What M2 left is now scheduled, not just listed: "Do later (from M2)" gives
  every item a slot (M2.41 and M2.42 before M3, a batch M3b after it, M4, M5), and an item is
  struck from the table when it lands. Right-to-left text and shaping are taken first, before
  M3, because without them whole languages are unreadable.
- **2026-10-12** - M2.41: right-to-left text. A paragraph now knows which way it is written: HTML `dir`
  (`rtl`, `ltr`, `auto`) and `direction` in a style, with `text-align: start` and `end` as sides
  of that way; RTF `\rtlpar` and `\ltrpar`; plain text by its first letter that has a
  direction, paragraph by paragraph. A right-to-left paragraph starts at the right unless it is
  told a side. Each line is put in the order it is seen in by the Unicode rules (UAX #9, with
  the `unicode-bidi` crate for the levels and our own L1, L2 and L4: lines are broken first and
  turned one by one, brackets are mirrored), in the reading pane and in print, so Hebrew and
  Arabic read from the right with Latin words and numbers inside them the right way round. In
  print, Arabic letters take the shapes they have beside their neighbours (the presentation
  forms, with the lam-alef ligatures) where the font has them; on screen egui's shaper already
  joins them from the font's own tables, and is handed one piece of a word at a time because
  it takes everything it is handed at once to run one way. Limits: 200,000 characters to a
  paragraph in print, 60,000 characters and 8,000 pieces on screen, past which the text is
  drawn as it is stored. Made better on the way, for every table: text at the right or the
  middle of a cell narrower than the least a text wraps at was drawn past the cell's edge, on
  screen and in print, and now stays inside it. Moved to "Do later": selecting text in a
  paragraph that was turned, the rest of a right-to-left paragraph's furniture, and
  right-to-left text outside the body (the list, the header, folder names). Checked by tests
  on text we wrote and by looking at a Hebrew and Arabic message of ours on screen and on a
  printed page; Outlook's drawing of the same message was not compared, and nothing is claimed
  about it.
- **2026-10-12** - M2.42: shaping in print. A stretch of text in one face, one look and one direction is
  now handed whole to the face's own `GSUB` and `GPOS` tables (through `harfrust`, a Rust port
  of HarfBuzz, MIT), in print, in the print preview and in a PDF: letters drawn as one glyph
  are (fi, ffi), pairs stand as close as the font says, accents and vowel signs stand where the
  font puts them, Arabic is joined by the font's own shapes with its marks placed, and scripts
  that need their letters reordered or stacked (Devanagari, Thai) come out as the font draws
  them. A stretch is shaped knowing the letters beside it, so Arabic joins across a change of
  look (bold, colour) inside a word. A face with neither table, Arabic in a face that does not
  join it, and any answer that is out of order or names a glyph the face has not got keep the
  way of M2.41 (our own pairs and the presentation forms). Made better on the way: a PDF now
  says what its glyphs stand for (`/ActualText`) wherever that is not the glyphs' own
  characters one after another, and the pieces of a turned line are written in the order they
  are read, so text copied from or searched in a PDF of Hebrew, Arabic or ligatures is the text
  that was written; and a character the italic or bold face of a family lacks (Arial Italic has
  no Arabic) is taken from the family's plain face, leant or thickened, where it was a box
  before. Limits: a glyph is put no further from its place than twice its size and never
  outside the line's room; a piece says at most 4,096 UTF-16 units. Not shaped yet, in "Do
  later": the name and page number at the head and foot of a page, and list markers. Checked
  by tests on fonts we make ourselves (with `GSUB` and `GPOS` tables written by the tests, and
  300 damaged copies of them) and by looking at a printed page of our own text in Latin,
  Arabic, Hebrew, Devanagari and Thai with the fonts installed here; no PDF reader's copying
  was tried, and Outlook's printing of the same text was not compared.
- **2026-10-12** - M3.1: the lowest layer of a `.pst` is written. `crates/pst/src/write.rs` makes a whole
  Unicode data file (512-byte pages, the kind Outlook 2003 and later open) from blocks and
  nodes: the header with both checksums, an allocation map in front of every 253,952 bytes and
  the page maps older readers expect, the density list, blocks with their trailers, data trees
  of one and two levels for items up to 4 GB, subnode trees with an index block, and the two
  B-trees of pages, with every reference count kept. Blocks can be left plain or encoded either
  way Outlook does. A file is always written whole and front to back, never changed in place,
  which is what the temporary-file-and-swap of M3.4 needs. Nothing a user can reach writes a
  `.pst` yet: the layers above (M3.2, M3.3) are next. Made better on the way: blocks with the
  same bytes are stored once and counted (the same attachment in fifty messages is one set of
  blocks; the format allows it through its reference counts); and
  `openmailclient-cli inspect file.pst --check` now also holds the file's structure against
  the format - header checksums, allocation maps, every page and block checksum and signature,
  blocks that overlap or lie in space called free, and reference counts - where it only loaded
  every item before (opening a mailbox checks no checksums). The check reads only; it covers
  Unicode files with 512-byte pages and says so for ANSI and 4K-page files (slot: M3b).
  Checked by 8 tests: what is written is read back by our reader in all three encodings, a
  33 MB item across 133 sections, 800 subnodes, 1,000 nodes, refusals for what the format cannot
  hold, and 400 damaged copies that the check and the reader survive. Not checked: no file
  from this writer has been opened in Outlook (there is nothing above the node layer to open
  yet), and the places of the free map and free page map pages in files over 32 MB and 2 GB are
  worked out from the coverage the specification gives, not compared with a file Outlook wrote.
  The bit order of the allocation maps was seen in the structure of a local data file; our
  files fill them in whole bytes, so they do not depend on it.
- **2026-10-12** - M3.2: the middle layer of a `.pst` is written. `crates/pst/src/ltp_write.rs` builds a
  heap over as many blocks as it needs (with the block headers and fill levels of each kind),
  the index kept in a heap with as many levels as it needs, and on them a property list and a
  table: values of up to four bytes in the record (eight in a table row), up to 3,580 bytes in
  the heap, larger ones in a subnode of their own; a table's rows in the heap or, when they
  are many, in a subnode whose blocks each hold whole rows. Values go in and come out as the
  bytes the file stores, so a property this code does not understand is written as it was
  read. Still nothing a user can reach writes a `.pst`: folders and messages are M3.3. Checked
  by 5 tests, all through our own reader and the structure check: every kind of value the
  model has, 3,000 properties in one list (more than eight heap blocks, an index of two levels), 6,000 rows
  in one table, rows with cells missing, and refusals for a property or a row id given twice
  and for more than 255 columns. Not checked: nothing here has been opened in Outlook, and
  which half of a byte holds the earlier block's fill level is not said by the specification
  and was not seen in a file (no local data file has a heap of several blocks). Noted for
  M3.4: the model keeps a 32-bit float as a 64-bit one and a currency or error value as an
  integer, so those types change when an item is written from the model; items that were not
  changed will be copied as stored.
- **2026-10-12** - M3.3: a whole mailbox is saved as a `.pst`. `crates/pst/src/store_write.rs` writes the
  messaging layer over M3.1 and M3.2: the message store, the name-to-id map, the root folder,
  the search folders and tables every data file must have ([MS-PST] 2.7.1), then each folder
  with its hierarchy, contents and associated-contents tables, each message with its
  recipients and attachments, and messages attached to messages (to 32 deep). The command is
  `file.export` (File > Export to a Data File); `openmailclient-cli run --save out.pst` and
  `openmailclient-cli convert in.pst|in.ost out.pst` reach it too. Checked through our own
  reader only: the sample mailbox and a mailbox of 1,500 messages come back the same, a
  saved file saved again gives the same mailbox, and the structure check finds nothing; three
  local `.ost` copies went through with every item counted on the other side. **Not opened in
  Outlook** (that adds a data file to the owner's profile and waits for their say), so two
  readings of the specification stand unproved: where a message attached to a message is kept,
  and the bucket a named property falls in. Made better on the way: the new file is written
  beside the target, synced, held against the structure check and only then put in the
  target's place, so a failed export leaves what was there byte for byte and an unsound file
  never lands; a source that cannot give an item or a folder's list loses only that, and the
  result says how many (a damaged file can be salvaged into a sound one); the contents tables
  carry the sender's name and the flag, so a list shows them without opening each message; a
  data file's folders are now listed parent-then-children, as the folder pane shows them
  (`inspect` indented them wrongly before). Left for later, each with a slot: hidden
  (associated) items and folder properties beyond name, class and counts; the ids that tell
  Outlook which folder is the Calendar or Contacts; export off the UI thread with progress;
  table cells whose type differs from the column's.
