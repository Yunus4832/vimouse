//! Coalescing, single-writer persistence. No file operations run in the input loop.
use super::{Marks, Store};
use std::{
    io,
    sync::{
        Arc, Condvar, Mutex,
        mpsc::{self, Receiver},
    },
    thread::{self, JoinHandle},
};
struct Snapshot {
    revision: u64,
    marks: Marks,
}
#[derive(Default)]
struct Mailbox {
    latest: Option<Snapshot>,
    closing: bool,
}
pub struct SaveResult {
    pub revision: u64,
    pub result: Result<(), String>,
}
pub struct SaveWorker {
    mailbox: Arc<(Mutex<Mailbox>, Condvar)>,
    results: Receiver<SaveResult>,
    thread: Option<JoinHandle<()>>,
    last_submitted: u64,
}
impl SaveWorker {
    pub fn spawn(mut store: Store) -> io::Result<Self> {
        Self::launch(move |revision, marks| {
            store
                .sync(revision, marks)
                .map_err(|error| format!("Cannot save {}: {error}", store.path().display()))
        })
    }
    fn launch(
        mut save: impl FnMut(u64, &Marks) -> Result<(), String> + Send + 'static,
    ) -> io::Result<Self> {
        let mailbox = Arc::new((Mutex::new(Mailbox::default()), Condvar::new()));
        let (send, results) = mpsc::channel();
        let shared = Arc::clone(&mailbox);
        let thread = thread::Builder::new()
            .name("vimouse-marks".into())
            .spawn(move || {
                let (lock, wake) = &*shared;
                loop {
                    let snapshot = {
                        let mut state = lock.lock().unwrap_or_else(|e| e.into_inner());
                        while state.latest.is_none() && !state.closing {
                            state = wake.wait(state).unwrap_or_else(|e| e.into_inner());
                        }
                        match state.latest.take() {
                            Some(snapshot) => snapshot,
                            None => break,
                        }
                    }; // Release the mailbox before writing or syncing the filesystem.
                    let result = save(snapshot.revision, &snapshot.marks);
                    if let Err(error) = &result {
                        eprintln!("{error}; marks kept in memory");
                    }
                    if send
                        .send(SaveResult {
                            revision: snapshot.revision,
                            result,
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            })?;
        Ok(Self {
            mailbox,
            results,
            thread: Some(thread),
            last_submitted: 0,
        })
    }
    pub fn submit(&mut self, revision: u64, marks: &Marks) -> io::Result<bool> {
        if revision == self.last_submitted {
            return Ok(false);
        }
        if self
            .thread
            .as_ref()
            .is_some_and(|thread| thread.is_finished())
        {
            return Err(io::Error::other("mark saver stopped unexpectedly"));
        }
        let (lock, wake) = &*self.mailbox;
        let mut state = lock
            .lock()
            .map_err(|_| io::Error::other("mark saver mailbox failed"))?;
        state.latest = Some(Snapshot {
            revision,
            marks: marks.clone(),
        });
        self.last_submitted = revision;
        wake.notify_one();
        Ok(true)
    }
    pub fn poll(&self) -> impl Iterator<Item = SaveResult> + '_ {
        self.results.try_iter()
    }
}
impl Drop for SaveWorker {
    fn drop(&mut self) {
        let (lock, wake) = &*self.mailbox;
        lock.lock().unwrap_or_else(|e| e.into_inner()).closing = true;
        wake.notify_one();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    #[test]
    fn blocked_writer_does_not_hold_the_input_mailbox_and_shutdown_flushes_latest() {
        let (started, ready) = mpsc::channel();
        let (release, gate) = mpsc::channel();
        let written = Arc::new(Mutex::new(Vec::new()));
        let saved = Arc::clone(&written);
        let mut worker = SaveWorker::launch(move |revision, _| {
            if revision == 1 {
                started.send(()).unwrap();
                gate.recv_timeout(Duration::from_secs(2)).unwrap();
            }
            saved.lock().unwrap().push(revision);
            Ok(())
        })
        .unwrap();
        worker.submit(1, &Marks::new()).unwrap();
        ready.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(
            worker.mailbox.0.try_lock().is_ok(),
            "filesystem I/O must run outside the input mailbox lock"
        );
        worker.submit(2, &Marks::new()).unwrap();
        worker.submit(3, &Marks::new()).unwrap();
        release.send(()).unwrap();
        drop(worker);
        assert_eq!(*written.lock().unwrap(), vec![1, 3]);
    }
}
