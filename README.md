# OpenMailClient

![stage](https://img.shields.io/badge/stage-early%20%C2%B7%20M1%20%2B%20M2%20in%20progress-orange)
![parity](https://img.shields.io/badge/Outlook%20Classic%20parity-42%20of%20116%20%C2%B7%2036%25-yellow)
![tests](https://img.shields.io/badge/tests-193%20passing-brightgreen)
![unsafe](https://img.shields.io/badge/unsafe-forbidden-success)
![rust](https://img.shields.io/badge/rust-1.90%2B%20%C2%B7%20edition%202024-b7410e)
![licence](https://img.shields.io/badge/licence-MIT%20OR%20Apache--2.0-blue)

A clean-room, open-source mail client written in Rust, aiming at parity with Microsoft Outlook Classic
(the Win32 desktop Outlook): the same layout and behaviour, files that go both ways, and every feature
reachable as a command from the UI, a CLI and (later) an MCP server.

OpenMailClient is an independent concept project. It is not affiliated with Microsoft. It follows the
conventions of [`storytold/wordcraft`](https://github.com/storytold/wordcraft).

![The OpenMailClient window with the sample mailbox: folder pane, message list, and an RTF receipt with tables, a bordered note, an attachment card and a picture in the reading pane](docs/images/openmailclient-reading-pane.png)

*The sample mailbox (invented content), drawn by `ui_shot`. The receipt is stored only as RTF.*

## Progress

The numbers are measured, not estimated: [ROADMAP.md](ROADMAP.md) has the headline numbers and the
progress log, [docs/parity-checklist.md](docs/parity-checklist.md) is generated from the feature
catalog, and [docs/gaps.md](docs/gaps.md) ranks what is missing.

### Milestones

| | Milestone | State |
|---|---|---|
| M1 | **Data files first**: open `.pst`/`.ost`/`.msg`/`.eml` in an Outlook-Classic-style window; save messages Outlook can reopen | ![in progress](https://img.shields.io/badge/-in%20progress-orange) |
| M2 | **Formatted reading**: HTML and RTF bodies laid out and drawn, pictures, printing | ![in progress](https://img.shields.io/badge/-in%20progress-orange) |
| M3 | **Writing `.pst`**: journalled, crash-safe writes | ![not started](https://img.shields.io/badge/-not%20started-lightgrey) |
| M4 | **Accounts**: IMAP, POP3 and SMTP | ![not started](https://img.shields.io/badge/-not%20started-lightgrey) |
| M5 | **Calendar, People and Tasks** | ![not started](https://img.shields.io/badge/-not%20started-lightgrey) |
| M6 | **MCP server, control channel, web build** | ![not started](https://img.shields.io/badge/-not%20started-lightgrey) |

### Outlook Classic features with a command

| Area | Done | | |
|---|---|---|---|
| Mail | 32 of 67 | `██████████░░░░░░░░░░` | 48% |
| File | 6 of 13 | `█████████░░░░░░░░░░░` | 46% |
| Message window | 4 of 17 | `█████░░░░░░░░░░░░░░░` | 24% |
| Calendar | 0 of 9 | `░░░░░░░░░░░░░░░░░░░░` | 0% |
| People | 0 of 5 | `░░░░░░░░░░░░░░░░░░░░` | 0% |
| Tasks | 0 of 5 | `░░░░░░░░░░░░░░░░░░░░` | 0% |
| **All** | **42 of 116** | `███████░░░░░░░░░░░░░` | **36%** |

### File formats and protocols

| | Read | Written |
|---|---|---|
| `.pst`, `.ost` (ANSI, Unicode, 4K pages) | yes | not yet (M3) |
| `.msg`, `.oft` | yes | yes |
| `.eml` | yes | yes |
| `.pdf` | | yes, a message printed in Memo Style |
| IMAP, POP3, SMTP | not yet (M4) | not yet (M4) |

## What works today

- Opens Outlook data files (`.pst`, `.ost`) read-only, and single messages (`.msg`, `.oft`, `.eml`).
- An Outlook-Classic-style window: ribbon (simplified and classic), folder pane, message list with
  date groups, reading pane, light and dark themes.
- Read, flag, categorise, move, delete, archive, search, sort and filter in writable (in-memory) stores.
- Reply, reply all, forward and new mail as drafts; saving messages as `.msg`, `.oft` or `.eml`.
- **Formatted bodies.** HTML and RTF mail is laid out and drawn: fonts the message names (installed
  ones only), colours, lists, links, tables (cell borders, fills, merged cells, tables inside cells),
  paragraph borders and spacing, line spacing, tab stops. Nothing in a message is run and nothing is fetched.
- **Pictures.** Attached and written-in pictures (PNG, JPEG, bitmaps, Windows and enhanced metafiles
  that are drawings) are decoded in-process and drawn in the text; an attachment that stands in the
  text is a card that a click saves.
- **Printing.** A message prints to a PDF file in Memo Style, with its tables, borders and pictures.
- A sample mailbox with invented content, for trying things without any real mail.

## What does not work yet

- **Sending and receiving.** There are no accounts. "Send" files the message in Outbox and stops.
- **Writing `.pst` files.** Data files are read-only; changes to them are refused.
- **Parts of formatted bodies.** Metafiles with a
  bitmap beside the drawing; shaping and right-to-left text in print.
- Calendar, People and Tasks.
- The MCP server, the control channel and the web build.

## Run it

```bash
cargo run --release -p openmailclient -- --sample
```

```bash
cargo run --release -p openmailclient -- path/to/archive.pst
```

Without a window:

```bash
cargo run --release -p openmailclient-cli -- inspect path/to/archive.pst --check
```

```bash
cargo run --release -p openmailclient-cli -- convert message.msg message.eml
```

A screenshot of the real UI without opening a window:

```bash
cargo run --release -p openmailclient-ui-egui --example ui_shot
```

## Safety rules the code follows

- **Never crash, never lose mail.** No panics in non-test code; every reader has a hostile-input test;
  data files are opened read-only and saved files are written to a temporary file and swapped in.
- **Mail is hostile and private.** Message content is data: nothing in a message is run, and remote
  content is never fetched. Attachments are only ever saved to a file you choose.
- **Clean room.** Formats come from public specifications ([MS-PST], [MS-OXMSG], RFC 5322, …). No
  Microsoft code, icons, fonts or wording; icons are drawn in code.

Details: [CLAUDE.md](CLAUDE.md).

## Development

```bash
cargo xtask ci
```

runs formatting, clippy with warnings denied, the tests, and the asset, layering, parity and wasm checks.

## Licence

MIT or Apache-2.0, at your option.
