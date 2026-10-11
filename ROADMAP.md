# MailCraft roadmap

**Status:** M1 (data files first) and M2 (formatted reading) in progress · revision 18 · 2026-10-11

## Headline numbers

| Measure | Now |
|---|---|
| Catalogued Outlook Classic features with a command | 39 of 114 (34%), see [docs/parity-checklist.md](docs/parity-checklist.md) |
| Tests | 136, all passing |
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
13. Next: a pass over what is already built, to make each feature better than the one it
    follows; then metafiles that are drawings.

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
