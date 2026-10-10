//! Worker pool (R-STATE-3, R-PLAT-4): blocking work off the UI thread; results come back tagged with the screen
//! that asked, so a popped screen's late result is dropped.
use std::any::Any;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};

type Job = Box<dyn FnOnce() + Send>;

pub struct Done {
    pub screen: u64,
    pub tag: &'static str,
    pub payload: Box<dyn Any + Send>,
}

pub struct Pool {
    jobs: Sender<Job>,
    done_tx: Sender<Done>,
    done_rx: Receiver<Done>,
    pending: Arc<AtomicUsize>,
}

impl Pool {
    pub fn new(threads: usize) -> Pool {
        let (jobs, rx) = channel::<Job>();
        let rx = Arc::new(Mutex::new(rx));
        for i in 0..threads.max(1) {
            let rx = rx.clone();
            std::thread::Builder::new()
                .name(format!("jf-worker-{i}"))
                .spawn(move || loop {
                    let job = { rx.lock().unwrap().recv() };
                    match job {
                        Ok(j) => j(),
                        Err(_) => return,
                    }
                })
                .expect("spawn worker");
        }
        let (done_tx, done_rx) = channel();
        Pool { jobs, done_tx, done_rx, pending: Arc::new(AtomicUsize::new(0)) }
    }

    /// Runs `f` on a worker; its result arrives through `drain` as `Done { screen, tag, payload: Box<T> }`.
    pub fn spawn<T: Send + 'static>(&self, screen: u64, tag: &'static str, f: impl FnOnce() -> T + Send + 'static) {
        let tx = self.done_tx.clone();
        let pending = self.pending.clone();
        pending.fetch_add(1, Ordering::SeqCst);
        let _ = self.jobs.send(Box::new(move || {
            let payload: Box<dyn Any + Send> = Box::new(f());
            let _ = tx.send(Done { screen, tag, payload });
            pending.fetch_sub(1, Ordering::SeqCst);
        }));
    }

    /// Fire-and-forget work (progress reports): counted as pending, no result.
    pub fn run(&self, f: impl FnOnce() + Send + 'static) {
        let pending = self.pending.clone();
        pending.fetch_add(1, Ordering::SeqCst);
        let _ = self.jobs.send(Box::new(move || {
            f();
            pending.fetch_sub(1, Ordering::SeqCst);
        }));
    }

    pub fn drain(&self) -> Vec<Done> {
        self.done_rx.try_iter().collect()
    }

    /// Jobs queued or running, plus results not yet drained.
    pub fn pending(&self) -> usize {
        self.pending.load(Ordering::SeqCst)
    }
}
