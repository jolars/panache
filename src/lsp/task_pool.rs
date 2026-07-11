//! A minimal fixed-size worker thread pool, modeled on rust-analyzer's
//! `TaskPool`.
//!
//! Heavy LSP reads (hover, completion, formatting, lint) are dispatched as
//! closures onto a small pool of std threads. Each closure posts its result onto
//! a shared result channel that the [`main loop`](crate::lsp) selects on, so
//! completed work re-enters the single-threaded event loop to be turned into a
//! response or a diagnostics publish.

use crossbeam_channel::Sender;

/// A boxed unit of work to run on a worker thread.
type Job = Box<dyn FnOnce() + Send + 'static>;

/// A fixed pool of worker threads that produce results of type `T`.
pub(crate) struct TaskPool<T> {
    job_tx: Sender<Job>,
    result_tx: Sender<T>,
    _workers: Vec<std::thread::JoinHandle<()>>,
}

impl<T: Send + 'static> TaskPool<T> {
    /// Spawn `n` worker threads (clamped to at least 1). Completed jobs send
    /// their `T` results on `result_tx`, which the caller selects on.
    pub(crate) fn new(result_tx: Sender<T>, n: usize) -> Self {
        let n = n.max(1);
        let (job_tx, job_rx) = crossbeam_channel::unbounded::<Job>();
        let workers = (0..n)
            .map(|_| {
                let job_rx = job_rx.clone();
                std::thread::Builder::new()
                    .name("panache-lsp-worker".to_owned())
                    .spawn(move || {
                        // Exits cleanly when all `job_tx` clones drop.
                        for job in job_rx {
                            // Catch genuine panics so one buggy handler can't
                            // silently take a worker out of rotation. Salsa
                            // `Cancelled` is already caught by `catch_cancelled`
                            // upstream — anything reaching here is a real bug.
                            if let Err(panic) =
                                std::panic::catch_unwind(std::panic::AssertUnwindSafe(job))
                            {
                                let msg = crate::lsp::helpers::panic_message(panic.as_ref());
                                log::error!("LSP task pool worker caught panic: {msg}");
                            }
                        }
                    })
                    .expect("failed to spawn LSP worker thread")
            })
            .collect();
        Self {
            job_tx,
            result_tx,
            _workers: workers,
        }
    }

    /// Hand a closure to the pool. It runs on some worker thread.
    pub(crate) fn spawn(&self, f: impl FnOnce() + Send + 'static) {
        // Send only fails if every worker has died, which we treat as shutdown.
        let _ = self.job_tx.send(Box::new(f));
    }

    /// A clone of the result sender, for workers that post results themselves.
    pub(crate) fn result_sender(&self) -> Sender<T> {
        self.result_tx.clone()
    }

    /// A detached spawn handle onto this pool's job queue, for a thread (the
    /// salsa writer) that dispatches work without owning the pool.
    pub(crate) fn spawner(&self) -> TaskSpawner {
        TaskSpawner {
            job_tx: self.job_tx.clone(),
        }
    }
}

/// A clonable handle that spawns jobs onto a [`TaskPool`] without borrowing it.
#[derive(Clone)]
pub(crate) struct TaskSpawner {
    job_tx: Sender<Job>,
}

impl TaskSpawner {
    /// Hand a closure to the pool. It runs on some worker thread.
    pub(crate) fn spawn(&self, f: impl FnOnce() + Send + 'static) {
        let _ = self.job_tx.send(Box::new(f));
    }
}

/// Default worker count for the main request pool — physical cores, matching
/// rust-analyzer. Hyperthreaded siblings don't meaningfully help CPU-bound
/// parser/lint reads.
pub(crate) fn default_pool_size() -> usize {
    num_cpus::get_physical().max(1)
}
