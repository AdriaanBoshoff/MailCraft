# MailCraft roadmap

**Status:** M1 (data files first) and M2 (formatted reading) in progress · revision 11 · 2026-10-11

## Headline numbers

| Measure | Now |
|---|---|
| Catalogued Outlook Classic features with a command | 38 of 114 (33%), see [docs/parity-checklist.md](docs/parity-checklist.md) |
| Tests | 107, all passing |
| File formats read | `.pst`/`.ost` (ANSI, Unicode, 4K pages), `.msg`, `.oft`, `.eml` |
| File formats written | `.msg`, `.oft`, `.eml` |
| Protocols | none |

## Milestones

| | Milestone | State |
|---|---|---|
| M1 | **Data files first**: open `.pst`/`.ost`/`.msg`/`.eml` in an Outlook-Classic-style window; save messages Outlook can reopen | in progress |
| M2 | Formatted reading: HTML and RTF bodies laid out and drawn, inline pictures, printing | in progress: HTML and RTF text, lists, tables, pictures and named fonts are drawn |
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
9. Next: printing; then pictures an RTF body holds as attached objects, and metafiles.

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
