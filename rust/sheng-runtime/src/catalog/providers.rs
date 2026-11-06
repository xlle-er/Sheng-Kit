//! `sheng_provider_catalog` (catalog.md): presets for OpenAI-compatible text translation and
//! the sign-up pages of each speech service. Written from the vendors' official sites; no
//! credentials. Model names are editable defaults.

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preset {
    pub id: &'static str,
    pub label: &'static str,
    pub api_key_url: &'static str,
    pub guide_url: &'static str,
    pub endpoint: &'static str,
    pub model: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub setup_kind: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub setup_help_key: Option<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceLink {
    pub id: &'static str,
    pub label: &'static str,
    pub api_key_url: &'static str,
    pub guide_url: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProviderCatalog {
    pub presets: Vec<Preset>,
    pub services: Vec<ServiceLink>,
}

const fn hosted(
    id: &'static str,
    label: &'static str,
    api_key_url: &'static str,
    guide_url: &'static str,
    endpoint: &'static str,
    model: &'static str,
) -> Preset {
    Preset {
        id,
        label,
        api_key_url,
        guide_url,
        endpoint,
        model,
        setup_kind: None,
        setup_help_key: None,
    }
}

const PRESETS: &[Preset] = &[
    hosted(
        "openAI",
        "OpenAI",
        "https://platform.openai.com/api-keys",
        "https://platform.openai.com/docs/api-reference/chat",
        "https://api.openai.com/v1",
        "gpt-4.1-mini",
    ),
    hosted(
        "deepSeek",
        "DeepSeek",
        "https://platform.deepseek.com/api_keys",
        "https://api-docs.deepseek.com/",
        "https://api.deepseek.com",
        "deepseek-chat",
    ),
    hosted(
        "dashScope",
        "阿里云百炼",
        "https://bailian.console.aliyun.com/",
        "https://help.aliyun.com/zh/model-studio/get-api-key",
        "https://dashscope.aliyuncs.com/compatible-mode/v1",
        "qwen-mt-flash",
    ),
    hosted(
        "gemini",
        "Google Gemini",
        "https://aistudio.google.com/apikey",
        "https://ai.google.dev/gemini-api/docs/openai",
        "https://generativelanguage.googleapis.com/v1beta/openai",
        "gemini-2.5-flash",
    ),
    Preset {
        id: "ollama",
        label: "Ollama",
        api_key_url: "https://docs.ollama.com/api/openai-compatibility",
        guide_url: "https://docs.ollama.com/api/openai-compatibility",
        endpoint: "http://127.0.0.1:11434/v1",
        model: "qwen3:8b",
        setup_kind: Some("localServer"),
        setup_help_key: Some("preset.ollama.setup"),
    },
];

const SERVICES: &[ServiceLink] = &[
    ServiceLink {
        id: "alibabaCloud",
        label: "阿里云百炼",
        api_key_url: "https://bailian.console.aliyun.com/",
        guide_url: "https://help.aliyun.com/zh/model-studio/get-api-key",
    },
    ServiceLink {
        id: "openAIRealtime",
        label: "OpenAI Realtime",
        api_key_url: "https://platform.openai.com/api-keys",
        guide_url: "https://platform.openai.com/docs/guides/realtime",
    },
    ServiceLink {
        id: "googleGeminiLive",
        label: "Google Gemini Live",
        api_key_url: "https://aistudio.google.com/apikey",
        guide_url: "https://ai.google.dev/gemini-api/docs/live",
    },
    ServiceLink {
        id: "azureOpenAIRealtime",
        label: "Azure OpenAI",
        api_key_url: "https://portal.azure.com/",
        guide_url: "https://learn.microsoft.com/azure/ai-foundry/openai/realtime-audio-reference",
    },
    ServiceLink {
        id: "volcanoEngine",
        label: "火山引擎同声传译",
        api_key_url: "https://console.volcengine.com/speech/app",
        guide_url: "https://www.volcengine.com/docs/6561/1756902",
    },
    ServiceLink {
        id: "tencentCloud",
        label: "腾讯云",
        api_key_url: "https://console.cloud.tencent.com/cam/capi",
        guide_url: "https://cloud.tencent.com/document/product/1093",
    },
    ServiceLink {
        id: "baiduTranslate",
        label: "百度智能云",
        api_key_url: "https://console.bce.baidu.com/ai/",
        guide_url: "https://ai.baidu.com/ai-doc/MT/Sl9p2h5k9",
    },
    ServiceLink {
        id: "xAIRealtime",
        label: "xAI",
        api_key_url: "https://console.x.ai/",
        guide_url: "https://docs.x.ai/developers/model-capabilities/audio/voice",
    },
    ServiceLink {
        id: "deepL",
        label: "DeepL",
        api_key_url: "https://www.deepl.com/your-account/keys",
        guide_url: "https://developers.deepl.com/docs/getting-started/quickstart",
    },
];

pub fn provider_catalog() -> ProviderCatalog {
    ProviderCatalog {
        presets: PRESETS.to_vec(),
        services: SERVICES.to_vec(),
    }
}
