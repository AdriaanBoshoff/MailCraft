# UI parity

**Status:** revision 1 · 2026-10-11 · first side-by-side look; nothing below is matched yet unless it says so.

How MailCraft's window compares with Outlook Classic's, from window captures of both at the same size.
Outlook captures hold private mail, stay on the dev machine and are never committed; this page records
only layout and measurements. Numbers are logical pixels at 96 dpi, from a 1440 × 753 window.

## What was compared

Outlook Classic 16.0, Mail module, as configured on the dev machine: **simplified (single-line) ribbon**
and a **dark grey theme**. Both are user settings; the classic three-row ribbon and the light themes are
equally Outlook Classic. MailCraft has only a classic-style ribbon in a light theme today.

## Measurements

| Part | Outlook Classic (observed) | MailCraft today | State |
|---|---|---|---|
| Title bar | about 58 high; search box 304 × 32, left edge on the message list's left edge | 34 high; search box centred, 518 wide | differs |
| Tab row | about 24 high: File, Home, Send / Receive, View, Help, then a "tell me" box | 28 high: File, Home, Send / Receive, Folder, View, Help | differs (Folder tab; no tell-me box) |
| Ribbon, simplified | one rounded panel, 48 high, 8 below the tabs, inset from the window edge | not built | missing |
| Ribbon, classic | not observed yet | 94 high, labelled groups | unverified |
| Module strip | 64 wide at the far left, full height below the title bar; icons 48 apart: Mail, Calendar, People, Tasks, More | 46 wide, starts below the ribbon; four modules | differs |
| Folder pane | 235 wide (user-resizable); rows 24 apart; Favorites group first, then one group per account; collapse arrow top right | 216 wide; rows 26 apart; no Favorites | differs |
| Message list | 276 wide (user-resizable); header row with All / Unread, "By Date" and a direction arrow; group header 24 high; rows about 70 high with sender, subject, preview | 348 wide; same header parts; rows 64 high | close |
| Row details | attachment mark at the right of the first line; date at the right of the second | marks at the right of the first line; time at the right of the second | close |
| Reading pane, nothing open | envelope drawing with "select an item to read" centred | blank | missing |
| Status bar | 22 high: item count left; connection state, two view buttons and a zoom slider right | 22 high: item and unread counts left, store kind right | close |
| Theme | dark grey here | light only | dark theme missing |

## Behaviour seen

- Opening a folder selects the first row but **does not open it** in the reading pane; the pane shows the
  empty state until the user picks a message. MailCraft opens whatever is selected. To check further
  before changing: this may depend on Outlook's reading-pane options.

## Next

1. A dark theme through `theme::Tokens` and the reading pane's empty state.
2. The simplified ribbon as a second ribbon mode, with the module strip at full height.
3. Capture Outlook with the classic ribbon and a light theme (the owner's settings would have to be
   switched for that; ask first) to verify the ribbon MailCraft already draws.
