//! Test-only hooks for installing scripted service clients behind the C functions.

use std::sync::Arc;

use crate::factory::{set_default_factory, ClientFactory};
use crate::registry;

/// Uses `factory` for the sessions of `handle`; false for an unknown handle.
pub fn set_factory(handle: u64, factory: Arc<dyn ClientFactory>) -> bool {
    let Some(engine) = registry::get(handle) else {
        return false;
    };
    engine.set_factory(factory);
    true
}

/// Uses `factory` for new handles and probes; `None` restores the service factory.
pub fn set_default(factory: Option<Arc<dyn ClientFactory>>) {
    set_default_factory(factory);
}
