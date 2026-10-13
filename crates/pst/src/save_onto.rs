//! Saving into the data file where it is: what changed is written into the file's free end
//! and past it, and the header is written last (`write_onto.rs`). A flag on one message of a
//! mailbox of gigabytes is then a few pages, and not the mailbox again.
//!
//! The data file is still never left damaged. Before the first byte is written, what will
//! be written over is kept in an undo file beside the data file and forced to disk
//! (`undo.rs`). The header that names the new state goes in only when everything it names
//! is on disk; the file is then held against the structure check and read back, and taking
//! the undo file away is what ends the save. Anything that goes wrong before that puts the
//! file back as it was, byte for byte, and the save is done the old way: a whole new file
//! put in this one's place. A run that ends in the middle leaves the undo file, and the
//! next run reads the data file as it was and puts it back when a save is next asked for.
//!
//! A file is written into only when it is plainly sound and nobody else has it open for
//! writing; the first save of a file whose original is to be kept, and a file earlier saves
//! have left too much unused space in, are written whole.

use super::*;
use crate::undo::{Undo, undo_beside};
use crate::write::Onto;

/// Unused space earlier saves left is let be up to this much, or a quarter of the file.
const WASTE_FLOOR: u64 = 4 << 20;

#[cfg(test)]
thread_local! {
    /// The stage at which a save stops dead, as when the power goes: nothing is put back.
    pub(crate) static CRASH: std::cell::Cell<Option<u8>> = const { std::cell::Cell::new(None) };
    /// The stage at which a save meets an error.
    pub(crate) static FAIL: std::cell::Cell<Option<u8>> = const { std::cell::Cell::new(None) };
    /// Another floor for the unused space that is let be.
    pub(crate) static WASTE: std::cell::Cell<Option<u64>> = const { std::cell::Cell::new(None) };
}

#[cfg(test)]
fn trip(at: u8) -> Result<(), PstError> {
    if CRASH.get() == Some(at) || FAIL.get() == Some(at) {
        return Err(PstError::Write(format!("stopped at stage {at} by a test")));
    }
    Ok(())
}

#[cfg(not(test))]
fn trip(_at: u8) -> Result<(), PstError> {
    Ok(())
}

#[cfg(test)]
fn crashing() -> bool {
    CRASH.get().is_some()
}

#[cfg(not(test))]
fn crashing() -> bool {
    false
}

#[cfg(test)]
fn waste_floor() -> u64 {
    WASTE.get().unwrap_or(WASTE_FLOOR)
}

#[cfg(not(test))]
fn waste_floor() -> u64 {
    WASTE_FLOOR
}

/// The data file, to be written into: an error when another program has it open for
/// writing. Others may go on reading it, as this program's own readers do.
#[cfg(windows)]
fn for_change(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::windows::fs::OpenOptionsExt;
    // FILE_SHARE_READ: the open succeeds only when nobody else can write the file.
    std::fs::File::options().read(true).write(true).share_mode(1).open(path)
}

#[cfg(not(windows))]
fn for_change(path: &Path) -> std::io::Result<std::fs::File> {
    std::fs::File::options().read(true).write(true).open(path)
}

fn was_stopped(e: &PstError) -> bool {
    matches!(e, PstError::Stopped) || matches!(e, PstError::Write(m) if m == STOPPED)
}

impl PstStore {
    /// Put the data file back as it was before a save that was cut short, when there was
    /// one. Until then the file is read as it was, through the undo file.
    pub(super) fn restore_torn(&mut self) -> Result<(), PstError> {
        let Some(undo) = self.torn.clone() else { return Ok(()) };
        let path = self.path.clone().ok_or_else(|| PstError::Write("this data file is not kept in a file".to_string()))?;
        let mut file = for_change(&path)
            .map_err(|e| PstError::Write(format!("a save that was cut short cannot be undone while the data file cannot be written ({e})")))?;
        if !undo.fits(&mut file) {
            return Err(PstError::Write("another program changed the data file since it was opened; nothing was saved".to_string()));
        }
        undo.restore(&mut file)?;
        drop(file);
        self.torn = None;
        let _ = std::fs::remove_file(undo_beside(&path));
        self.ndb = Ndb::open(Box::new(std::fs::File::open(&path)?))?;
        self.note_stamp();
        Ok(())
    }

    /// Save the changes into the data file where it is. `None` when that is not done for
    /// this file, or was tried and undone: the file is as it was, and is to be written whole.
    pub(crate) fn in_place(&mut self, path: &Path, seed: [u8; 16]) -> Result<Option<Written>, PstError> {
        self.onto_failed = None;
        if !self.in_place || self.torn.is_some() || !self.can_edit() {
            return Ok(None);
        }
        // The first save keeps the file as it was under another name, which costs nothing
        // when the new file takes its place and would cost a copy here.
        if self.keep_before && !original_beside(path).exists() {
            return Ok(None);
        }
        let Ok(mut file) = for_change(path) else { return Ok(None) };
        let Some(onto) = Onto::read(&mut file) else { return Ok(None) };
        // Writing the file whole is what gives unused space back.
        if onto.waste() > waste_floor().max(onto.size() / 4) {
            return Ok(None);
        }
        let written_at = file.metadata().and_then(|m| m.modified()).ok();
        let mut undo = onto.undo();
        let since = onto.since();
        let at = undo_beside(path);
        if undo.write(&at).is_err() {
            let _ = std::fs::remove_file(&at);
            return Ok(None);
        }
        let done = self.onto_file(path, &mut file, onto, &mut undo, &at, since, seed).and_then(|ok| {
            trip(6)?;
            // Taking the undo file away ends the save.
            std::fs::remove_file(&at)?;
            Ok(ok)
        });
        let e = match done {
            Ok((fresh, written)) => {
                drop(file);
                self.ndb = fresh.ndb;
                self.finish_swap(fresh.name, fresh.names, fresh.root, fresh.special);
                self.onto_done += 1;
                return Ok(Some(written));
            }
            Err(e) => e,
        };
        if crashing() {
            return Err(e);
        }
        if let Err(io) = undo.restore(&mut file) {
            self.torn = Some(undo);
            return Err(PstError::Write(format!(
                "the save failed ({e}) and the data file could not be put back as it was ({io}); what it held is kept in {} and is put back by the next save",
                at.display()
            )));
        }
        // The file is what it was, and says so.
        if let Some(when) = written_at {
            let _ = file.set_modified(when);
        }
        drop(file);
        let _ = std::fs::remove_file(&at);
        self.note_stamp();
        if was_stopped(&e) {
            return Err(e);
        }
        self.onto_failed = Some(e.to_string());
        Ok(None)
    }

    /// Write the changes into `file`, whose undo file is at `at` and on disk; check what was
    /// written and read it back. Gives the file as it is now, open.
    #[allow(clippy::too_many_arguments)]
    fn onto_file(
        &mut self,
        path: &Path,
        file: &mut std::fs::File,
        onto: Onto,
        undo: &mut Undo,
        at: &Path,
        since: u64,
        seed: [u8; 16],
    ) -> Result<(Self, Written), PstError> {
        trip(1)?;
        let mut o = Out { w: NdbWriter::onto(&mut *file, onto), names: Names::seeded(&self.names), uid: seed, serial: 0 };
        o.w.carry(self.ndb.counters);
        let edits = self.edits.take();
        let wrote = self.write_into(o, edits.as_ref(), Vec::new());
        self.edits = edits;
        let (o, done) = wrote?;
        let (_, head, changed) = o.w.finish_onto()?;
        trip(2)?;
        // Everything the new header names is on disk before the header is.
        file.sync_data()?;
        undo.next = head.get(..1024).map(<[u8]>::to_vec);
        undo.write(at)?;
        trip(3)?;
        file.seek(SeekFrom::Start(0))?;
        #[cfg(test)]
        if CRASH.get() == Some(4) {
            file.write_all(head.get(..512).unwrap_or_default())?;
            return Err(PstError::Write("stopped in the middle of the header by a test".to_string()));
        }
        file.write_all(&head)?;
        file.sync_all()?;
        trip(5)?;
        // The file against the structure check: what was there before is not read again,
        // what names it is.
        let stop = self.progress.clone();
        let found = match crate::check::check_since(&mut std::io::BufReader::new(&mut *file), stop.as_deref(), since, &changed) {
            Err(PstError::Stopped) => return Err(PstError::Write(STOPPED.to_string())),
            found => found?,
        };
        if let Some(problem) = found.problems.first() {
            return Err(PstError::Write(format!("the file did not pass its own check ({problem})")));
        }
        // Inside the nodes, what was wrong before is not this save's doing.
        if !found.inside.is_empty() {
            let was: HashSet<String> = crate::check::inside_of(&mut self.ndb, &changed).into_iter().collect();
            if let Some(problem) = found.inside.iter().find(|p| !was.contains(*p)) {
                return Err(PstError::Write(format!("the file did not pass its own check ({problem})")));
            }
        }
        if let Some(p) = &self.progress {
            p.part(4);
        }
        let mut fresh = Self::open_plain(path)?;
        if !self.reads_back(&mut fresh)? {
            return Err(PstError::Write("the file does not read back as the mailbox that was saved".to_string()));
        }
        if let Some(p) = &self.progress {
            p.part(4);
        }
        if self.progress.as_ref().is_some_and(|p| p.stopped()) {
            return Err(PstError::Write(STOPPED.to_string()));
        }
        Ok((fresh, done))
    }
}
