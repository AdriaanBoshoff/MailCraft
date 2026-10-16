//! Work done on a thread of its own, such as talking to a mail server: the window is never
//! kept waiting for a network. A job's work gets no `Session`; what it found is handed back as
//! a step that `Session::poll` takes on the session's own thread, so the mailbox is only ever
//! changed there.

use crate::{CmdResult, EngineError, Session};
use openmailclient_model::Progress;
use serde_json::{Value, json};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

/// What a job found, taken on the session's thread: it says what came of it, in words for the
/// status bar and as a result for whoever asked.
pub type Ended = Box<dyn FnOnce(&mut Session) -> (String, Value) + Send>;

/// Results of jobs that ended, kept for whoever asks after them.
const MAX_FINISHED: usize = 20;

pub struct Job {
    /// Counted up from 1 over the run, so one can be named while others come and go.
    pub id: u64,
    /// What it is doing, in words.
    pub what: String,
    pub progress: Arc<Progress>,
    done: Receiver<Ended>,
}

impl Drop for Job {
    fn drop(&mut self) {
        self.progress.stop();
    }
}

/// A job's work ended in a panic: said, and nothing taken from it.
fn broke(what: String) -> Ended {
    Box::new(move |_| (format!("{what} failed inside; nothing was changed"), json!({ "ok": false })))
}

impl Session {
    /// Do `work` on a thread of its own and take what it found when it is done; with `wait`,
    /// or where there are no threads, do it here and now and give back what it found.
    pub(crate) fn begin(&mut self, what: String, wait: bool, work: impl FnOnce(&Progress) -> Ended + Send + 'static) -> CmdResult {
        let progress = Arc::new(Progress::default());
        if wait || cfg!(target_arch = "wasm32") {
            let ended = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work(&progress))).unwrap_or_else(|_| broke(what.clone()));
            // A data file being saved takes no change until the save ends: what the job found
            // waits for it, so none of it is turned away.
            self.end_saves(true);
            let (status, value) = ended(self);
            self.status = status;
            return Ok(value);
        }
        let (tx, done) = channel();
        let theirs = Arc::clone(&progress);
        let name = what.clone();
        std::thread::Builder::new()
            .name("job".to_string())
            .spawn(move || {
                let ended = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work(&theirs))).unwrap_or_else(|_| broke(name));
                let _ = tx.send(ended);
            })
            .map_err(|_| EngineError::Format("no thread could be had for this; ask again with \"wait\": true".into()))?;
        self.status = format!("{what}…");
        self.job_seq = self.job_seq.saturating_add(1);
        self.jobs.push(Job { id: self.job_seq, what: what.clone(), progress, done });
        Ok(json!({ "started": true, "job": what }))
    }

    /// Keep what came of a piece of work that ended, the newest last.
    pub(crate) fn ended(&mut self, what: Value) {
        self.finished.push(what);
        if self.finished.len() > MAX_FINISHED {
            self.finished.remove(0);
        }
    }

    /// Take what the jobs that are done found; with `wait`, wait for every one first. True
    /// when one ended.
    pub(crate) fn end_jobs(&mut self, wait: bool) -> bool {
        // What a job found is taken once no data file is being saved: a file being saved takes
        // no change, and mail that came or went would be turned away from it.
        if !wait && !self.saves.is_empty() {
            return false;
        }
        let mut ended = false;
        let mut at = 0;
        while let Some(job) = self.jobs.get(at) {
            let step = if wait {
                job.done.recv().unwrap_or_else(|_| broke(job.what.clone()))
            } else {
                match job.done.try_recv() {
                    Ok(step) => step,
                    Err(TryRecvError::Empty) => {
                        at += 1;
                        continue;
                    }
                    Err(TryRecvError::Disconnected) => broke(job.what.clone()),
                }
            };
            let job = self.jobs.remove(at);
            ended = true;
            let (status, value) = step(self);
            let ok = value.get("ok").and_then(Value::as_bool) != Some(false) && !job.progress.stopped();
            self.status = status.clone();
            self.ended(json!({ "job": job.what, "result": value, "said": status, "ok": ok, "stopped": job.progress.stopped() }));
        }
        ended
    }
}

/// Work that runs, as Show Progress lists it.
#[derive(Clone, Debug, PartialEq)]
pub struct Running {
    /// "job-N", "save-N" (the data file's place) or "export-N" (its place in the list).
    pub id: String,
    pub what: String,
    /// From 0 to 1; 0 while it is not known how much there is.
    pub done: f32,
    /// Asked to stop, and stopping at the next step it comes to.
    pub stopping: bool,
}

impl Session {
    /// Everything that runs on a thread of its own, the oldest first.
    pub fn running(&self) -> Vec<Running> {
        let saves = self.saves.iter().map(|s| Running {
            id: format!("save-{}", s.store),
            what: format!("Saving {}", s.name),
            done: s.progress.fraction().clamp(0.0, 1.0),
            stopping: s.progress.stopped(),
        });
        let exports = self.exports.iter().enumerate().map(|(i, e)| Running {
            id: format!("export-{i}"),
            what: format!("Exporting to {}", e.to.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()),
            done: e.progress.fraction().clamp(0.0, 1.0),
            stopping: e.progress.stopped(),
        });
        let jobs = self.jobs.iter().map(|j| Running {
            id: format!("job-{}", j.id),
            what: j.what.clone(),
            done: j.progress.fraction().clamp(0.0, 1.0),
            stopping: j.progress.stopped(),
        });
        saves.chain(exports).chain(jobs).collect()
    }
}
