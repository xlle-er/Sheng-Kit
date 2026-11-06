//! Apple on-device recognition and translation (step 4a, apple.md).
//!
//! The Swift bridge in `rust/sheng-runtime/apple/` does the framework work and never
//! captures audio. Logic that does not need the frameworks (locale mapping, status
//! strings, error codes, event routing, resource rules) lives here and is tested with
//! fakes of the `system` traits.

use std::sync::Arc;

use sheng_core::Source;

use crate::config::SessionPlan;
use crate::provider::{SessionError, SpeechClient, TextTranslator};

#[cfg(target_os = "macos")]
pub mod bridge;
pub mod events;
pub mod locale;
pub mod resources;
pub mod speech;
pub mod status;
pub mod system;
pub mod translator;

pub use speech::AppleSpeech;
pub use system::Apple;
pub use translator::AppleTranslator;

/// The real system services, or `None` where the bridge does not exist.
pub fn system() -> Option<Apple> {
    #[cfg(target_os = "macos")]
    {
        bridge::apple()
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

/// Speech client for one source's lane (appleSpeech).
pub fn speech(plan: &SessionPlan, _source: Source) -> Result<Box<dyn SpeechClient>, SessionError> {
    let apple = system().ok_or(SessionError::ProviderConfigurationFailed)?;
    Ok(Box::new(AppleSpeech::new(
        apple,
        plan.source_language.clone(),
    )))
}

/// Text translator for a recognizing route (text kind apple).
pub fn translator(_plan: &SessionPlan) -> Result<Arc<dyn TextTranslator>, SessionError> {
    let apple = system().ok_or(SessionError::ProviderConfigurationFailed)?;
    Ok(Arc::new(AppleTranslator::new(apple.system)))
}

/// Resource operation `kind` of `sheng_resources_start` (apple.md §1).
pub async fn resource(kind: u32, source: String, target: String) -> resources::Outcome {
    resources::run(system(), kind, &source, &target).await
}
