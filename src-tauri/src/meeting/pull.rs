//! One download of the local language model at a time. Setup starts it in
//! the background and the Models page may press Download meanwhile: a
//! second call waits for the running download and gets its result instead
//! of writing the same file twice. Its progress can be read at any time,
//! so a screen that opens mid-download shows how far it is.

use std::sync::{Condvar, Mutex, MutexGuard};

pub use sokki_engine::llm::PullProgress;

#[derive(Default)]
struct State {
    running: bool,
    progress: Option<PullProgress>,
    /// Counts finished downloads, so a waiter knows its one ended.
    finished: u64,
    last: Option<Result<(), String>>,
}

#[derive(Default)]
pub struct Flight {
    state: Mutex<State>,
    done: Condvar,
}

impl Flight {
    pub const fn new() -> Self {
        Self {
            state: Mutex::new(State {
                running: false,
                progress: None,
                finished: 0,
                last: None,
            }),
            done: Condvar::new(),
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Runs `download`, or, while one runs, waits for it and returns its
    /// result. `download` reports progress through the function it gets.
    pub fn run(
        &self,
        download: impl FnOnce(&dyn Fn(PullProgress)) -> Result<(), String>,
    ) -> Result<(), String> {
        let mut st = self.lock();
        if st.running {
            let mine = st.finished;
            while st.running && st.finished == mine {
                st = self.done.wait(st).unwrap_or_else(|e| e.into_inner());
            }
            return st.last.clone().unwrap_or(Ok(()));
        }
        st.running = true;
        st.progress = None;
        drop(st);
        let result = download(&|p| self.lock().progress = Some(p));
        let mut st = self.lock();
        st.running = false;
        st.progress = None;
        st.finished += 1;
        st.last = Some(result.clone());
        self.done.notify_all();
        result
    }

    /// The running download's progress: `None` when none runs, zeros
    /// before its first report.
    pub fn progress(&self) -> Option<PullProgress> {
        let st = self.lock();
        st.running.then(|| {
            st.progress.clone().unwrap_or(PullProgress {
                status: "starting".into(),
                completed: 0,
                total: 0,
            })
        })
    }
}

/// The app's one download of the language model.
pub static LLM: Flight = Flight::new();

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{mpsc, Arc};

    fn step(done: u64) -> PullProgress {
        PullProgress {
            status: "downloading".into(),
            completed: done,
            total: 100,
        }
    }

    #[test]
    fn a_second_call_waits_for_the_first_and_shares_its_result() {
        let flight = Arc::new(Flight::new());
        let calls = Arc::new(AtomicUsize::new(0));
        let (go_tx, go_rx) = mpsc::channel::<()>();
        let (started_tx, started_rx) = mpsc::channel::<()>();
        let first = {
            let (flight, calls) = (flight.clone(), calls.clone());
            std::thread::spawn(move || {
                flight.run(|report| {
                    calls.fetch_add(1, Ordering::SeqCst);
                    report(step(40));
                    started_tx.send(()).unwrap();
                    go_rx.recv().unwrap();
                    Err("network gone".into())
                })
            })
        };
        started_rx.recv().unwrap();
        assert_eq!(flight.progress(), Some(step(40)));
        let second = {
            let (flight, calls) = (flight.clone(), calls.clone());
            std::thread::spawn(move || {
                flight.run(|_| {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                })
            })
        };
        // Give the second call time to start waiting.
        std::thread::sleep(std::time::Duration::from_millis(100));
        go_tx.send(()).unwrap();
        assert_eq!(first.join().unwrap(), Err("network gone".into()));
        assert_eq!(second.join().unwrap(), Err("network gone".into()));
        assert_eq!(calls.load(Ordering::SeqCst), 1, "downloaded only once");
        assert_eq!(flight.progress(), None);
    }

    #[test]
    fn after_a_download_ends_the_next_call_downloads_again() {
        let flight = Flight::new();
        assert_eq!(flight.progress(), None);
        assert_eq!(flight.run(|_| Err("stopped".into())), Err("stopped".into()));
        let mut ran = false;
        assert_eq!(
            flight.run(|_| {
                ran = true;
                Ok(())
            }),
            Ok(())
        );
        assert!(ran);
    }

    #[test]
    fn progress_reads_zero_before_the_first_report() {
        let flight = Arc::new(Flight::new());
        let (tx, rx) = mpsc::channel::<()>();
        let (seen_tx, seen_rx) = mpsc::channel();
        let f = flight.clone();
        let t = std::thread::spawn(move || {
            f.run(|_| {
                seen_tx.send(()).unwrap();
                rx.recv().unwrap();
                Ok(())
            })
        });
        seen_rx.recv().unwrap();
        let p = flight.progress().unwrap();
        assert_eq!((p.completed, p.total), (0, 0));
        tx.send(()).unwrap();
        t.join().unwrap().unwrap();
    }
}
