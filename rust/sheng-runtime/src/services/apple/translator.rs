//! Apple translation for `textCredentials.kind:"apple"` (translation-pipeline.md). Each
//! request is one sentence; the bridge opens a hidden host for it and never downloads.

use std::sync::Arc;

use tokio::sync::OnceCell;

use super::locale::map_locale;
use super::system::{AppleSystem, TranslateFailure};
use crate::provider::{BoxFuture, ProviderError, TextTranslator, TranslationRequest};

pub struct AppleTranslator {
    system: Arc<dyn AppleSystem>,
    languages: OnceCell<Option<Vec<String>>>,
}

impl AppleTranslator {
    pub fn new(system: Arc<dyn AppleSystem>) -> Self {
        Self {
            system,
            languages: OnceCell::new(),
        }
    }

    async fn pair(&self, source: &str, target: &str) -> Option<(String, String)> {
        let languages = self
            .languages
            .get_or_init(|| self.system.translation_languages())
            .await
            .as_ref()?;
        let source = map_locale(source, languages)?;
        let target = map_locale(target, languages)?;
        Some((source.to_owned(), target.to_owned()))
    }
}

/// Missing or unsupported languages need the user to change the configuration; other
/// failures are temporary and retried by the pipeline.
fn provider_error(failure: TranslateFailure) -> ProviderError {
    match failure {
        TranslateFailure::Unavailable
        | TranslateFailure::Unsupported
        | TranslateFailure::AssetsMissing => ProviderError::Configuration,
        TranslateFailure::Failed => ProviderError::Unavailable { retry_after: None },
    }
}

impl TextTranslator for AppleTranslator {
    fn translate(
        &self,
        request: TranslationRequest,
    ) -> BoxFuture<'_, Result<String, ProviderError>> {
        Box::pin(async move {
            let (source, target) = self
                .pair(&request.source_language, &request.target_language)
                .await
                .ok_or(ProviderError::Configuration)?;
            self.system
                .translate(request.text, source, target)
                .await
                .map_err(provider_error)
        })
    }
}
