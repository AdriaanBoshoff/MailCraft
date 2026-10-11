# UI parity

**Status:** revision 7 · 2026-10-11 · side-by-side looks at the Mail module in both ribbon layouts, and at five message bodies; layout only.

How MailCraft's window compares with Outlook Classic's, from window captures of both at the same size.
Outlook captures hold private mail, stay on the dev machine and are never committed; this page records
only layout and measurements. Numbers are logical pixels at 96 dpi, from a 1440 × 753 window.

## What was compared

Outlook Classic 16.0, Mail module, as configured on the dev machine: **simplified (single-line) ribbon**
and a **dark grey theme**. Both are user settings; the classic ribbon and the light themes are equally
Outlook Classic. The classic ribbon was captured once by switching the layout for one capture and
switching it back; the theme was left alone, so the light themes are still unobserved. MailCraft has
both ribbon layouts (`ui.ribbonMode`) and a light and a dark colour set (`ui.theme`); it starts with the
simplified ribbon in the light set. The colours are our own in both sets, not Outlook's.

The two ribbon layouts share one window shell: title bar, module strip, tab row and panes are the same,
and only the ribbon panel's height and contents change. MailCraft follows that.

## Measurements

| Part | Outlook Classic (observed) | MailCraft | State |
|---|---|---|---|
| Title bar | about 58 high; search box 304 × 32, left edge on the message list's left edge | 48 high; search box up to 304 × 30 on the list's left edge | close |
| Tab row | about 24 high: File, Home, Send / Receive, View, Help, then a "tell me" box; the classic layout adds a Folder tab after Send / Receive | 28 high: File, Home, Send / Receive, Folder, View, Help in both layouts | differs (Folder tab always shown; no tell-me box) |
| Ribbon, simplified | one rounded panel, 48 high, inset from the right window edge | one rounded panel, 48 high, inset 16 | close; no overflow menu when the window is narrow |
| Ribbon, classic | one rounded panel, about 98 high, inset from the right window edge like the simplified one; large buttons with a label of up to two lines under the icon, columns of up to three small buttons, a group name centred under each group, a collapse arrow bottom right; panes start about 6 below it | one rounded panel, 98 high, inset 16; the same button and group arrangement; panes 7 below | close; no collapse arrow, no Quick Steps gallery, fewer groups |
| Module strip | 64 wide at the far left, full height below the title bar; icons 48 apart: Mail, Calendar, People, Tasks, More | 64 wide, full height, 48 apart; four modules | close |
| Folder pane | 235 wide (user-resizable); rows 24 apart; Favorites group first, then one group per account; collapse arrow top right | 216 wide, fixed; rows 26 apart; no Favorites | differs |
| Message list | 276 wide (user-resizable); header row with All / Unread, "By Date" and a direction arrow; group header 24 high; rows about 70 high with sender, subject, preview | 348 wide, fixed; same header parts; rows 64 high | close |
| Row details | attachment mark at the right of the first line; date at the right of the second | marks at the right of the first line; time at the right of the second | close |
| Reading pane, nothing open | envelope drawing with a one-line hint, centred | the same arrangement, our own drawing | close |
| Reading pane, message open | formatted body on a page in the theme's colour | formatted body laid out as Outlook lays out the same message: see Message bodies below | close |
| Status bar | 22 high: item count left; connection state, two view buttons and a zoom slider right | 22 high: item and unread counts left, store kind right | close |

## Message bodies

Two messages from MailCraft's own sample mailbox and three probe messages (all synthetic, written by
us) were saved as `.msg` and opened in an Outlook Classic message window, one at a time, then closed
without saving. The window was compared with MailCraft's reading pane showing the same message.
Nothing from the owner's mailbox was involved.

| What | Outlook Classic (observed) | MailCraft | State |
|---|---|---|---|
| Unstyled HTML text | 12 pt | 12 pt (16 px); relative sizes and headings start from it | same |
| `<table width=600 align=center>` | a column 600 wide, centred in the window | the same; also `max-width`, `width` in a style, and `margin: auto` for centring | same |
| Table without a width | as wide as its content | as wide as its content; cell `width` attributes are kept | same |
| `bgcolor` or a background on a cell | fills the whole cell | fills the whole cell; text on it is kept readable against that colour | same |
| A one-column table with a fill, a width or a place | drawn as a block of that shape | drawn; a one-column table with none of them still reads as ordinary blocks | same |
| Plain `<blockquote>` | set in about 40 to 48 on the left, no bar | set in 40, no bar | close |
| Quoted earlier mail (`type=cite`, a left border, a quote class) | a bar on the left | a bar on the left | close |
| Picture sent with the message (`cid:`) | drawn at the size the body asks for | the same | same |
| Picture on a server, not loaded, with a size | an empty box of the size asked for, with a mark, a notice and the alternative text, standing in the line of text | an empty frame of that size with our own mark, a notice and the alternative text, on a row of its own | close |
| Picture on a server, not loaded, without a size | a small box with a mark only | a line of text with the alternative text | differs (ours says what the picture is) |
| Table, cell padding and spacing | 1 and 2 where the table sets none; `cellpadding` and `cellspacing` followed; with spacing the table's own border is seen round the cells' | the same | same |
| `border-collapse: collapse` | no room between cells | the same | same |
| `padding` on a cell | followed, above and below apart from left and right | the same; one value each way (the left and right, top and bottom are not told apart) | close |
| Background on a row (`bgcolor`, or a style) | fills the row's cells | the same | same |
| What a cell holds, in a taller cell | in the middle; `valign` and `vertical-align` on the cell or the row followed | the same | same |
| Table width as a percentage | that share of the window | that share of the room | same |
| Cell width as a percentage | that share of the table | the same, for tables that have a width | same |
| Style sheet (`<style>`) | rules for an element name, a class, an id, or a name with a class are followed; only the **first** class an element names counts; rules with several classes, with one element inside another, and inside `@media` are not | the same | same |
| Link colour | the link colour whatever the text around it has; a colour set on the link itself is kept | the same, with our own link colour | same |
| RTF table | cell widths from the row definition (15 twips to the pixel); only the borders the RTF names | the same widths; every cell boxed when any border is named | close |
| RTF left indent | 720 twips is 48 | 720 twips is 48 | same |
| RTF highlight | a bright marker colour from a fixed palette | the colour the RTF's colour table gives | differs |
| Message window type | an RTF-only `.msg` is shown as rich text only when it is marked as in sync (see [file-format-parity.md](file-format-parity.md)) | written so | same |

Not followed yet, in order of how much they show: text beside a picture (a picture or its frame
always stands on a row of its own); fonts by name; borders set in styles and RTF borders per cell
edge; the space Outlook leaves between a paragraph and a table; cells that span rows. They are in
[gaps.md](gaps.md), item 3.

## Behaviour seen

- Opening a folder selects the first row but **does not open it** in the reading pane; the pane shows the
  empty state until the user picks a message. MailCraft selects nothing when a folder opens. To check
  further before changing: this may depend on Outlook's reading-pane options.

## Next

1. Pane splitters (folder pane and message list widths are fixed today) and Favorites.
2. An overflow menu for the simplified ribbon, and the "tell me" command search.
3. A light theme has not been observed (the theme setting roams to the owner's other machines, so
   it was not switched; ask first).

## Revision history

- 7 · 2026-10-11 — three probe messages compared: picture frames, cell padding and spacing, row fills, vertical places, percentage widths, style sheets, link colour.
- 6 · 2026-10-11 — two message bodies compared with Outlook's display of the same `.msg`; Message bodies section added.
- 5 · 2026-10-11 — attached pictures drawn in the body; row updated.
- 4 · 2026-10-11 — classic ribbon observed; MailCraft's classic layout rebuilt on the shared shell.
- 3 · 2026-10-11 — formatted bodies drawn; row added.
- 2 · 2026-10-11 — simplified ribbon, full-height module strip and dark colour set built; table updated.
- 1 · 2026-10-11 — first comparison.
