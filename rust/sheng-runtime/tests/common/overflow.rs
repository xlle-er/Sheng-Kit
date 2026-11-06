//! Shared check for step 5 fix 1: a service whose reader meets a full provider event queue
//! sets the sink's overflow flag and stops reading, so the session sees a closed channel and
//! reports `provider_event_backlog_overflow`.

use std::time::Duration;

use sheng_runtime::provider::traits::PROVIDER_EVENT_QUEUE;
use sheng_runtime::provider::{OverflowFlag, ProviderEvent};
use tokio::sync::mpsc::Receiver;

/// Messages a mock server sends to overflow the queue for certain.
pub const FLOOD: usize = PROVIDER_EVENT_QUEUE + 64;

const WAIT: Duration = Duration::from_secs(5);
const POLL: Duration = Duration::from_millis(10);

/// Leaves `events` unread until the flag is set, then checks the reader stopped: the queue
/// never held more than its bound and the channel closes once drained.
pub async fn assert_reader_stops_on_overflow(
    flag: &OverflowFlag,
    events: &mut Receiver<ProviderEvent>,
) {
    let flagged = tokio::time::timeout(WAIT, async {
        while !flag.is_set() {
            tokio::time::sleep(POLL).await;
        }
    })
    .await;
    assert!(flagged.is_ok(), "overflow flag was not set");
    let mut received = 0;
    loop {
        let next = tokio::time::timeout(WAIT, events.recv())
            .await
            .expect("event channel closes after the overflow");
        match next {
            Some(_) => received += 1,
            None => break,
        }
    }
    assert!(received <= PROVIDER_EVENT_QUEUE, "queue held {received}");
}
