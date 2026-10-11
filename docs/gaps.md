# Gaps

**Status:** revision 1 · 2026-10-11 · ranked: the top of the list is what hurts most.

## Not built

1. **No accounts, no sending, no receiving.** IMAP, POP3, SMTP and Exchange are absent. `compose.send`
   files the message in Outbox and says so.
2. **`.pst`/`.ost` are read-only.** No writer exists; every change to such a store is refused. Changes
   made in the sample mailbox live in memory and are lost on exit.
3. **Message bodies are plain text.** HTML is reduced to text (no formatting, tables or pictures).
   Bodies stored only as compressed RTF ([MS-OXRTFCP]) are not decoded and show empty.
4. **Calendar, People, Tasks** modules show a placeholder.
5. **MCP server and control channel** (`mailcraft-cli mcp`, `--control`) are not built.
6. **Web build.** The engine and everything under it compiles for wasm32; there is no web app.

## Built but unproven

7. **`.pst` reader has only met our own fixtures.** It is written from [MS-PST] and tested against files
   produced by a test-only builder, including truncated, corrupted and cyclic ones. It has not been
   checked against a broad set of Outlook-written files.
8. **4K-page `.ost` (wVer 36)** support is a best effort from the specification, with no fixture at all.
9. **`.msg` output has not been opened in Outlook Classic.** It round-trips through our own reader and
   follows [MS-OXMSG]; that is not the same as Outlook accepting it.
10. **The UI has not been compared side by side with Outlook Classic.** Layout follows the general
    arrangement; measurements, spacing and behaviour details are unverified.

## Known shortcomings

11. Special folders in a `.pst` are found by English name when the store does not identify them, so a
    localised Inbox may sort as an ordinary folder.
12. Message-list rows from a `.pst` have no preview line.
13. Date groups are Today, Yesterday, This Week, Last Week, This Month, Older; Outlook's are finer.
14. One message selected at a time; no multi-select, no conversations, no drag and drop.
15. Keyboard shortcuts cover the common commands only; there is no command search.
16. Password-protected and encrypted data files are refused or opened without the password check.
17. No localisation; times use a 12-hour clock and month/day/year order.
