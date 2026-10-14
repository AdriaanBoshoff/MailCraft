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
        self.jobs.push(Job { what: what.clone(), progress, done });
        Ok(json!({ "started": true, "job": what }))
    }

    /// Take what the jobs that are done found; with `wait`, wait for every one first. True
    /// when one ended.
    pub(crate) fn end_jobs(&mut self, wait: bool) -> bool {
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
            self.status = status;
            self.finished.push(json!({ "job": job.what, "result": value }));
            if self.finished.len() > MAX_FINISHED {
                self.finished.remove(0);
            }
        }
        ended
    }
}
