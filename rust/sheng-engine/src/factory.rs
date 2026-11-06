//! Creation of service clients for a validated plan.
//!
//! `ServiceFactory` dispatches each provider and text translator to its module under
//! `sheng_runtime::services`. Modules whose protocol is not implemented yet report
//! `provider_configuration_failed` (docs/verification.md). Tests install scripted clients
//! through the `test-support` feature.

use std::sync::{Arc, PoisonError, RwLock};

use sheng_core::Source;
use sheng_runtime::config::request::{Provider, TextKind};
use sheng_runtime::config::route::TextRoute;
use sheng_runtime::config::SessionPlan;
use sheng_runtime::provider::{SessionError, SpeechClient, TextTranslator};
use sheng_runtime::services;

pub trait ClientFactory: Send + Sync {
    /// Speech client for one source's lane.
    fn speech(
        &self,
        plan: &SessionPlan,
        source: Source,
    ) -> Result<Box<dyn SpeechClient>, SessionError>;

    /// Independent text translator for a recognizing route with a translation target.
    fn translator(&self, plan: &SessionPlan) -> Result<Arc<dyn TextTranslator>, SessionError>;
}

/// Factory of the real service clients.
#[derive(Debug, Default)]
pub struct ServiceFactory;

impl ClientFactory for ServiceFactory {
    fn speech(
        &self,
        plan: &SessionPlan,
        source: Source,
    ) -> Result<Box<dyn SpeechClient>, SessionError> {
        match plan.provider {
            Provider::AppleSpeech => services::apple::speech(plan, source),
            Provider::OpenAiRealtime
            | Provider::AzureOpenAiRealtime
            | Provider::CustomOpenAiAsr => services::openai::speech(plan, source),
            Provider::XaiRealtime => services::xai::speech(plan, source),
            Provider::GoogleGeminiLive => services::gemini::speech(plan, source),
            Provider::AlibabaCloud | Provider::CustomDashScopeAsr => {
                services::alibaba::speech(plan, source)
            }
            Provider::TencentCloud => services::tencent::speech(plan, source),
            Provider::BaiduTranslate => services::baidu::speech(plan, source),
            Provider::VolcanoEngine => services::volcano::speech(plan, source),
        }
    }

    fn translator(&self, plan: &SessionPlan) -> Result<Arc<dyn TextTranslator>, SessionError> {
        match plan.text_route {
            TextRoute::Independent(TextKind::Apple) => services::apple::translator(plan),
            TextRoute::Independent(
                TextKind::Xai
                | TextKind::OpenAiCompatible
                | TextKind::ChatMock
                | TextKind::DeepL
                | TextKind::DeepLx,
            ) => services::text::translator(plan),
            TextRoute::QwenMt(_) => services::alibaba::translator(plan),
            TextRoute::None => Err(SessionError::ProviderConfigurationFailed),
        }
    }
}

static OVERRIDE: RwLock<Option<Arc<dyn ClientFactory>>> = RwLock::new(None);

/// Factory used by new handles and probes.
pub fn default_factory() -> Arc<dyn ClientFactory> {
    OVERRIDE
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
        .unwrap_or_else(|| Arc::new(ServiceFactory))
}

#[cfg(feature = "test-support")]
pub(crate) fn set_default_factory(factory: Option<Arc<dyn ClientFactory>>) {
    *OVERRIDE.write().unwrap_or_else(PoisonError::into_inner) = factory;
}
