//! Live engine handles by id. Ids are never reused within a process.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use crate::session::EngineHandle;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);
static HANDLES: Mutex<Option<HashMap<u64, Arc<EngineHandle>>>> = Mutex::new(None);

fn handles() -> MutexGuard<'static, Option<HashMap<u64, Arc<EngineHandle>>>> {
    HANDLES.lock().unwrap_or_else(PoisonError::into_inner)
}

pub fn create() -> u64 {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    handles()
        .get_or_insert_with(HashMap::new)
        .insert(id, Arc::new(EngineHandle::new()));
    id
}

pub fn get(id: u64) -> Option<Arc<EngineHandle>> {
    handles().as_ref()?.get(&id).cloned()
}

pub fn remove(id: u64) -> Option<Arc<EngineHandle>> {
    handles().as_mut()?.remove(&id)
}
