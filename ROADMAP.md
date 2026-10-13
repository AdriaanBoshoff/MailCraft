# OpenMailClient roadmap

**Status:** M2 (formatted reading) closed; M1's checks against Outlook still open; M3 (writing `.pst`) closed: a mailbox is written as a `.pst`, changes to an open one are kept when saved, settings are kept between runs; next the do-later batch (M3b) · revision 79 · 2026-10-12

## Headline numbers

| Measure | Now |
|---|---|
| Catalogued Outlook Classic features with a command | 44 of 117 (37%), see [docs/parity-checklist.md](docs/parity-checklist.md) |
| Tests | 322, all passing |
| File formats read | `.pst`/`.ost` (ANSI, Unicode, 4K pages), `.msg`, `.oft`, `.eml` |
| File formats written | `.msg`, `.oft`, `.eml`; a message prints to `.pdf` |
| Protocols | none |

## Milestones

| | Milestone | State |
|---|---|---|
| M1 | **Data files first**: open `.pst`/`.ost`/`.msg`/`.eml` in an Outlook-Classic-style window; save messages Outlook can reopen | in progress |
| M2 | Formatted reading: HTML and RTF bodies laid out and drawn, inline pictures, printing | closed: HTML and RTF text, lists, tables, pictures and named fonts are drawn; a message or a folder's list prints to a PDF file, with a page setup and a preview. See "Do later (from M2)" |
| M3 | Writing `.pst`: journalled, crash-safe writes; rules and folder changes persist | closed: a mailbox is saved as a new `.pst`, changes to an open Unicode `.pst` are kept when saved, settings are kept between runs. Journalled changes are in M3b and rules in M5; see "Do later (from M2)". Not yet opened in Outlook |
| M3b | The do-later batch from M2 and M3 (see "Do later (from M2)") | in progress: unsaved changes are journalled (M3b.1), export copies a `.pst` as stored (M3b.2), panes and window kept (M3b.3), right-to-left text outside the body (M3b.4), print's own text shaped (M3b.5), right-to-left lists and quotations (M3b.6), the journal's loose ends (M3b.7), right-to-left text typed in a field (M3b.8), what was left of a right-to-left paragraph (M3b.9), copying out of a PDF tried in a reader (M3b.10), attachments and search-folder rows of a changed message (M3b.11), a save that does not hold the window (M3b.12), the structure check inside nodes (M3b.13), an export that does not hold the window (M3b.14), the other modules' folders named by id (M3b.15), value kinds, header counters and the keep-original choice (M3b.16), attachments with new contents and ones that cannot be read (M3b.17), cells of another type, the check hearing Stop and fill levels (M3b.18), an export of any mailbox on a thread of its own (M3b.19), the structure check for ANSI and 4K-page files (M3b.20), ANSI data files written and kept as ANSI (M3b.21), Table Style columns, headings and selected row, preview zoom (M3b.22), SVG, WebP and TIFF pictures and animation (M3b.23), several pictures at the sides and paper behind dark see-through ones (M3b.24), boxes inside one another and a colour behind the body (M3b.25), borders round text and rounded corners (M3b.26), RTF highlight colours and list overrides (M3b.27) |
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
37. Done: changes to an open Unicode `.pst` are kept: `file.save` writes the file anew beside itself, with everything untouched copied as it is stored (M3.4).
38. Done: settings are kept between runs: the page setup, the panes, the theme and ribbon, and the data files that were open (M3.5). M3 closed.
39. Done: unsaved changes to a data file are journalled as they are made and come back after a run that ended without saving (M3b.1).
40. Done: `file.export` of a Unicode `.pst` copies it as it is stored: hidden items, folder properties and unknown properties go along (M3b.2).
41. Done: the edges of the panes are dragged, and the window's size and place and the panes' sizes are kept between runs (M3b.3).
42. Done: right-to-left text reads the right way in the message list, the reading pane's header and folder names, and a turning mark cannot disguise a name (M3b.4).
43. Done: the head and foot of a printed page and the markers of lists are shaped and turned as the body is (M3b.5).
44. Done: a paragraph written from the right has its indent, its list marker and its quotation bars at the right, on screen and in print (M3b.6).
45. Done: the journal of unsaved changes is kept in a folder of our own when it cannot be kept beside the data file, is forced to disk once a second, and says exactly how many changes a damaged journal lost when that can be known (M3b.7).
46. Done: a line that is typed (the search box, a folder's new name, the address and subject lines, a page's head and foot) is seen in the order it is read while it is typed, with the cursor, the selection and a click where the text is seen (M3b.8).
47. Done: what was left of a paragraph written from the right: tab stops and their leaders are counted from the right, a border's own indent is at the right, text stands beside a picture on the right side of it, the columns of a table written from the right start at the right, and `<bdo>`, `<bdi>`, `dir` on a piece of a line and `unicode-bidi` say which way their text runs, on screen and in print (M3b.9).
48. Done: text copied out of our PDFs was tried in a reader (xpdf's `pdftotext` 4.06) and what it found was put right: text written from the right is given as it is seen, as readers expect it, and only glyphs that are not one letter each say what they stand for (M3b.10).
49. Done: when a message's list of attachments changes, each attachment the file already holds keeps what is stored on it; a search folder's table shows a changed message as it is now (M3b.11).
50. Done: a save asked for in the window is done on a thread of its own, with a bar that fills in the status bar and a way to stop it (M3b.12).
51. Done: the structure check looks inside the nodes: every heap, index, property list and table is held against the specification (M3b.13).
52. Done: an export of a data file runs on a thread of its own, with progress and a way to stop it (M3b.14).
53. Done: an exported file names the Calendar, Contacts, Tasks, Notes, Journal and Drafts folders by id on its root and its Inbox (M3b.15).
54. Done: the small things left in the writer: values keep their kind, a rewritten file goes on from the header's counters, and the file as it was need not be kept (M3b.16).
55. Done: a file attached again with other contents keeps what is stored on it, and an attachment that cannot be read stays where it is (M3b.17).
56. Done: a value of another type than its column's is shown as the column holds it; the structure check hears Stop and compares a heap's fill levels with its blocks (M3b.18).
57. Done: an export of any mailbox runs on a thread of its own: the sample, an `.ost`, an ANSI `.pst` and a 4K-page file as well (M3b.19).
58. Done: the structure check covers ANSI data files and files with 4K pages (M3b.20).
59. Done: ANSI data files are written, and a re-saved one keeps its kind (M3b.21).
60. Done: Table Style prints chosen columns, group headings and the selected message; the print preview has a zoom (M3b.22).
61. Done: SVG, WebP and TIFF pictures are shown, and pictures that move are played (M3b.23).
62. Done: Several pictures stand at the sides of one paragraph, and a dark see-through picture has paper behind it on the dark pane (M3b.24).
63. Done: Boxes stand inside one another, a table stands inside the block it was written in, and a body has its colour behind all of it (M3b.25).
64. Done: Text within a line is drawn in its border, and boxes and cells have rounded corners, in the window, on paper and in the PDF (M3b.26).
65. Done: An RTF highlight is one of the sixteen marker colours, and list numbers follow what their override changes (M3b.27).
66. Next: M3b.28: metafiles (a part of a bitmap, mirrored and turned ones, run-packed ones, copies that are not plain, patterned brushes, regions of many boxes).

## Do later (from M2)

M2 closed with these not done. They are not dropped: each has a slot, and is struck from this
table when it lands. The slots are a plan, and an item moves up when it starts to hurt.

| Do later | When |
|---|---|
| ~~Right-to-left text (Hebrew, Arabic) in the right order, and Arabic letters joined, on screen and in print~~ | done: M2.41 |
| ~~Shaping from the font's own tables in print (ligatures, marks placed, Indic and other complex scripts; Arabic joined by the font's tables and across a change of look; copying text out of a PDF in the order it is read)~~ | done: M2.42 |
| ~~Shaping for the text print sets itself: the name and page number at the head and foot of a page, list markers, the header's field names and values~~ | done: M3b.5 |
| ~~Copying text out of our PDFs tried in a PDF reader (xpdf's `pdftotext`)~~ | done: M3b.10 |
| Copying text out of our PDFs tried in the readers people use most (Acrobat, a browser's viewer): none of them gives its text to a script here | when one can be tried without driving a window |
| ~~Right-to-left text in the message list, the header of the reading pane and folder names~~ | done: M3b.4 |
| ~~Right-to-left text while it is typed in a field of one line: the search box, a folder's name being changed, the address and subject lines~~ | done: M3b.8 |
| Right-to-left text while it is typed in the body of a message being written (the toolkit's box draws it as stored; the body gets its own editor with the item windows) | M5 |
| A right-to-left line in the list, the header and the folder pane set from the right of its cell | M5, with views |
| ~~For a right-to-left paragraph: list markers, indents and quotation bars at the right~~ | done: M3b.6 |
| ~~For a right-to-left paragraph: tab stops and their leaders counted from the right; a frame's own indent at the right; text beside a picture at the side; columns of a right-to-left table from the right; `<bdo>` and `unicode-bidi`~~ | done: M3b.9 |
| Selecting and copying text in a paragraph with right-to-left writing; Arabic joined across a change of look on screen; other complex scripts checked on screen (egui shapes each piece from the font's tables; only Hebrew and Arabic were looked at) | with M5, when the item windows get their own text drawing |
| ~~Keeping the page setup between runs~~ | done: M3.5 |
| Rules kept between runs (there are no rules yet; they are kept in the data file as Outlook keeps them, and in the settings for accounts without one) | M5, with rules |
| ~~The window's size and place and the widths of the panes kept between runs~~ | done: M3b.3 |
| The columns of the list kept between runs (the list has no columns yet: they come with the single-line view) | M5, with views |
| The window's place on a second screen kept (the size is; egui does not say where the other screens are) | M5 |
| Each folder's own view (sort, grouping, reading pane) kept, in the data file where Outlook keeps it | M5, with views |
| A settings page in the app (the file is written and read; nothing shows or edits it but the commands that change each setting) | M5 |
| A `.pst` from our writer opened in Outlook Classic (needs the owner's say: it adds a data file to the profile); with it, where a message attached to a message is kept and the bucket of a named property, both written from the specification alone | when the owner says so |
| ~~Hidden (associated) items and a folder's own properties beyond name, class and counts kept when an open `.pst` is saved~~ | done: M3.4 |
| ~~Hidden (associated) items and a folder's own properties carried by `file.export` when the source is a Unicode `.pst`~~ | done: M3b.2 |
| ~~Hidden items and a folder's own properties carried by `file.export` from an ANSI `.pst`~~ | done: M3b.21 |
| Hidden items and a folder's own properties carried by `file.export` from an `.ost` and a 4K-page file (read through the model still) | with writing 4K-page files |
| ~~Changes journalled as they are made: a crash loses what was not saved~~ | done: M3b.1 |
| ~~The journal kept somewhere else when the data file's folder cannot be written; forced to disk on a timer as well as every 64 records; a count of records lost after a damaged one that is exact~~ | done: M3b.7 |
| Unsaved changes saved on their own after a while (waits for the writer to be proved in Outlook: nothing writes a data file unasked until then) | when the owner says so, with the Outlook check |
| ~~Saving off the UI thread, with progress~~ | done: M3b.12 |
| A save that does not rewrite the whole data file (needs writes into the data file that are journalled themselves) | M3b |
| ~~Unknown properties of an attachment kept when a message's attachments change~~ | done: M3b.11 |
| ~~Unknown properties of an attachment whose contents were changed too, and stored attachments that cannot be read~~ | done: M3b.17 |
| ~~The cells of a changed message in search-folder tables~~ | done: M3b.11 |
| Working out again which search folders a changed message belongs to (its rows are brought up to date; a row is not added or taken away) | M5 |
| ~~Changes to an ANSI `.pst` kept~~ (an `.ost` stays read-only for good: it is a server's copy) | done: M3b.21 |
| Changes to a 4K-page `.pst` kept | with writing 4K-page files |
| ~~The node-id counters of a file's header carried into the rewritten file~~ | done: M3b.16 |
| ~~A choice not to keep the file as it was beside a data file on its first save~~ | done: M3b.16 (`file.keepOriginal`) |
| ~~The ids on the root and the Inbox that tell Outlook which folder is the Calendar, Contacts, Tasks, Notes, Journal and Drafts, for a file made by `file.export`~~ | done: M3b.15 |
| ~~Export off the UI thread, with progress and a way to stop~~ | done: M3b.14, for a Unicode `.pst` copied as stored |
| ~~Export off the UI thread for a mailbox written through the model (the sample, an `.ost`, an ANSI `.pst`, a 4K-page file)~~ | done: M3b.19 |
| ~~The structure check hearing Stop~~ | done: M3b.18 |
| ~~Table cells whose type differs from the column's kept in a saved `.pst`'s tables~~ | done: M3b.18, where nothing is lost by it; otherwise the cell is empty and the item keeps the value |
| ~~The structure check for ANSI and 4K-page (`.ost`) files~~ | done: M3b.20 |
| The structure check on a 4K-page file's heap fill levels, on the last eight bytes of its trailers and on a second section's map pages | with a 4K-page file that shows them, or a public description |
| ~~The structure check reaching into heaps, property lists and tables~~ | done: M3b.13 |
| ~~The structure check comparing a heap's fill levels with its free space~~ | done: M3b.18 |
| The structure check run on a 512-byte-page `.pst` written by Outlook | with the first such file we may read |
| ~~32-bit floats, currency and error values keeping their type when an item is written from the model~~ | done: M3b.16 |
| A heap's fill levels compared with an Outlook-written heap of several blocks | with the first such file we may read |
| The places of free map and free page map pages compared with an Outlook-written file over 32 MB | with the first such file we may read |
| ~~Writing ANSI data files (a re-saved file keeps its kind)~~ | done: M3b.21 |
| Writing 4K-page data files | when the owner allows a file of ours to be opened in Outlook, or with a public description of the 4K layout: it is in no specification, and a guess cannot be proved without one of the two |
| What Outlook makes of Unicode text in an ANSI file of ours, and of wVer 14 where a file had 15 | M1 checks against Outlook, with the owner's say |
| ~~The selected row only, chosen columns and group headings in Table Style~~ | done: M3b.22 |
| Several messages selected at once (Table Style's selected rows then follow the selection) | M5, with interaction parity |
| Columns of the message list chosen in the window (Table Style then follows them) | M5, with interaction parity |
| ~~A zoom in the print preview~~ | done: M3b.22 |
| ~~SVG, WebP and TIFF pictures; GIF animation~~ | done: M3b.23 |
| SVG drawings in the web build (one thread, a small stack); bitmaps inside a drawing, under the limits of other pictures; `.svgz` | M6, with the web app |
| Pictures decoded off the thread that draws the window | M4, with pictures loaded from a server on command |
| ~~A backing for see-through pictures on the dark pane; several pictures at the sides of one paragraph~~ | done: M3b.24 |
| On paper, a table or a bordered block beside a picture at the side (in the window it stands beside it) | M5, with the other print styles |
| ~~Boxes inside one another; a fill behind a table in a block and behind the whole body~~ | done: M3b.25 |
| ~~Borders on text within a line; rounded corners~~ | done: M3b.26 |
| ~~Outlook's highlight colours; list numbers an override changes (`\lfolevel`)~~ | done: M3b.27 |
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
- **2026-10-12** - M3.4: changes to an open `.pst` are kept. A Unicode `.pst` now opens editable: reading,
  flagging, moving, deleting, new messages and folder changes are held beside the open file
  (`crates/pst/src/edit.rs`), which stays byte for byte as it was until `file.save` (File >
  Save Changes, Ctrl+S). Saving writes a whole new file beside the old one. Every node the
  changes do not touch is copied as it is stored, block for block, so hidden (associated)
  items, views, search folders, folder properties and values the model has no place for go
  across untouched; a changed message keeps every stored property that was not changed, and
  its recipients and attachments as stored when they are the same; a moved message keeps its
  node and its id. The new file is held against the structure check, read back and compared
  with what was shown, and only then put in the old one's place in one step. A save is
  refused, with the file left alone and the changes still there, when another program wrote
  the file since it was opened or (on Windows) has it open. An `.ost`, an ANSI file and a
  4K-page file stay read-only. Closing a data file with unsaved changes is refused unless it
  says `{"save": true}` or `{"discard": true}`; leaving the app with unsaved changes says so
  once and needs a second asking. Nothing is ever written without being asked for. Checked
  through our own reader only (the sample mailbox, a file given content our writer never
  makes, damaged files, and three local copies converted and then changed: counts only);
  **not opened in Outlook**. Made better on the way: the first save of a data file keeps the
  file as it was beside it (`name.before-openmailclient.pst`, a second name for the same bytes,
  so no copying), and later saves never write over that; the File page says which data files
  have unsaved changes. Left for later, each with a slot: changes journalled as they are made
  (a crash loses what was not saved, never the file); a save that does not rewrite the whole
  file; saving off the UI thread; unknown properties of an attachment when a message's
  attachments change; stale cells for a changed message in search-folder tables; `file.export`
  still goes through the model.
- **2026-10-12** - M3.5: settings are kept between runs, and M3 is closed. `settings.save` and
  `settings.load` (`crates/engine/src/cmd/settings.rs`) write and read one small JSON file:
  the page setup and print style, where the reading pane is, whether the folder pane shows,
  the data files that were open, and what the window keeps for itself (light or dark, the
  ribbon's kind, bodies read as plain text). The app keeps it in the per-user configuration
  folder (`%APPDATA%\OpenMailClient\settings.json`; `~/.config/openmailclient/` on Linux,
  `~/Library/Application Support/OpenMailClient/` on macOS) and opens last time's data files
  again unless the run names a file or `--sample`; `--no-settings` neither reads nor writes
  it. The file is read as anything else from disk: capped at 1 MB, taken a part at a time, a
  part that makes no sense left out and counted, a file that is not ours changing nothing
  and never failing the start; only `.pst`/`.ost` paths are opened again, at most 32, and
  one that is gone is counted and said in the status line. It is written beside itself and
  put in place in one step. No passwords or tokens will ever go in it (CLAUDE.md: those live
  in the OS credential store). Made better on the way: settings are written when they
  change, not only on the way out, so a run that ends badly keeps them; each part stands by
  itself, so one bad value never costs the rest; the file is plain JSON a person can read
  and carry to another machine. M3 closes with what it was for done (a mailbox written as a
  `.pst`, changes to an open one kept, a check of the structure, settings kept); what its
  first wording also named has a slot: changes journalled as they are made (M3b) and rules
  kept between runs (M5, where rules are built). Nothing our writer made has been opened in
  Outlook yet (needs the owner's say).
- **2026-10-12** - M3b.1: unsaved changes are journalled. Each change to an open Unicode `.pst` is now
  also written, as it is made, to a journal beside the file (`name.pst.openmailclient-journal`,
  `crates/pst/src/journal.rs`); the data file itself is still written only by `file.save`.
  When the file is opened again after a run that ended without saving, `file.open` plays the
  journal back and the changes are there, unsaved as they were (`{"recover": false}` leaves
  them where they are); `file.save` puts them in the file and takes the journal away, and
  closing with `{"discard": true}` throws both away. The journal is a list of what was asked
  for, each record a length, a checksum and the request, and is tied to the file it was made
  for by the file's length and the sum of its header: a journal for a file that anyone has
  written since is never applied (it is left alone, and kept under `.old` when a new one
  starts). Playing back stops at the first record that is cut short, damaged or cannot be
  done; what came before stands, the journal as found is kept under `.old`, and the status
  line says how many were lost. A journal is read as hostile: lengths are believed only as
  far as the file goes, counts only as far as the record goes, messages attached to messages
  16 deep. Two running copies do not write one journal: the second is told, and its changes
  live in memory only (the File page and the word at exit say so). Made better on the way: a
  change to a message is journalled as what changed in it, and a move as the move, so marking
  a message with a 600 KB attachment as read and moving it costs under 1 KB; leaving the app
  with unsaved changes no longer loses them, and the word at exit says they come back. Each
  record reaches the system as it is made (a crash of the program loses nothing) and the disk
  every 64 records (a power cut may cost the last few; the cut record is dropped, never half
  applied). Not yet, each with a slot: the journal is beside the data file only (a folder
  that cannot be written means no journal, said on the File page); a save still rewrites the
  whole file.
- **2026-10-12** - M3b.2: export copies a data file as it is stored. `file.export` (and `convert` in the
  CLI) of a Unicode `.pst` now writes the new file node for node, the way `file.save` does,
  instead of going through the model: hidden (associated) items, a folder's own properties,
  the ids that say which folder is the Calendar, and properties this program does not know
  are in the copy (`"copied": true`). Unsaved changes are in the copy too, and the source
  stays as it is, still unsaved, with its journal. The copy keeps the id of the data file it
  was made from, as a copy of the file itself would. Made better on the way: a source with
  damage that cannot be copied so, or whose copy does not pass the structure check, is
  exported the old way, as far as it reads, and the status line and the result say that
  hidden items are then not in it (pinned by a test that damages a file in one place after
  another); the copy is also the file made small again (free space is not carried). The
  sample mailbox, an `.ost`, an ANSI and a 4K-page file still go through the model.
- **2026-10-12** - M3b.3: the panes are sized by dragging, and the window stands where it stood. The edge
  between the folder pane and the list, and between the list and the reading pane (beside it
  or under it), is taken hold of and dragged; the sizes go through a command (`ui.layout`),
  are held to what leaves the other panes room, and are kept in the settings with the
  window's size, its place and whether it was maximized. The next run opens the window there.
  A window is never put where no screen is: a settings file is believed only between the
  smallest window and the largest screen, the first frames see that the title bar can be
  reached on the screen as it is now and bring the window to the corner if not (tried on the
  real window: a place of 9000 across came back at 40, 40), and a place on a second screen
  is not kept (egui does not say where the other screens are; the size is). Made better on
  the way: a double click on an edge gives that pane the size it started with, and
  `ui.layout {"reset": true}` brings the panes and the window back to where a first run has
  them; the settings file is written at most once a second while a window is dragged. The
  list has no columns to keep yet (it shows each message on three lines); that waits for
  the views of M5.
- **2026-10-12** - M3b.4: right-to-left text outside the body. Every single line the window draws from a
  message or a data file (folder names, the sender, subject and preview in the list, the
  header of the reading pane, the title, the chips of attachments) goes through one place,
  and that place now puts a line with Hebrew or Arabic in it in the order it is read
  (UAX #9), its direction that of its first letter, the same way a paragraph of a body is
  turned. Made better on the way: a line too long for its room loses the end of the text as
  it is read, with the ellipsis there, and not whatever happens to stand at the right; and
  the marks that turn the text after them round (embeddings, overrides, isolates) are left
  out of such lines, so an attachment called `photo<mark>gpj.exe` is shown with the ending it
  has and not as `photoexe.jpg` (found by the test written for it: the marks were honoured
  at first). Looked at in a headless capture with our own Hebrew and Arabic lines. Not yet:
  the compose box and the search box (egui's own text editor draws them as stored), and a
  right-to-left line is still set from the left of its cell.
- **2026-10-12** - M3b.5: the text print sets itself is set as the body is. What stands at the head and
  the foot of a page and the marker before an item of a list were set glyph for character,
  from the left: they now go the way a paragraph's text goes, so letters the font draws as
  one glyph are, pairs stand as close as the font says, Arabic letters join, and a part with
  Hebrew or Arabic in it is drawn in the order it is read and copied out of the PDF in that
  order. (The field names and values of the header already were: they are paragraphs in a
  table.) Made better on the way: a part too long for its share of the line loses the end
  of what it says as that is read, whichever way it is drawn; a marker too long for the
  room before its item is cut the same way and no longer hangs off the sheet (found by the
  junk test: a twelve-letter marker stood fifteen points left of the page); and a body that
  filled what a print may hold still has its pages' heads and feet set. Looked at in a page
  rendered from our own text.
- **2026-10-12** - M3b.6: what a right-to-left paragraph moves to the right. A paragraph written from the
  right is now set in from the right by the room its list level and its quotation take; the
  marker of an item stands at the right of the item, outside its text; the bars of a
  quotation stand at the right. The same on screen and in print, pinned in print as the
  mirror of the same paragraph written from the left. Made better on the way: the marker is
  read the way its item is, so the stop after a number stands at its left (`.1`), and a
  marker at the right is held on the sheet as one at the left is. Looked at in the window
  and on a rendered page with our own Hebrew and Arabic lists. Not yet, and listed: tab
  leaders, a frame's own indent, text beside a picture, the columns of a right-to-left table,
  `<bdo>` and `unicode-bidi`.
- **2026-10-12** - M3b.7: the journal's loose ends. When the journal cannot be kept beside the data file
  (the folder is not to be written), it is kept in a folder of the program's own beside the
  settings (`journals/`, under the data file's name and a number made of the whole of where
  the file is), and found there when the file is opened again, also once the folder can be
  written again; what is cut off a damaged journal is kept beside the journal, wherever it
  is. A journal another running copy holds is never gone around that way. The window now
  has the journal forced to disk no more than a second after a change, where it was every
  64 changes only, so a power cut costs the last second and not up to 63 changes
  (`Session::settle`; the store has no clock of its own, which keeps it building for the
  web). The count of what a damaged journal lost is now exact where it can be: a record
  whose sum is wrong is counted with each that follows it, and a last record cut short is
  one; only when a length itself is wrong, so that records cannot be told apart any more,
  does the status line say "or more" (`lostMore` in what `file.open` returns). Pinned in
  `crates/pst/src/tests_journal.rs` and the engine's tests.
- **2026-10-12** - M3b.8: right-to-left text while it is typed. Every field of one line is now our own
  (`crates/ui-egui/src/line_edit.rs`): the search box, a folder's new name, the To, Cc, Bcc
  and Subject lines and the head and foot of a printed page. The toolkit's field drew text as
  it is stored, so Hebrew or Arabic mixed with other writing came out in the wrong order and
  the cursor stood at the wrong end of it. The line is now put in the order it is read as it
  is typed, Arabic letters joined; the cursor stands where the next letter will go on from;
  what is selected is marked where it is seen; a click lands on the letter under it; and the
  arrow keys go the way the eye does in a line read from the right. Made better on the way:
  a field come to with the keyboard has its text selected, to be typed over; what is typed
  or pasted is one line and brings no mark that turns text round, and one that is already in
  a name is shown as a dot and can be seen to be taken out; a paste is cut to what the field
  takes; typing is taken back a word at a time (Ctrl+Z, Ctrl+Y); and the lines above a message
  being written that say what is wrong with it, the names on its attachments and the words
  of the link question read the right way too (they were missed in M3b.4). Looked at in the
  window headlessly: `ui_shot` scripts can now click and type. Not yet, and listed: the body
  of a message being written is still the toolkit's box; it gets its own editor with the
  item windows (M5), where its text also stops being plain.
- **2026-10-12** - M3b.9: what was left of a paragraph written from the right. A tab now stands between what
  is on either side of it the way the paragraph is read (UAX #9, L1: a tab and the blanks
  before it take the paragraph's direction), so its stops are counted from the right and the
  dots or line that fill its room lie between the two texts, in print and on screen. A
  border set in from the side is set in from the side its paragraphs start at. The room a
  picture at the side takes is taken from the right side of the lines beside it. A table
  written from the right (`dir=rtl` on it or around it, `\rtlrow` in RTF) has its first
  column at the right, cells that cover several columns over the same ones, and stands at
  the right when the text around it is read from the right; its lines stay where they were.
  `<bdo>`, `<bdi>`, `dir` on a piece of a line and `unicode-bidi` (embed, isolate,
  bidi-override, isolate-override, plaintext) are carried as the marks Unicode has for them,
  opened again in every paragraph the element goes on in and closed where it ends, so the
  screen and the page read them the same way. Found and put right on the way: the leaders
  of tabs were never drawn on screen in a line with right-to-left text; the text of a cell
  in a table inside right-to-left text was put back at the left; the room beside a picture
  counted the paragraph's indent on the wrong side. Made better: the words of a link cannot
  be turned letter by letter (an override element or mark inside a link is dropped, and a
  link inside overridden text is set apart from it), so an address that is shown reads as
  it is written. Looked at headlessly, window and page. Not compared with Outlook.
- **2026-10-13** - M3b.10: copying text out of a printed PDF, tried in a reader. A message of our own with
  Latin ligatures, Hebrew, Arabic, brackets, numbers and accents was printed and read back
  with xpdf's `pdftotext` 4.06, the one reader on this machine that gives its text to a
  script. What it showed: a line of Hebrew or Arabic came out letter for letter backwards.
  The page said, around each piece of a line, how the piece is read (`/ActualText`); the
  reader took that and then turned it round as it turns every right-to-left run it meets.
  Now a piece written from the right is given the way every other PDF gives it: glyphs in
  the order they are seen, each saying the character whose shape it is (a bracket as it is
  drawn), and the reader does the turning. Only glyphs that are not one character each say
  what they stand for, each for itself and inside the text it belongs to: letters drawn as
  one glyph (fi, ffl, lam-alef), a letter drawn as several, a mark read with its letter.
  Made better on the way: a glyph the file already says is one character says so itself
  where it stands for another, so two characters that share a glyph are both copied right.
  Read back after the change: Hebrew and Arabic words, the ligatures, the accents and the
  brackets are copied as written, in the order read. Still wrong in that reader, and its
  own guess, not something the file can say: a full stop or a number in a line written from
  the right is put on the wrong side of the words, and a space beside a Hebrew word in a
  Latin line moves to its other side. Its `-raw` mode gives right-to-left text as it is
  seen, as it does for any PDF. No other reader was tried (none here gives its text to a
  script, and windows are not driven with keystrokes on this machine).
- **2026-10-13** - M3b.11: what is stored on an attachment is kept, and search folders show a changed
  message as it is now. Until now, adding, removing or moving one attachment of a stored
  message wrote every attachment again from the model, so anything stored on them that the
  model has no place for (a property under a name of another program's, a value of a kind
  not understood) was dropped from all of them. Now each attachment in the new list is
  matched with the one the file holds: first those that are the same but for their place,
  then those with the same contents and other properties. A matched attachment stays in
  the subnode it has, every stored property with it; only what the model says differently
  is written over, and its number in the list is set. Only an attachment that is truly new
  is written from the model, under an id past every one the file had. An attachment whose
  stored form cannot be opened is written from the model, as before. Made better on the
  way: a renamed attachment keeps what was stored on it too, and so does a message attached
  to a message, which stays in the file as it was stored and is not written again. Search
  folders: their tables were only cleared of deleted messages; the row of a changed message
  kept saying what the message said before. Now such a row is brought up to date the way
  the folder's own table is: what the message has now under the columns the table has,
  less what the message lost, and what only the table knows left alone. A table none of
  whose rows changed still goes across block for block. This is also the first test that a
  search folder's rows survive a save at all: our writer makes no search folder with rows,
  so the test writes one as another program would. Not done: the list of search folders a
  message belongs to is not worked out again (a message changed so that it no longer fits
  a search stays in that search's table until the program that made it looks again), and
  the third item of this task, a save that does not write the whole file, is M3b.12.
- **2026-10-13** - M3b.12: a save that does not hold the window. `file.save` takes `"wait": false`: the
  long part (writing the new file beside the old one, the structure check, reading it back
  and comparing) is done on a thread of its own, with its own handle on the data file and
  its own copy of the changes; the short part (the new file takes the old one's place, the
  journal is ended) is done by the caller when `Session::poll` finds the work finished, after
  looking once more that no other program changed the file meanwhile. Until then the data
  file is read as before and takes no changes: what is saved is what was shown when the
  save was asked for, and a change asked for meanwhile is refused with a reason, not lost.
  Selecting a message while a save runs shows it and does not mark it read. Closing the
  data file is refused until the save ends; leaving the program waits for it. The window
  saves this way (Ctrl+S and Save Changes on the File page); the command line and callers
  that say nothing wait, as before. The status bar shows the name of what is being saved,
  a bar that fills and a percentage, seen in a headless shot with a 240 MB mailbox written
  by our own exporter. Made better on the way: a save can be stopped (`file.stopSave`, and
  Stop in the status bar); the data file stays as it was, the changes stay unsaved and in
  the journal, and the status says it was stopped and not that it failed. The file written
  beside the data file is removed whether the save ends, is stopped, fails, or the store
  goes away first. A panic in the work becomes an error; no thread on the web build, where
  the save waits. Not done: every save still writes the whole file anew. Writing only what
  changed means writing into the file that holds the mail, which the rule that a failed
  write leaves the file byte for byte as it was forbids unless such writes are journalled
  themselves; that stays in Do later. Export still holds the window.
- **2026-10-13** - M3b.13: the structure check looks inside the nodes. Until now it stopped at blocks and
  nodes: checksums, maps, the two trees and the reference counts. A file could pass it and
  still hold a property list no other program can read. Now, in a file sound that far, every
  node that is to hold properties or a table (folders, messages, attachments, hidden items,
  the store, the name map, the tables of folders, attachments and recipients, and the same
  among the subnodes, to eight deep) is held against [MS-PST] 2.3: the heap's signature and
  kind, each block's page map (inside the block, after its header, allocations in order);
  an index's header, the sizes of its keys and entries, its levels, keys ascending; a
  property's value where its record says (an allocation that is there, or a subnode that is
  there), at the size its kind has, text of two bytes a letter with an even length, a list
  of values with its starts in order, an object naming a subnode that is there; a table's
  header at the size its columns need, the row laid out as four, two and one bytes and a
  bit for each column, columns in the order of their tags, each where a value of its size
  belongs and with a bit of its own, the row's id and version where they belong, the rows
  filling their allocation, the row index naming rows that are there, once each, with the
  id the row has, and each cell kept elsewhere being there. One line names the first thing
  wrong with a node. `Check::inside` has them; `openmailclient-cli inspect` lists them with
  the rest. What it found in our own writer on the first run: an item carrying a value
  typed as an object, with bytes that name no subnode, was written as it came; such a value
  is now left out (there is nothing it could be read from), and a value of a kind with a
  size of its own is written at that size whatever came. Made better on the way: a save is
  stopped by what it did wrong and not by what was wrong before. When the new file has
  something wrong inside a node, the file as it was is checked too, and only what is new
  stops the save: a file from another program with an oddity of its own is still changed
  and saved, its nodes going across as stored. 3 tests: 27 kinds of damage each named, some
  10,000 changed bytes and cut-short heaps without a panic, and a save refused for its own
  mistake and not for an old one. Not done: fill levels of a heap are not compared with the
  space that is free; the check covers Unicode files with 512-byte pages only, as before;
  no file written by Outlook of that kind is at hand to run it on (the profile here has
  `.ost` files with 4K pages), so what it says of such a file is not yet known.
- **2026-10-13** - M3b.14: an export that does not hold the window. `file.export` with `"wait": false`
  comes back at once with `{"started": true}` when the source is a Unicode `.pst` that can
  be copied as it is stored: the copy and the structure check run on a thread of their own,
  through a handle of their own on the data file, and write beside the target as before;
  `Session::poll` puts the new file in the target's place when the work is done, and
  `Session::finish_saves` waits for it before the window goes. The status bar says
  "Exporting to name.pst" with the bar, the percentage and Stop that a save has;
  `file.stopSave` stops exports as well as saves, and a stopped export leaves nothing where
  it was to go. File > Export in the window asks for it this way. Made better on the way:
  the mailbox is not locked while it is exported. A save holds the store until it is done,
  for the file under it is being replaced; an export takes the changes as they stand when
  it is asked for and reads the file itself, so mail is read, changed and deleted meanwhile
  and the new file holds the mailbox as it stood (a test deletes a message while the export
  runs and finds it in the exported file). Two exports to one file are refused; a data file
  is not closed while it is exported; a target opened here meanwhile is not written over; a
  session that goes while an export runs waits for it to stop and removes what it wrote.
  When the copy fails for a reason other than being stopped, the mailbox is written from
  what is read, as before, and that part holds the caller. 1 test. Not done: a mailbox that
  is in no data file, an `.ost`, an ANSI `.pst` and a 4K-page file are exported on the
  caller's thread still (they are written through the model, which is not to be shared
  between threads as it is); the bar was not looked at in a screenshot for an export, the
  drawing being the one a save uses with other words; the structure check at the end does
  not hear Stop, so a stop asked for in the last moments takes until the check is done.
- **2026-10-13** - M3b.15: the folders other programs find by id. A file made by `file.export` from
  a mailbox read through the model named only the Outbox, Sent Items and Deleted Items on
  its store. Now the first folder of each kind among Calendar, Contacts, Journal, Notes,
  Tasks and Drafts is named by an entry id on the root folder and on the Inbox
  ([MS-OXOSFLD] 2.2.3: 0x36D0 to 0x36D4 and 0x36D7), so a program that looks for the
  calendar by id and not by name finds it. Made better on the way: the reader takes the
  same ids from the root of any file it opens, so a Drafts folder is known as Drafts
  whatever it is called in the file's language (it was known by its English name alone;
  the other five were known by their class already). An id that names no folder in the
  file is passed over. 1 test. Not done: nothing names the Inbox in a data file (a server
  says which folder mail is delivered to), so an Inbox with another name is a mail folder
  like any other; written from the specification alone, as the rest of an exported file is
  until one is opened in Outlook.
- **2026-10-13** - M3b.16: three small things in the writer. (1) Values keep their kind. A 32-bit
  float was read as a 64-bit one, currency as a whole number, a time kept as days as a
  float, and an error code as a number, and each was written back as what it had been read
  as when an item was written from the model: another type, and for a float another size.
  The model now has a kind for each (`PropValue::Float32`, `Currency`, `FloatTime`,
  `Error`), so a `.pst`, a `.msg` and the journal of unsaved changes write them as they
  were. Made better on the way: an error code standing where a value could not be given is
  no longer taken for a number by what reads a property as one. (2) A file rewritten by a
  save goes on from the counters of the header it replaces ([MS-PST] 2.2.2.6): the next
  node id of each kind is never lower than it was, so the id of a deleted message is not
  given to a new one (a program holding the old id would have found another message
  under it), and the count of how often the header was written goes up by one where it was
  set back to one. Counters that make no sense are held to what a node id can be. (3)
  `file.keepOriginal` chooses whether the file as it was stays beside a data file on its
  first save; it does until told otherwise, and the choice is kept with the settings. 2
  tests, and more in three that were there. Not done: `file.keepOriginal` has no place on
  a settings page (there is none yet; M5); a file kept before is not removed when the
  choice is turned off.
- **2026-10-13** - M3b.17: attachments, the two cases left. (1) A file put in again with other
  contents. When a message's attachments changed, a stored attachment was known again by
  its contents alone, so a file that was worked on and attached again was written as a new
  one and lost every property this program has no place for. Now two more ways of knowing
  it follow the two there were: a file with other contents and the same properties, then a
  file with other contents under the same name and kind. Its new contents take the place
  of the old in the node it had (a plain file's bytes, or the object of another program in
  a subnode of its own, the old one let go), and what else is stored on it stays. A file
  with another name and other contents is a new file, as it should be; a message inside a
  message is known by what it holds alone. (2) Attachments that cannot be read. A message
  showed only those of its attachments this program could read, and saving a change to the
  list wrote only those: a damaged attachment, or the ones past the twenty thousand we
  read, were gone from the file though nobody had taken them away. They now stay as they
  are stored, after the others, with their rows; a message left with only such attachments
  still says it has attachments. Made better on the way: the property list of a changed
  node is written after the subnodes that stay with it are known, so a value that names
  one is checked against what is really there. 1 test (three files put in again, a file
  under another name, an attachment that cannot be read through two saves). Not done: an
  attachment that cannot be read is carried, not mended; its row keeps the place it is
  given after the readable ones and not the one it had among them.
- **2026-10-13** - M3b.18: three things left over in the data file's tables and its check. (1) A value
  of another type than its column's. A table's column has one type ([MS-PST] 2.3.4.2); a
  message that had the value as another type (a 16-bit number where the column is 32-bit,
  8-bit text where the column is Unicode) got no cell for it. Now the value goes in as the
  column's type when nothing is lost by that: whole numbers into a wider one or a narrower
  one with room, a 32-bit float as a 64-bit one and back when it comes back the same, text
  of 7-bit characters as the other kind of string. This holds for a changed message's row
  in its folder's table and in search folders' tables, for a message or folder that came
  from elsewhere, and for a mailbox written from the model. The item keeps the value as it
  was given in every case. Made better on the way: a value that cannot go in (a number too
  large, a kind that is no number) used to leave the cell saying what the message said
  before the change; the cell is now empty, so a list never shows what is no longer true.
  (2) The structure check hears Stop: `check_until` looks at the progress before each block
  and each node and gives `PstError::Stopped`; a save stopped while its new file is checked
  says it was stopped, and an export on a thread of its own stops in the check as it does in
  the copy. (3) Fill levels: the check reads the four bits a block that say how full each
  block of a heap is (the header's eight and each later map's 128, [MS-PST] 2.3.1.2 and
  2.3.1.4), and says when a block is said to be fuller than it is, to have more room than
  it has (unless it has given allocations up, which leave room its length does not show),
  or when a level is given to a block the heap does not have. A level may be one step off,
  for how another program counts the edge of a level is not known. Which half of a byte
  comes first was a guess; heaps of one block in Outlook-written files read locally have
  the level in the low half of the first byte, as we write it (counts only were looked at).
  3 tests. Not done: a heap of several blocks written by Outlook has not been seen, so the
  second block's half of the byte rests on the first; text beyond 7 bits is not converted
  between the kinds of string (it depends on a code page); the check is still not run on a
  512-byte-page `.pst` written by Outlook, there being none here.
- **2026-10-13** - M3b.19: an export that does not hold the window, for every kind of mailbox. A store
  gives itself apart (`Store::apart`): the mailbox as it stands when asked, to be had on
  another thread. A mailbox in memory hands over a copy of itself; a data file of any kind
  is opened there through a handle of its own, with the unsaved changes as they are, after
  a look that no other program changed the file meanwhile. `file.export` with
  `"wait": false` now starts for the sample mailbox, an `.ost`, an ANSI `.pst` and a
  4K-page file too, and the whole of the writing runs there: the copy of a Unicode `.pst`
  as it is stored, and the writing from what is read where that cannot be done.
  `write_store_until` says how far it is, an item a step, and stops when asked; the check
  after it hears the stop as well. This takes the place of `Store::copy_begin`, which could
  only copy. Made better on the way: when a copy as stored failed (a source with damage),
  the mailbox was written from what is read on the caller's thread, holding the window for
  as long as that took; that part runs on the export's thread now, so nothing of an export
  asked for this way holds the caller. A copy that was stopped is not tried the other way.
  1 test extended (the sample exported while a message is deleted, which the new file still
  has; an `.ost` exported and left byte for byte as it was; one stopped). Not done: an ANSI
  `.pst` and a 4K-page file were not exported this way in a test, there being no writer for
  them yet (they take the same path as the `.ost` in the test); a data file that was opened
  from bytes and is in no file is exported on the caller's thread.
- **2026-10-13** - M3b.20: the structure check for the other two kinds of data file. It knew Unicode files
  with 512-byte pages and said of the others that it could not look. It is now given the
  layout of the kind it is looking at (`Geom` in `crates/pst/src/check.rs`): the size of a
  page and of its trailer, where a map's bits are, how much one bit stands for, where the
  first map is, where a tree's page counts its entries, a block's trailer and its largest
  size, and whether an id is four bytes or eight. An ANSI file is held against [MS-PST]:
  the header with its one checksum and its root at 164, trailers of twelve bytes with the id
  before the checksum, maps with four bytes of padding before their bits, tree entries of
  twelve and sixteen bytes, blocks of up to 8180 bytes, subnode blocks whose entries begin
  after four bytes, and inside the nodes a table's row index with places of two bytes. A
  file with 4K pages (wVer 36) is in no specification; what is held against it was measured
  on the local `.ost` copies, structure only: pages of 4096 bytes with a trailer of 24 whose
  first sixteen are a Unicode trailer, entry counts of two bytes at 4056, the first
  allocation map at 0x22000 with a bit for 512 bytes and one section of 4072 * 8 * 512
  bytes, no page map or free maps, blocks of up to 64 KB less a trailer of 24, a checksum
  of the bytes as stored, and a block entry with the inflated size before the reference
  count. The three local files pass with nothing found, inside the nodes too (5 to 8 pages,
  99 to 313 blocks, 158 to 272 nodes; counts only, the files stay local). Made better on
  the way: a block stored deflated is inflated by the check and has to come to the size its
  entry gives, which the reader does not ask; and `inspect --check` of the CLI now says
  what it finds for every kind of data file we read. 3 tests: an ANSI file and a 4K-page
  file put together byte by byte in the test (not by our writer, and not from any real
  file), each sound and then with a block, the free count, a map and a heap's fill level
  wrong; both cut short and smashed 400 times without a panic. This is also the first
  4K-page fixture in the repository (gaps item 8), small as it is. Not done: a 4K-page
  file's heap fill levels (their steps for 64 KB blocks are not known) and the eight bytes
  after its trailers are not held against anything; a 4K-page file with a second section
  was not seen, so whether one has more map pages is not known; where an ANSI file has its
  free maps is worked out as for a Unicode file; no Outlook-written ANSI file was tried.
- **2026-10-13** - M3b.21: ANSI data files are written. The writer of the lower layer is given the kind
  of file it writes (`NdbWriter::of` in `crates/pst/src/write.rs`) and lays an ANSI file
  out as [MS-PST] has it: the header with wVer 14, its root at 164 and its one checksum;
  page trailers of twelve bytes; map pages with four bytes before their bits; tree entries
  of twelve and sixteen bytes; blocks of up to 8180 bytes with a trailer of twelve; ids of
  four bytes in the blocks that name blocks and subnodes; a table's row index with places
  of two bytes. A file past 2 GB is refused with a reason, not cut. An ANSI `.pst` that is
  open now takes changes and is saved as the kind it is (`file.save`): what is copied block
  for block has blocks of that kind's size, so it could be nothing else. `file.export`
  copies an ANSI `.pst` as it is stored, hidden items and unknown properties with it, and
  takes `"kind": "ansi"` or `"unicode"` to write the other kind from what is read. Made
  better on the way: text is never replaced by a question mark. It is written as 8-bit
  text in the code page the reader will use for that place (a message's own, its
  recipients and attachments by the message's, an embedded message by its own, folders
  and tables by Windows-1252) only when those bytes read back as the same text
  (`codepage::encode`), and is left as Unicode text when they would not; and a table now
  has one kind of text in a column in any file we write, where a value of the other kind
  was before written under a column of its own. 8 tests: a mailbox written as an ANSI file
  is the same mailbox as its Unicode twin, to the last property; text goes by the code
  page of its message (Cyrillic under 1251, a name 1251 has not, an embedded message
  under 1252); changes of every kind kept over three saves, each an ANSI file; an ANSI
  file on disk saved in place and copied; a file with more blocks than one block names
  and more subnodes than one block holds; cut short and smashed 150 times without a panic;
  the code pages encode only what comes back the same; the export by kind, waited for and
  not. Every file written in them passes the structure check of M3b.20. Not done: no
  ANSI file written by Outlook was tried, and none of ours was opened in Outlook; what a
  program that knows only 8-bit text makes of Unicode text in an ANSI file is not known;
  wVer is always 14 (a file that had 15 is saved with 14); a 4K-page file is not written
  (its layout is in no specification), so it stays read-only and its export goes through
  the model, as an `.ost` does.
- **2026-10-13** - M3b.22: Table Style prints what is asked of it, and the print window has a zoom. The
  columns of the table are chosen (`"columns"` of `file.pageSetup`, `file.print` and
  `file.printPreview`: who, from, to, subject, date, size, notes, categories, importance,
  flag, attachment, preview; `print::Field`), a heading stands over each group of the list
  as it is arranged, across every column (`"groups"`; `Row::heading` in
  `crates/layout/src/list.rs`), and `"rows": "selected"` prints the selected message's row
  alone. The print window has these beside the style, and shows the page at 25 to 400
  hundredths of its size on paper: steps, Ctrl and the wheel or a pinch over the page, and
  "Whole page"; a page larger than its room is moved about under it (`ui.printPreview` with
  `"zoom"`). Made better on the way: a heading says how many items its group has and how
  many are unread; the columns and the headings are kept with the settings, which rows is
  of the moment; the page says which message of how many it is when one is printed. Found
  and fixed: a page drawn larger than the graphics card's largest texture stopped the
  window, so a page is never drawn wider or taller than that. 2 tests: the columns, the
  headings in two arrangements, the selected row, twelve columns at once and what is
  refused; a heading across every column with its fill; the print window's test goes
  through the sizes, the refused ones and the changes to the table. Not done: one message
  is selected at a time, so "selected" is one row until there is a selection of several;
  the window's list has no columns of its own to follow, so the columns are the print's;
  a narrow column may break a word in two; Outlook's Table Style was not observed.
- **2026-10-13** - M3b.23: more kinds of picture, and pictures that move. WebP and TIFF files are read by
  the decoders the other kinds are (the `image` crate, under the same limits). An SVG drawing
  is drawn to pixels by `resvg` (Apache-2.0 OR MIT), in `crates/engine/src/svg.rs`: nothing
  in a drawing is run, since there is no script engine; no picture a drawing refers to is
  read, from a file, an address or its own text; its words are set in the fonts installed
  here, from the one list the program keeps (`Library::shared`). The reader is not ours, so a
  drawing is drawn on a thread of its own with room for the deepest nesting the reader takes,
  is given up after three seconds, and no other is begun while two that were given up still
  run. An animated GIF, PNG or WebP is played in the reading pane (`pictures::film`): every
  frame is the whole picture, shown as long as the file says and as many times; on paper and
  in a PDF it is its first frame. Made better on the way: a drawing has no pixels of its
  own, so it is kept at twice its size and is sharp on a dense screen and on paper; a
  photograph that says which way up it was taken (Exif orientation) is shown that way, in
  every kind that says so; a frame that asks for no time is given a tenth of a second
  and is not a blur; `view.animations` stops pictures moving and starts them, and is kept
  with the settings, for a reader who wants a still page. 7 tests: WebP and TIFF to the
  last pixel, cut short and bent at every byte, a TIFF that claims 60000 pixels a side;
  the photograph turned, and Exif that is not Exif; a drawing with its text, a box and no
  size, one a million wide, and pictures it refers to four ways, none read; hostile
  drawings (ten thousand million shapes by `use`, the same by XML entities, 6000 groups
  inside one another, a chain of 3000 uses, shapes that use themselves, numbers that are no
  sizes, a blur of 100000), cut short and bent; the frames and their times, a film cut at
  every byte, too many frames and too large a canvas shown still; the reel in the window
  through two playings to its last frame; the command and its setting. Not done: the web
  build does not draw SVG (it has one thread and a small stack; with the web app, M6); a
  bitmap inside a drawing is left out; a compressed drawing (`.svgz`) and one in UTF-16 are
  not read; an animated PNG or WebP was not tried, there being no writer for one here;
  a picture is decoded on the thread that draws the window, as before, so a drawing given up
  holds it for three seconds; what Outlook shows of any of these was not observed.
- **2026-10-13** - M3b.24: several pictures at the sides, and paper behind a dark picture on glass. A
  paragraph sets every picture that asks for a side at that side, one beside another, while
  room for text is left between them (eight of its own at most, sixteen standing at once);
  one for which no room is left stands in the text as before. A picture set while another
  still stands is set beside it. The text is as wide as the pictures leave it line by line:
  it widens where each one ends. This is so in the reading pane (`para` in
  `crates/ui-egui/src/body.rs`, which sets the text in a piece for each height at which a
  picture ends) and on the printed page (`Flow::asides` in `crates/layout/src/flow.rs`).
  In the dark reading pane a picture that is seen through in places and is dark where it is
  seen (a logo drawn for a white page) has light paper laid behind it, so that it can be
  read. Made better on the way: the paper is laid only where it helps. A light picture on
  glass was drawn for a dark page and is left as it is; a picture that covers its room needs
  none; and where the message chose what is behind (a filled cell or block) that choice is
  kept. A photograph with a few soft pixels at its corners is not taken for a drawing on
  glass. 3 tests: in the window, heights with one, two and three pictures at the sides, two
  at one side, pictures of different heights, one set while another stands, one with no room,
  inside a border, and sixty to a paragraph at widths from nothing to a million; how light a
  picture is, and the paper drawn in each of the cases above and in no other; on paper, where
  every line starts and ends beside two pictures of different heights, two at one side from
  the left and from the right, one set while another stands, one with no room, and forty to
  a paragraph in sixty paragraphs and in cells. Looked at in the window in both themes
  (`plan/shots/asides.jsonl`). Not done: on paper a table or a bordered block still starts
  under a picture at the side, where in the window it stands beside it; the paper is not
  laid behind a dark picture in a cell the message filled dark; what Outlook does with
  several pictures at the sides, or with see-through pictures in its dark mode, was not
  observed.
- **2026-10-13** - M3b.25: boxes inside one another, tables in blocks, a colour behind the body. A
  block with a border, a fill or room of its own that is written inside another is a box
  inside that box (`Frame::within` in `crates/formats/src/rich.rs`), eight deep at most
  (`MAX_BOXES`); before, the inner one took the outer one's place. A table written inside a
  filled or bordered block stands inside it (`Table::inside`), where before the block ended
  above the table and began again under it. A colour on `<body>` or `<html>` is the colour
  behind the whole message (`Doc::fill`, CSS 2.1 section 14.2): in the reading pane it fills
  the pane from edge to edge and text with no colour of its own is drawn to be read on it; on
  paper it lies behind the body on every page the body is on, under every other fill. Both
  drawers walk the boxes level by level with `rich::boxed_run`: `framed` in
  `crates/ui-egui/src/body.rs` and in `crates/layout/src/flow.rs`, where `Flow::lift` keeps
  each fill over the one it stands in. Made better on the way: a box keeps only its own fill,
  so a box with none inside a filled one no longer paints the outer colour a second time;
  white is kept as a colour on a coloured body as it is in a coloured block; what a layout
  table of one column held keeps the box the table stood in; and what a box holds never
  stands outside it, however much room boxes inside one another ask for (found by the
  hostile test on small paper: text was set past the right margin). 4 tests: in the reader,
  three levels of boxes and which paragraph stands in which, the colour nearest behind, the
  body colour from `<body>` and `<html>` and not from inside a table, five thousand blocks
  inside one another, and a chain of ten thousand made by hand; in the window, the heights of
  one box, two, and three paragraphs in two, a table in a filled block, three hundred deep at
  widths from nothing to a million, and the ink of text on a dark body in both themes; on
  paper, the order and the place of four fills inside one another, what text stands in
  which, the body colour on every page, and forty boxes deep with tables in cells on small
  paper. Looked at in the window in both themes (`plan/shots/boxes.jsonl`,
  `plan/shots/boxes-body.jsonl`). Not done: borders on text within a line and rounded
  corners (M3b.26); a word wider than a very narrow column still stands out of its cell; a
  body colour is not drawn behind the message header; what Outlook does with a body colour
  in its dark mode was not observed.
- **2026-10-13** - M3b.26: borders on text within a line; rounded corners. A border on a `<span>`,
  `<code>` or any other element within a line is kept on its text (`Style::edge` in
  `crates/formats/src/rich.rs`, CSS 2.1 section 8.5) where all four sides are the same line
  (`rich::one_edge`); a line on one side only is not drawn as a box. In the window each
  stretch of a line with the same border has its box (`body::outlines` in
  `crates/ui-egui/src/body.rs`), and on paper each piece has a line over and under it
  (`Look::edge` in `crates/layout/src/flow.rs`); a box is open at the side where its text
  goes on in the next line, and a border that names no colour has the colour of its text.
  `border-radius` rounds a block and a table cell (`Frame::radius`, `Cell::radius`, CSS
  Backgrounds and Borders 3 section 5.1): the first length is taken for all four corners,
  64 pixels at most (`MAX_RADIUS`) and never more than half the shorter side, so a very
  large radius makes a pill. On paper a rounded fill is a `Fill` with a `radius`, and a
  rounded border is a `Fill` with a `ring`: a band that follows the corners, in place of
  four straight lines. The rasteriser fills it between two outlines by the even-odd rule
  (`rounded` in `crates/render/src/lib.rs`) and the PDF writer writes four curves and `f*`
  (`Writer::fill_round` in `crates/pdf/src/lib.rs`, PDF 32000-1 section 8.5). Made better on
  the way: a fill with no border is rounded too; a cell that has only the table's own lines
  is rounded with them; and what cannot be rounded honestly keeps square corners with all
  of its lines, where sides differ, where the line is dashed, dotted or double, and where a
  box on paper goes on over a page. 5 tests: in the reader, which text has which border,
  the radius of blocks and cells, junk lengths and three thousand spans inside one another;
  in the window, how far a corner is rounded, the box round two letters where they are
  drawn, boxes open where text goes on over lines, heights unchanged by borders and
  corners, and any radius at widths from nothing to a million; on paper, the four lines
  round a word and where they stand, the colour of the text for a border with none, two
  sides only for a border over many lines, the fill and the band of a rounded box and their
  order, square corners for sides that differ and for boxes over pages, and any radius on
  three sizes of paper; in the rasteriser, the pixels in, on and outside the corners of a
  fill and a band at three scales, and a hundred junk pairs; in the PDF writer, the curves
  as written and junk numbers. Looked at in the window in both themes and on a printed
  page (`plan/shots/round.jsonl`, `plan/shots/round-print.jsonl`). Not done: text drawn in
  another order than it is stored (right-to-left) has no border round it in the window;
  each corner having its own radius, and radii that are a share of the box; padding of an
  element within a line; what Outlook, which draws HTML with Word, shows of either was not
  observed.
- **2026-10-13** - M3b.27: RTF highlight colours and list overrides. A highlight (`\highlightN`) is a
  place in the colour table (RTF 1.9.1, "Highlighting") and is shown as one of sixteen
  marker colours (`MARKERS` and `highlighter` in `crates/formats/src/rtf.rs`; ECMA-376 Part
  1, 17.18.40 names them): itself when it is one of them, a grey by how light it is when it
  has nearly no colour, else the pure colour of its hue, bright or dark. The one case seen
  in Outlook, a pale yellow shown as bright yellow in our own receipt message, is now the
  same; a colour behind text that is not a highlight (`\cb`, `\chcbpat`) is kept as
  written. A number that list paragraphs name may change levels of its list
  (`\lfolevel`; RTF 1.9.1, "List Override Table"): start one somewhere else
  (`\listoverridestartat`) or write it another way (`\listoverrideformat` with a level of
  its own). `Builder::override_word` reads them and `counted` follows them. Made better on
  the way: one list now counts on through every number that names it, where before each
  number counted from the start by itself, and a number that starts a level somewhere
  begins there again each time it takes over from another. 1 test: seven highlights from
  one colour table and what each is shown as, colours behind text left alone, places that
  are not in the table; thirteen paragraphs over four numbers of one list, with a start
  changed, a level written anew with and without a start of its own, and a level below
  counted under it; and fifteen hundred overrides of twelve levels each with numbers at the
  edge and words outside any level. Not done: what Outlook shows for highlights other than
  the pale yellow was not observed, so the choice by hue is ours; `\levelrestart`,
  `\levellegal` and levels that do not restart under the one above.
