//! Session request JSON (start-request.md). Both object levels reject unknown fields;
//! unknown enum values, missing fields and wrong types are parse failures.

use std::fmt;

use serde::{de, Deserialize, Deserializer};
use sheng_core::language::{is_source_language, is_target_language};

use super::secret::Secret;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartRequest {
    pub input: Input,
    pub configuration: Configuration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Input {
    System,
    Microphone,
    Both,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Configuration {
    pub provider: Provider,
    pub qwen_mt_model: QwenMtModel,
    pub credentials: Credentials,
    #[serde(default)]
    pub text_credentials: Option<TextCredentials>,
    #[serde(deserialize_with = "source_language")]
    pub source_language: String,
    #[serde(deserialize_with = "target_language")]
    pub target_language: String,
    pub translation_mode: TranslationMode,
    pub network_proxy: NetworkProxy,
    pub text_network_proxy: NetworkProxy,
    pub speech_translation_mode: SpeechTranslationMode,
    pub xai_speech_mode: XaiSpeechMode,
    #[serde(default)]
    pub alibaba_realtime_mode: AlibabaRealtimeMode,
    #[serde(default)]
    pub alibaba_workspace_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
pub enum Provider {
    #[serde(rename = "appleSpeech")]
    AppleSpeech,
    #[serde(rename = "alibabaCloud")]
    AlibabaCloud,
    #[serde(rename = "volcanoEngine")]
    VolcanoEngine,
    #[serde(rename = "googleGeminiLive")]
    GoogleGeminiLive,
    #[serde(rename = "tencentCloud")]
    TencentCloud,
    #[serde(rename = "baiduTranslate")]
    BaiduTranslate,
    #[serde(rename = "openAIRealtime")]
    OpenAiRealtime,
    #[serde(rename = "azureOpenAIRealtime")]
    AzureOpenAiRealtime,
    #[serde(rename = "xAIRealtime")]
    XaiRealtime,
    #[serde(rename = "customDashScopeASR")]
    CustomDashScopeAsr,
    #[serde(rename = "customOpenAIASR")]
    CustomOpenAiAsr,
}

impl Provider {
    pub fn id(self) -> &'static str {
        match self {
            Self::AppleSpeech => "appleSpeech",
            Self::AlibabaCloud => "alibabaCloud",
            Self::VolcanoEngine => "volcanoEngine",
            Self::GoogleGeminiLive => "googleGeminiLive",
            Self::TencentCloud => "tencentCloud",
            Self::BaiduTranslate => "baiduTranslate",
            Self::OpenAiRealtime => "openAIRealtime",
            Self::AzureOpenAiRealtime => "azureOpenAIRealtime",
            Self::XaiRealtime => "xAIRealtime",
            Self::CustomDashScopeAsr => "customDashScopeASR",
            Self::CustomOpenAiAsr => "customOpenAIASR",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum QwenMtModel {
    Lite,
    Plus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TranslationMode {
    Turbo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SpeechTranslationMode {
    #[default]
    Simultaneous,
    AsrMt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum XaiSpeechMode {
    #[default]
    Voice,
    AsrMt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AlibabaRealtimeMode {
    LiveTranslate,
    #[default]
    Disabled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProxyMode {
    System,
    Direct,
    Custom,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetworkProxy {
    pub mode: ProxyMode,
    #[serde(default)]
    pub url: Option<String>,
}

/// Speech service credentials. `Debug` shows only the kind.
#[derive(Clone, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum Credentials {
    #[serde(rename = "appleSpeech")]
    AppleSpeech,
    #[serde(rename = "apiKey", rename_all = "camelCase")]
    ApiKey { api_key: Secret },
    #[serde(rename = "azureOpenAI", rename_all = "camelCase")]
    AzureOpenAi {
        endpoint: Secret,
        deployment: String,
        transcription_deployment: String,
        api_key: Secret,
    },
    #[serde(rename = "tencentCloud", rename_all = "camelCase")]
    TencentCloud {
        app_id: Secret,
        secret_id: Secret,
        secret_key: Secret,
    },
    #[serde(rename = "baiduTranslate", rename_all = "camelCase")]
    BaiduTranslate { app_id: Secret, app_key: Secret },
    #[serde(rename = "customSpeech", rename_all = "camelCase")]
    CustomSpeech {
        endpoint: Secret,
        model: String,
        api_key: Secret,
    },
}

impl Credentials {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::AppleSpeech => "appleSpeech",
            Self::ApiKey { .. } => "apiKey",
            Self::AzureOpenAi { .. } => "azureOpenAI",
            Self::TencentCloud { .. } => "tencentCloud",
            Self::BaiduTranslate { .. } => "baiduTranslate",
            Self::CustomSpeech { .. } => "customSpeech",
        }
    }
}

impl fmt::Debug for Credentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Credentials")
            .field("kind", &self.kind())
            .finish_non_exhaustive()
    }
}

/// Independent text translation credentials. `Debug` shows only the kind.
#[derive(Clone, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum TextCredentials {
    #[serde(rename = "apple")]
    Apple,
    #[serde(rename = "deepL", rename_all = "camelCase")]
    DeepL { api_key: Secret },
    #[serde(rename = "deepLX")]
    DeepLx { endpoint: Secret, token: Secret },
    #[serde(rename = "xAI", rename_all = "camelCase")]
    Xai { api_key: Secret, model: String },
    #[serde(rename = "openAICompatible", rename_all = "camelCase")]
    OpenAiCompatible {
        endpoint: Secret,
        model: String,
        api_key: Secret,
    },
    #[serde(rename = "chatMock", rename_all = "camelCase")]
    ChatMock {
        endpoint: Secret,
        model: String,
        api_key: Secret,
    },
}

/// Text translator families; also used by capability queries (`textTranslation`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
pub enum TextKind {
    #[serde(rename = "apple")]
    Apple,
    #[serde(rename = "deepL")]
    DeepL,
    #[serde(rename = "deepLX")]
    DeepLx,
    #[serde(rename = "xAI")]
    Xai,
    #[serde(rename = "openAICompatible")]
    OpenAiCompatible,
    #[serde(rename = "chatMock")]
    ChatMock,
}

impl TextCredentials {
    pub fn kind(&self) -> TextKind {
        match self {
            Self::Apple => TextKind::Apple,
            Self::DeepL { .. } => TextKind::DeepL,
            Self::DeepLx { .. } => TextKind::DeepLx,
            Self::Xai { .. } => TextKind::Xai,
            Self::OpenAiCompatible { .. } => TextKind::OpenAiCompatible,
            Self::ChatMock { .. } => TextKind::ChatMock,
        }
    }
}

impl fmt::Debug for TextCredentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TextCredentials")
            .field("kind", &self.kind())
            .finish_non_exhaustive()
    }
}

fn source_language<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let code = String::deserialize(deserializer)?;
    if !is_source_language(&code) {
        return Err(de::Error::custom("unknown source language"));
    }
    Ok(code)
}

fn target_language<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let code = String::deserialize(deserializer)?;
    if !is_target_language(&code) {
        return Err(de::Error::custom("unknown target language"));
    }
    Ok(code)
}
