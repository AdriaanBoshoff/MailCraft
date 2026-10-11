# MailCraft

A clean-room, open-source mail client written in Rust, aiming at parity with Microsoft Outlook Classic
(the Win32 desktop Outlook): the same layout and behaviour, files that go both ways, and every feature
reachable as a command from the UI, a CLI and (later) an MCP server.

MailCraft is an independent concept project. It is not affiliated with Microsoft. It follows the
conventions of [`storytold/wordcraft`](https://github.com/storytold/wordcraft).

**Status: early.** See [ROADMAP.md](ROADMAP.md) for what works and [docs/gaps.md](docs/gaps.md) for what
does not.

## What works today

- Opens Outlook data files (`.pst`, `.ost`) read-only, and single messages (`.msg`, `.oft`, `.eml`).
- An Outlook-Classic-style window: ribbon, folder pane, message list with date groups, reading pane.
- Read, flag, categorise, move, delete, archive, search, sort and filter in writable (in-memory) stores.
- Reply, reply all, forward and new mail as drafts; saving messages as `.msg`, `.oft` or `.eml`.
- A sample mailbox with invented content, for trying things without any real mail.

## What does not work yet

- **Sending and receiving.** There are no accounts. "Send" files the message in Outbox and stops.
- **Writing `.pst` files.** Data files are read-only; changes to them are refused.
- **Formatted bodies.** HTML mail is shown as plain text; RTF-only bodies are not decoded.
- Calendar, People and Tasks.

## Run it

```bash
cargo run --release -p mailcraft -- --sample
```

```bash
cargo run --release -p mailcraft -- path/to/archive.pst
```

Without a window:

```bash
cargo run --release -p mailcraft-cli -- inspect path/to/archive.pst --check
```

```bash
cargo run --release -p mailcraft-cli -- convert message.msg message.eml
```

A screenshot of the real UI without opening a window:

```bash
cargo run --release -p mailcraft-ui-egui --example ui_shot
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
