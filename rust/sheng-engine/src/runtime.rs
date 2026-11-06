//! The shared async runtime that runs sessions and operations off the caller's threads.

use std::sync::OnceLock;

use tokio::runtime::{Builder, Handle, Runtime};

const WORKER_THREADS: usize = 2;

static RUNTIME: OnceLock<Option<Runtime>> = OnceLock::new();

/// Handle of the shared runtime, created on first use; `None` if it could not be created.
pub fn handle() -> Option<Handle> {
    RUNTIME
        .get_or_init(|| {
            Builder::new_multi_thread()
                .worker_threads(WORKER_THREADS)
                .thread_name("sheng-engine")
                .enable_all()
                .build()
                .ok()
        })
        .as_ref()
        .map(|runtime| runtime.handle().clone())
}
