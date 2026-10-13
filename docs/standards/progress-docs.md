# Progress documents

**Status:** revision 1 · 2026-10-13 · written for this repository; it replaces a link to a
standard kept elsewhere that readers of this repository could not open.

How this repository says where it stands, so that a person or an agent arriving cold can find
out what is built, what is not, and what comes next, without reading the code or the history.

## The documents

| Document | What it holds | State |
|---|---|---|
| [`ROADMAP.md`](../../ROADMAP.md) | The stage the project is at, headline numbers, the milestones and their state, the current focus of each open milestone as a numbered list ending in one "Next" item, the "Do later" table, and the progress log | kept |
| [`docs/gaps.md`](../gaps.md) | The ranked list of what is not built, what is built but unproven, and known shortcomings | kept |
| [`docs/parity-checklist.md`](../parity-checklist.md) | Outlook Classic's features per module and which have a command; written by `cargo xtask parity`, never by hand | kept |
| [`docs/file-format-parity.md`](../file-format-parity.md) | What is read, written and kept of each file format, and what was checked against Outlook | kept |
| [`docs/ui-parity.md`](../ui-parity.md) | How the window compares with Outlook Classic's: layout and measurements only | kept |
| `docs/interaction-parity.md` | Observed selection, keyboard and compose behaviour, pinned by tests | not written yet: comes with the M1 checks against Outlook |
| `docs/protocol-parity.md` | IMAP, POP3, SMTP coverage | not written yet: comes with M4 |
| `docs/target-app-parity.md`, `docs/architecture.md`, `docs/hardware-parity.md`, `docs/localization-parity.md` | The overall assessment, the crate layers, devices, languages | not written yet; until then `ROADMAP.md`, `docs/gaps.md` and the architecture table in `AGENTS.md` are the source |

A document that is "not written yet" is created by the arc of work that first has something
true to put in it. Nothing links to it as if it existed.

## Rules

1. **Every document opens with a status line:** `**Status:** revision N · date · one sentence
   on what the document covers and how far it can be trusted`. A document that changes bumps
   the revision and adds a line to its revision history (newest first) saying what changed.
2. **Say only what was checked.** "Compared with Outlook" means it was looked at side by side;
   otherwise the document says "not compared with Outlook". A number is one a command or a
   test produced (the test count comes from `cargo xtask ci`, the parity figure from
   `cargo xtask parity`).
3. **Nothing is dropped silently.** Work that is put off gets a row in `ROADMAP.md`'s "Do
   later" table with the milestone it is planned for. When it lands the row is struck through
   and marked `done:` with the task id; it is not deleted.
4. **When work lands, in the same commit:** the "Next" item of the current focus becomes
   "Done" and a new "Next" is written; the progress log gets a dated entry that says what was
   built, what was found and put right on the way, what was made better than plain parity,
   and what is still not done; `docs/gaps.md` and the parity document the work touches are
   updated; the status line in `AGENTS.md` and `CLAUDE.md` is brought up to date.
5. **Private data never appears.** These documents record layout, measurements, counts and
   schema. Nothing from a real mailbox (text, names, addresses, screenshots) goes into them.
6. **Plain words.** Entries are written so that someone who has not seen the code can follow
   them, and never copy Microsoft's wording beyond the names of features.

## Task ids

A commit carries the id of the task it finishes: milestone, a dot, a running number
(`M3b.9: what was left of a paragraph written from the right`). The same id appears in the
"Done" item, the progress log and the "Do later" row it strikes.
