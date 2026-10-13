# Never crash, never lose mail

**Status:** revision 2 · 2026-10-13 · says what a journalled write into a data file is (M3b.29); written for this repository; it replaces a link to a
standard kept elsewhere that readers of this repository could not open.

People trust a mail client with years of mail. A crash loses what was being done; a bad write
loses what was kept. This standard outranks feature work: a feature never ships through a path
that can panic, and a crash is fixed before anything is built on top of it.

## The rules

1. **No panics in code that ships.** No `unwrap()`, `expect()`, `panic!`, `unreachable!`,
   `todo!` or `unimplemented!` outside tests, and no `unsafe` (`unsafe_code = "forbid"` in the
   workspace). Every production crate root carries

   ```rust
   #![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
   ```

   and a new crate starts with that line. Tests may unwrap (`clippy.toml`).
2. **Errors are values.** A function that can fail returns `Result<T, E>` with its crate's
   error type, and callers pass it on with `?`. A feature that is not finished returns an
   error or is disabled (its command's `enabled` says no); it never panics.
3. **Numbers that come from outside are hostile.** Files, messages, server replies, command
   parameters and control or MCP parameters are written by someone else. For anything derived
   from them:
   - `get()` and `get(a..b)` instead of `[i]` and `[a..b]`;
   - strings are cut only at character boundaries;
   - arithmetic is checked or saturating (`checked_add`, `saturating_sub`, `try_from`);
   - sizes are clamped and allocations capped before they are made (a length field is not a
     reason to reserve that much memory);
   - recursion and loops that follow references in the data are bounded (a depth limit, a
     visited set, a step count), so a cycle or a very deep tree ends;
   - floating-point values are checked with `is_finite()` before they are used as sizes or
     places.

   A `.pst` block offset, a MIME boundary, a TNEF length and an IMAP literal size are all
   under an attacker's control.
4. **A user's data file is never damaged.**
   - Opening is read-only until a write is asked for. Unsaved changes are journalled in a file
     of our own beside the data file (`crates/pst/src/journal.rs`); the data file is written
     only by a save.
   - A write to `.pst`, `.ost` or `.msg` goes to a temporary file that is swapped in
     atomically, or is journalled. A write that fails or is interrupted leaves the original
     byte for byte as it was.
   - A journalled write into a data file (`crates/pst/src/save_onto.rs`) writes only into
     space the file calls free and past its end, and the header last. What it writes over
     is first put in an undo file of our own beside the data file and forced to disk
     (`crates/pst/src/undo.rs`); taking that file away, after the result was checked and
     read back, is what ends the write. Until then the data file is read as it was, through
     the undo file, and is put back from it before it is written again. Opening still
     writes nothing.
   - A store that Outlook has open is never written.
5. **The last-resort guard is not a licence.** `Session::run` turns a panic that escaped into
   an error and puts the session's state back. It exists so that a mistake costs one command
   and not a mailbox; code is still written as if it were not there.

## Proving it

- **Every crash fix lands with a test** that fails without the fix.
- **Every command is run with junk.** `crates/engine/src/tests.rs::hostile_params_never_panic`
  runs every registered command with parameters of the wrong type, wrong size and wrong shape.
  It stays green; a new command is covered by it without doing anything.
- **Every reader of a file format has a hostile-input test:** truncated at every length that
  matters, sizes far larger than the file, references that point at themselves or at each
  other, and plain garbage. The test asserts an error or a partial result, never a panic, and
  that time and memory stay bounded.
- **Every writer that touches a user's file has a test that interrupts it** and compares the
  original with what is on disk afterwards.
- `cargo xtask ci` runs clippy with warnings as errors, so the lints in rule 1 fail the build.

## When a rule seems to be in the way

Indexing that "cannot fail" is written with `get()` anyway: the proof that it cannot fail
lives in the reader's head, and the next change breaks it. Where a value truly cannot be
absent, return the crate's error for the impossible case; it costs one line and is never the
line a user's mail is lost on.
