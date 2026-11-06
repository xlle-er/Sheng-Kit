//! Routing of bridge callbacks back to Rust. Each speech session and each one-shot call gets
//! a fresh token; callbacks for tokens that are no longer registered are dropped, so late
//! events of a closed session cannot reach a newer one.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use tokio::sync::oneshot;

/// Event kinds of a recognition session (`SpeechEvent` in the Swift bridge).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BridgeEvent {
    Ready,
    Draft(String),
    Final(String),
    Ended,
    /// Failure name, see `status::speech_failure`.
    Failed(String),
}

impl BridgeEvent {
    pub fn from_raw(kind: i32, text: String) -> Option<Self> {
        Some(match kind {
            0 => Self::Ready,
            1 => Self::Draft(text),
            2 => Self::Final(text),
            3 => Self::Ended,
            4 => Self::Failed(text),
            _ => return None,
        })
    }
}

/// Answer to a one-shot bridge call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    pub ok: bool,
    pub text: String,
}

type Handler = Arc<dyn Fn(BridgeEvent) + Send + Sync>;

static NEXT_TOKEN: AtomicU64 = AtomicU64::new(1);
static HANDLERS: Mutex<Option<HashMap<u64, Handler>>> = Mutex::new(None);
static REPLIES: Mutex<Option<HashMap<u64, oneshot::Sender<Reply>>>> = Mutex::new(None);

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn next_token() -> u64 {
    NEXT_TOKEN.fetch_add(1, Ordering::Relaxed)
}

/// Registers a session's event handler and returns its token.
pub fn register(handler: impl Fn(BridgeEvent) + Send + Sync + 'static) -> u64 {
    let token = next_token();
    lock(&HANDLERS)
        .get_or_insert_with(HashMap::new)
        .insert(token, Arc::new(handler));
    token
}

pub fn unregister(token: u64) {
    if let Some(map) = lock(&HANDLERS).as_mut() {
        map.remove(&token);
    }
}

/// Hands an event to the session registered under `token`, if it still is.
pub fn deliver(token: u64, event: BridgeEvent) {
    let handler = lock(&HANDLERS)
        .as_ref()
        .and_then(|map| map.get(&token).cloned());
    if let Some(handler) = handler {
        handler(event);
    }
}

/// A pending one-shot call; dropping it forgets the token.
pub struct PendingReply {
    pub token: u64,
    receiver: Option<oneshot::Receiver<Reply>>,
}

impl PendingReply {
    pub fn new() -> Self {
        let token = next_token();
        let (sender, receiver) = oneshot::channel();
        lock(&REPLIES)
            .get_or_insert_with(HashMap::new)
            .insert(token, sender);
        Self {
            token,
            receiver: Some(receiver),
        }
    }

    /// Waits for the reply; `None` when the call was forgotten or answered nothing.
    pub async fn wait(mut self) -> Option<Reply> {
        self.receiver.take()?.await.ok()
    }
}

impl Default for PendingReply {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for PendingReply {
    fn drop(&mut self) {
        if let Some(map) = lock(&REPLIES).as_mut() {
            map.remove(&self.token);
        }
    }
}

/// Completes the call registered under `token`; unknown tokens are ignored.
pub fn complete(token: u64, reply: Reply) {
    let sender = lock(&REPLIES).as_mut().and_then(|map| map.remove(&token));
    if let Some(sender) = sender {
        let _ = sender.send(reply);
    }
}
