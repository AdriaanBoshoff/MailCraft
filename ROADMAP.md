# MailCraft roadmap

**Status:** M1 (data files first) in progress · revision 1 · 2026-10-11

## Headline numbers

| Measure | Now |
|---|---|
| Catalogued Outlook Classic features with a command | 38 of 114 (33%), see [docs/parity-checklist.md](docs/parity-checklist.md) |
| Tests | 50, all passing |
| File formats read | `.pst`/`.ost` (ANSI, Unicode, 4K pages), `.msg`, `.oft`, `.eml` |
| File formats written | `.msg`, `.oft`, `.eml` |
| Protocols | none |

## Milestones

| | Milestone | State |
|---|---|---|
| M1 | **Data files first**: open `.pst`/`.ost`/`.msg`/`.eml` in an Outlook-Classic-style window; save messages Outlook can reopen | in progress |
| M2 | Formatted reading: HTML and RTF bodies laid out and drawn, inline pictures, printing | not started |
| M3 | Writing `.pst`: journalled, crash-safe writes; rules and folder changes persist | not started |
| M4 | Accounts: IMAP, POP3 and SMTP behind an account trait, with an in-process fake server for tests | not started |
| M5 | Calendar, People and Tasks modules | not started |
| M6 | MCP server, control channel, web build | not started |

## Current focus (M1)

1. Check the `.pst` reader against more real Outlook-written files, locally (results are never
   published). Done so far: a few small 4K-page `.ost` files.
2. Have Outlook Classic open MailCraft-written `.msg` files and compare, property by property.
3. Side-by-side layout comparison with Outlook Classic and the interaction parity doc.

The ranked work list is [docs/gaps.md](docs/gaps.md).

## Progress log

- **2026-10-11** — M1.1: item model, MIME, `.msg` and `.pst` readers, engine with 48 commands, CLI.
  M1.2: the egui window (ribbon, folder pane, message list, reading pane, inline compose, File page),
  headless screenshots, the desktop app, and `cargo xtask ci`.
- **2026-10-11** — M1.3: `.ost` files are accepted (client magic "SO"); 4K-page reading confirmed
  against real offline stores, locally.
