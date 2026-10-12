# MailCraft roadmap

**Status:** M1 (data files first) and M2 (formatted reading) in progress · revision 35 · 2026-10-12

## Headline numbers

| Measure | Now |
|---|---|
| Catalogued Outlook Classic features with a command | 41 of 115 (36%), see [docs/parity-checklist.md](docs/parity-checklist.md) |
| Tests | 173, all passing |
| File formats read | `.pst`/`.ost` (ANSI, Unicode, 4K pages), `.msg`, `.oft`, `.eml` |
| File formats written | `.msg`, `.oft`, `.eml`; a message prints to `.pdf` |
| Protocols | none |

## Milestones

| | Milestone | State |
|---|---|---|
| M1 | **Data files first**: open `.pst`/`.ost`/`.msg`/`.eml` in an Outlook-Classic-style window; save messages Outlook can reopen | in progress |
| M2 | Formatted reading: HTML and RTF bodies laid out and drawn, inline pictures, printing | in progress: HTML and RTF text, lists, tables, pictures and named fonts are drawn; a message prints to a PDF file |
| M3 | Writing `.pst`: journalled, crash-safe writes; rules and folder changes persist | not started |
| M4 | Accounts: IMAP, POP3 and SMTP behind an account trait, with an in-process fake server for tests | not started |
| M5 | Calendar, People and Tasks modules | not started |
| M6 | MCP server, control channel, web build | not started |

## Current focus (M1)

1. Check the `.pst` reader against more real Outlook-written files, locally (results are never
   published). Done so far: a few small 4K-page `.ost` files.
2. Have Outlook Classic open MailCraft-written `.msg` files and compare, property by property. Done
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
23. Next: what is left of M2, in this order: list numbers from the list tables; backgrounds and
    padding on HTML blocks, and opening a link; text beside pictures and kerning in print;
    page setup, Table Style and a print preview; metafiles with a bitmap beside the drawing.

The ranked work list is [docs/gaps.md](docs/gaps.md).

## Progress log

- **2026-10-11** — M1.1: item model, MIME, `.msg` and `.pst` readers, engine with 48 commands, CLI.
  M1.2: the egui window (ribbon, folder pane, message list, reading pane, inline compose, File page),
  headless screenshots, the desktop app, and `cargo xtask ci`.
- **2026-10-11** — M1.3: `.ost` files are accepted (client magic "SO"); 4K-page reading confirmed
  against real offline stores, locally.
- **2026-10-11** — M1.4: Outlook Classic opens MailCraft-written `.msg` and `.oft` files and reads the
  same properties back; a file Outlook wrote survives a trip through MailCraft. Fixed the named-property
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
  a link's address when the pointer rests on it and leaves the comparing to the reader. MailCraft
  compares: a link whose text names one place and goes to another, one with a name before an `@`
  in its address, one that goes to a number, one written in letters that can pass for others, and
  one that is no web address at all (`javascript:`, `file:`) is underlined in the warning colour
  before anyone points at it, and says what is wrong in a sentence when they do ([RFC 3986] 3.2).
  A name and its own sub-sites count as one place, and a file's name is not taken for a site.
  Nothing is opened, fetched or resolved to tell. The sample's Junk Email folder has a message
  with each kind.
- **2026-10-11** - M2.15: saving attachments, the third of the pass. Outlook saves an attachment
  under the name the message gave it and asks before replacing a file. MailCraft has Save All
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
  so a printed message from Outlook does not say where its links go. MailCraft's does: a link
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
  attachment, and only once Send is pressed: MailCraft says, while the message is still being
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
  MailCraft did not.
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
  goes beyond Outlook: these are cases it reads and MailCraft did not.
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
