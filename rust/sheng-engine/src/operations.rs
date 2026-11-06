//! Asynchronous operations shared by resource preparation and connection checks
//! (abi-v1.md “异步操作”). At most eight exist at once.

use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use serde_json::{json, Value};
use tokio::task::AbortHandle;

/// Operations that may exist at the same time.
pub const MAX_OPERATIONS: usize = 8;

pub type Outcome = Result<Value, &'static str>;

enum Entry {
    Pending(Arc<Mutex<Option<Outcome>>>, Option<AbortHandle>),
    Done(Outcome),
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);
static TABLE: Mutex<Option<HashMap<u64, Entry>>> = Mutex::new(None);

fn table() -> MutexGuard<'static, Option<HashMap<u64, Entry>>> {
    TABLE.lock().unwrap_or_else(PoisonError::into_inner)
}

fn insert(entry: Entry) -> u64 {
    let mut guard = table();
    let map = guard.get_or_insert_with(HashMap::new);
    if map.len() >= MAX_OPERATIONS {
        return 0;
    }
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    map.insert(id, entry);
    id
}

/// Registers an operation that is already complete; `0` when the table is full.
pub fn finished(outcome: Outcome) -> u64 {
    insert(Entry::Done(outcome))
}

/// Runs `work` on the shared runtime; `0` when the table is full or nothing can run.
pub fn spawn<F>(work: F) -> u64
where
    F: Future<Output = Outcome> + Send + 'static,
{
    let Some(runtime) = crate::runtime::handle() else {
        return 0;
    };
    let slot = Arc::new(Mutex::new(None));
    let id = insert(Entry::Pending(Arc::clone(&slot), None));
    if id == 0 {
        return 0;
    }
    let task = runtime.spawn(async move {
        let outcome = work.await;
        *slot.lock().unwrap_or_else(PoisonError::into_inner) = Some(outcome);
    });
    if let Some(Some(Entry::Pending(_, abort))) = table().as_mut().map(|map| map.get_mut(&id)) {
        *abort = Some(task.abort_handle());
    } else {
        task.abort();
    }
    id
}

fn state_json(outcome: &Outcome) -> Value {
    match outcome {
        Ok(value) => json!({"state": "done", "value": value}),
        Err(code) => json!({"state": "failed", "error": code}),
    }
}

pub fn poll(id: u64) -> Value {
    let mut guard = table();
    let Some(entry) = guard.as_mut().and_then(|map| map.get_mut(&id)) else {
        return state_json(&Err("invalid_operation"));
    };
    if let Entry::Pending(slot, _) = entry {
        let ready = slot.lock().unwrap_or_else(PoisonError::into_inner).take();
        match ready {
            Some(outcome) => *entry = Entry::Done(outcome),
            None => return json!({"state": "pending"}),
        }
    }
    match entry {
        Entry::Done(outcome) => state_json(outcome),
        Entry::Pending(..) => json!({"state": "pending"}),
    }
}

pub fn cancel(id: u64) {
    let removed = table().as_mut().and_then(|map| map.remove(&id));
    if let Some(Entry::Pending(_, Some(abort))) = removed {
        abort.abort();
    }
}
