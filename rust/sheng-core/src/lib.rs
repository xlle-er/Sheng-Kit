//! Pure caption state machine and pacing rules. No I/O, no async, no clocks of its own:
//! every time-dependent function takes the current time as an argument.

#![forbid(unsafe_code)]

pub mod align;
pub mod backoff;
pub mod caption;
pub mod ids;
pub mod language;
pub mod order;
pub mod pacing;
pub mod policy;
pub mod text;

pub use caption::{
    Caption, CaptionError, CaptionEvent, CaptionReducer, ReducerConfig, TrackView,
    CONFIRMED_QUEUE_LIMIT, RECENT_LIMIT,
};
pub use ids::{Source, UtteranceId};
pub use text::MAX_TEXT_BYTES;
