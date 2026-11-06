//! Which speech service path and which translation a provider/mode combination uses.

use super::request::{
    AlibabaRealtimeMode, Provider, QwenMtModel, SpeechTranslationMode, TextKind, XaiSpeechMode,
};

/// Speech service path. Translating routes produce finished pairs themselves;
/// recognizing routes only produce originals and need a text translator for targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SpeechRoute {
    AppleSpeech,
    AlibabaAsr,
    AlibabaLiveTranslate,
    Volcano,
    Tencent,
    Baidu,
    OpenAiTranscribe,
    OpenAiTranslate,
    AzureTranscribe,
    AzureTranslate,
    GeminiTranscribe,
    GeminiTranslate,
    XaiStt,
    XaiVoice,
    CustomDashScope,
    CustomOpenAi,
}

impl SpeechRoute {
    /// True when the service itself returns translations.
    pub fn translates(self) -> bool {
        matches!(
            self,
            Self::AlibabaLiveTranslate
                | Self::Volcano
                | Self::Tencent
                | Self::Baidu
                | Self::OpenAiTranslate
                | Self::AzureTranslate
                | Self::GeminiTranslate
                | Self::XaiVoice
        )
    }

    /// Input sample rate the service expects (providers/README.md).
    pub fn sample_rate(self) -> u32 {
        match self {
            Self::OpenAiTranscribe
            | Self::OpenAiTranslate
            | Self::AzureTranscribe
            | Self::AzureTranslate
            | Self::XaiStt
            | Self::XaiVoice
            | Self::CustomOpenAi => 24_000,
            _ => 16_000,
        }
    }
}

/// Mode fields shared by start requests and capability queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RouteModes {
    pub speech_translation: SpeechTranslationMode,
    pub xai_speech: XaiSpeechMode,
    pub alibaba_realtime: AlibabaRealtimeMode,
}

/// Providers whose speech model can either translate directly or only transcribe.
fn has_speech_translation_choice(provider: Provider) -> bool {
    matches!(
        provider,
        Provider::OpenAiRealtime | Provider::AzureOpenAiRealtime | Provider::GoogleGeminiLive
    )
}

/// Resolves the speech route; `None` for an invalid profile combination
/// (`speechTranslationMode:"asrMt"` on a provider without that choice).
pub fn speech_route(provider: Provider, modes: RouteModes) -> Option<SpeechRoute> {
    let asr_mt = modes.speech_translation == SpeechTranslationMode::AsrMt;
    if asr_mt && !has_speech_translation_choice(provider) {
        return None;
    }
    let route = match provider {
        Provider::AppleSpeech => SpeechRoute::AppleSpeech,
        Provider::AlibabaCloud => match modes.alibaba_realtime {
            AlibabaRealtimeMode::LiveTranslate => SpeechRoute::AlibabaLiveTranslate,
            AlibabaRealtimeMode::Disabled => SpeechRoute::AlibabaAsr,
        },
        Provider::VolcanoEngine => SpeechRoute::Volcano,
        Provider::TencentCloud => SpeechRoute::Tencent,
        Provider::BaiduTranslate => SpeechRoute::Baidu,
        Provider::OpenAiRealtime if asr_mt => SpeechRoute::OpenAiTranscribe,
        Provider::OpenAiRealtime => SpeechRoute::OpenAiTranslate,
        Provider::AzureOpenAiRealtime if asr_mt => SpeechRoute::AzureTranscribe,
        Provider::AzureOpenAiRealtime => SpeechRoute::AzureTranslate,
        Provider::GoogleGeminiLive if asr_mt => SpeechRoute::GeminiTranscribe,
        Provider::GoogleGeminiLive => SpeechRoute::GeminiTranslate,
        Provider::XaiRealtime => match modes.xai_speech {
            XaiSpeechMode::AsrMt => SpeechRoute::XaiStt,
            XaiSpeechMode::Voice => SpeechRoute::XaiVoice,
        },
        Provider::CustomDashScopeAsr => SpeechRoute::CustomDashScope,
        Provider::CustomOpenAiAsr => SpeechRoute::CustomOpenAi,
    };
    Some(route)
}

/// Translation used for a recognizing route.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextRoute {
    /// Nothing translates; only `original` is a valid target.
    None,
    Independent(TextKind),
    /// Alibaba's own Qwen-MT ("followService" in the UI).
    QwenMt(QwenMtModel),
}

/// Text route for a recognizing speech route given the configured translator, if any.
pub fn text_route(
    route: SpeechRoute,
    configured: Option<TextKind>,
    qwen: QwenMtModel,
) -> TextRoute {
    if route.translates() {
        return TextRoute::None;
    }
    match (configured, route) {
        (Some(kind), _) => TextRoute::Independent(kind),
        (None, SpeechRoute::AlibabaAsr) => TextRoute::QwenMt(qwen),
        (None, _) => TextRoute::None,
    }
}
