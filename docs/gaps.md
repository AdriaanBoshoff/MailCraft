# Gaps

**Status:** revision 2 · 2026-10-11 · ranked: the top of the list is what hurts most.

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

7. **`.pst` reader has met few real files.** It is written from [MS-PST] and tested against files
   produced by a test-only builder, including truncated, corrupted and cyclic ones. Checked locally
   against a few small Outlook-written 4K-page `.ost` files (wVer 36): every folder and every item
   loaded. No Outlook-written Unicode or ANSI `.pst`, and no large mailbox, has been tried yet.
8. **No committed fixture for 4K-page files.** The real files used for the check above are private and
   stay local; the test builder only writes 512-byte-page files.
9. **`.msg`/`.oft` output is checked in Outlook Classic only for plain mail.** Outlook opens our
   messages and templates and reads back subject, sender, recipients, times, flags, categories, bodies
   and a file attachment ([file-format-parity.md](file-format-parity.md)). Attached messages, inline
   pictures, non-mail items and opening from the Outlook window are unchecked.
10. **The UI differs from Outlook Classic in measured ways.** A first side-by-side look is in
    [ui-parity.md](ui-parity.md): fixed pane widths, no Favorites, no ribbon overflow or command search.

## Known shortcomings

11. Special folders in a `.pst` are found by English name when the store does not identify them, so a
    localised Inbox may sort as an ordinary folder.
12. Message-list rows from a `.pst` have no preview line.
13. Date groups are Today, Yesterday, This Week, Last Week, This Month, Older; Outlook's are finer.
14. One message selected at a time; no multi-select, no conversations, no drag and drop.
15. Keyboard shortcuts cover the common commands only; there is no command search.
16. Password-protected and encrypted data files are refused or opened without the password check.
17. No localisation; times use a 12-hour clock and month/day/year order.
