import Foundation

public enum AudioInput: String, Codable, CaseIterable, Identifiable {
    case system, microphone, both
    public var id: String { rawValue }
    public var title: String { switch self { case .system: return L10n.text("系统声音"); case .microphone: return L10n.text("麦克风"); case .both: return L10n.text("系统声音与麦克风") } }
}
public enum AudioSource: String, Codable { case system, microphone
    public var title: String { L10n.text(self == .system ? "系统声音" : "麦克风") }
    var lane: UInt32 { self == .system ? 0 : 1 }
}
public struct Caption: Codable, Identifiable, Equatable {
    public let id: String
    public let source: AudioSource
    public var original: String
    public var translation: String
    public let timestampMs: UInt64
    public let sourceLanguage: String
    public let targetLanguage: String
}
public struct CaptionTrack: Codable {
    public let source: AudioSource
    public let original: String
    public let translation: String
    public let pendingOriginal: String
    public let isFinal: Bool
}
public struct EngineSnapshot: Codable {
    public let `protocol`: Int
    public let revision: UInt64
    public let generation: UInt64
    public let runId: String
    public let status: String
    public let error: String?
    public let sampleRate: UInt32
    public let settled: Bool
    public let tracks: [CaptionTrack]
    public let recent: [Caption]
    public var diagnostics: [LaneDiagnostics]? = nil
    public var echo: EchoDiagnostics? = nil
}
public struct LaneDiagnostics: Codable {
    public let source: AudioSource
    public let apiLatencyMs: UInt64?
    public let translationLatencyMs: UInt64?
    public let translationLatencyKind: String?
    public let translationPending: Bool
    public let recovery: TranslationRecoveryStatus?
}
public struct TranslationRecoveryStatus: Codable {
    public let reason: String
    public let retryScheduled: Bool
}
public enum Product: String { case shengJian, shengXi
    public var name: String { L10n.text(self == .shengJian ? "声笺" : "声席") }
    public var bundleID: String { self == .shengJian ? "app.xlle.shengjian" : "app.xlle.shengxi" }
}
public struct ServiceProfile: Codable, Identifiable, Equatable {
    public var id = UUID()
    public var name = "Apple 本地"
    public var provider = "appleSpeech"
    public var endpoint = ""
    public var model = ""
    public var appID = ""
    public var deployment = ""
    public var transcriptionDeployment = ""
    public var workspaceID = ""
    public var speechMode = "asrMt"
    public var textProvider = "apple"
    public var textEndpoint = ""
    public var textModel = ""
    public var proxyMode = "system"
    public var proxyURL = ""
    public var qwenMtModel = "lite"
    public var presetSource: String? = nil
    public var presetTarget: String? = nil
    public var customSpeechLanguages: [String]? = nil
    public var textProxyMode: String? = nil
    public var textProxyURL: String? = nil
    /// Quick source → target choices for this profile; the first is applied when the profile is put in use.
    public var languagePresets: [LanguagePair] = []
    public init() {}
    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id = try c.decodeIfPresent(UUID.self, forKey: .id) ?? UUID()
        name = try c.decodeIfPresent(String.self, forKey: .name) ?? "Apple 本地"
        provider = try c.decodeIfPresent(String.self, forKey: .provider) ?? "appleSpeech"
        endpoint = try c.decodeIfPresent(String.self, forKey: .endpoint) ?? ""
        model = try c.decodeIfPresent(String.self, forKey: .model) ?? ""
        appID = try c.decodeIfPresent(String.self, forKey: .appID) ?? ""
        deployment = try c.decodeIfPresent(String.self, forKey: .deployment) ?? ""
        transcriptionDeployment = try c.decodeIfPresent(String.self, forKey: .transcriptionDeployment) ?? ""
        workspaceID = try c.decodeIfPresent(String.self, forKey: .workspaceID) ?? ""
        speechMode = try c.decodeIfPresent(String.self, forKey: .speechMode) ?? "asrMt"
        textProvider = try c.decodeIfPresent(String.self, forKey: .textProvider) ?? "apple"
        textEndpoint = try c.decodeIfPresent(String.self, forKey: .textEndpoint) ?? ""
        textModel = try c.decodeIfPresent(String.self, forKey: .textModel) ?? ""
        proxyMode = try c.decodeIfPresent(String.self, forKey: .proxyMode) ?? "system"
        proxyURL = try c.decodeIfPresent(String.self, forKey: .proxyURL) ?? ""
        qwenMtModel = try c.decodeIfPresent(String.self, forKey: .qwenMtModel) ?? "lite"
        presetSource = try c.decodeIfPresent(String.self, forKey: .presetSource)
        presetTarget = try c.decodeIfPresent(String.self, forKey: .presetTarget)
        customSpeechLanguages = try c.decodeIfPresent([String].self, forKey: .customSpeechLanguages)
        textProxyMode = try c.decodeIfPresent(String.self, forKey: .textProxyMode)
        textProxyURL = try c.decodeIfPresent(String.self, forKey: .textProxyURL)
        let single = presetSource.flatMap { source in presetTarget.map { [LanguagePair(source: source, target: $0)] } } ?? []
        let presets = try c.decodeIfPresent([LanguagePair].self, forKey: .languagePresets) ?? []
        // The single legacy preset is cleared whenever presets are edited, so it only fills an empty list.
        languagePresets = presets.isEmpty ? single : presets
    }
    public static let providers: [(String, String)] = [
        ("appleSpeech", "Apple 本地"), ("alibabaCloud", "阿里云"), ("volcanoEngine", "火山引擎"),
        ("googleGeminiLive", "Google Gemini"), ("tencentCloud", "腾讯云"), ("baiduTranslate", "百度翻译"),
        ("openAIRealtime", "OpenAI Realtime"), ("azureOpenAIRealtime", "Azure OpenAI"), ("xAIRealtime", "xAI"),
        ("customDashScopeASR", "自定义 DashScope 识别"), ("customOpenAIASR", "自定义 OpenAI 识别")
    ]
    public static let translators: [(String, String)] = [("apple", "Apple 本地"), ("followService", "跟随识别服务"), ("openAICompatible", "OpenAI 兼容"), ("deepL", "DeepL"), ("deepLX", "DeepLX"), ("xAI", "xAI"), ("chatMock", "ChatMock")]
    public var usesIndependentText: Bool {
        if ["appleSpeech", "customDashScopeASR", "customOpenAIASR"].contains(provider) { return true }
        return speechMode == "asrMt" && ["alibabaCloud", "openAIRealtime", "azureOpenAIRealtime", "googleGeminiLive", "xAIRealtime"].contains(provider)
    }
    /// Only these realtime providers choose between ASR+MT and simultaneous translation; capability queries must send the same value.
    var speechTranslationMode: String { ["openAIRealtime", "googleGeminiLive", "azureOpenAIRealtime"].contains(provider) ? speechMode : "simultaneous" }
    public func request(input: AudioInput, source: String, target: String, secrets: ServiceCredentials = ServiceCredentials()) throws -> Data {
        let apiKey = secrets.apiKey, secretID = secrets.secretID, secretKey = secrets.secretKey, textKey = secrets.textKey
        var credentials: [String: Any] = ["kind": "apiKey", "apiKey": apiKey]
        switch provider {
        case "appleSpeech": credentials = ["kind": "appleSpeech"]
        case "customDashScopeASR", "customOpenAIASR": credentials = ["kind": "customSpeech", "endpoint": endpoint, "model": model, "apiKey": apiKey]
        case "azureOpenAIRealtime": credentials = ["kind": "azureOpenAI", "endpoint": endpoint, "deployment": deployment, "transcriptionDeployment": transcriptionDeployment, "apiKey": apiKey]
        case "tencentCloud": credentials = ["kind": "tencentCloud", "appId": appID, "secretId": secretID, "secretKey": secretKey]
        case "baiduTranslate": credentials = ["kind": "baiduTranslate", "appId": appID, "appKey": apiKey]
        default: break
        }
        var text: [String: Any]? = nil
        if target != "original" && usesIndependentText && textProvider != "followService" {
            switch textProvider {
            case "apple": text = ["kind": "apple"]
            case "deepL": text = ["kind": "deepL", "apiKey": textKey]
            case "deepLX": text = ["kind": "deepLX", "endpoint": textEndpoint, "token": textKey]
            case "xAI": text = ["kind": "xAI", "apiKey": textKey, "model": textModel.isEmpty ? "grok-4.3" : textModel]
            default: text = ["kind": textProvider, "endpoint": textEndpoint, "model": textModel, "apiKey": textKey]
            }
        }
        var proxy: [String: Any] = ["mode": proxyMode]
        if proxyMode == "custom" { proxy["url"] = proxyURL }
        var textProxy: [String: Any] = ["mode": textProxyMode ?? proxyMode]
        if (textProxyMode ?? proxyMode) == "custom" { textProxy["url"] = textProxyURL ?? proxyURL }
        var configuration: [String: Any] = ["provider": provider, "qwenMtModel": qwenMtModel, "credentials": credentials,
            "sourceLanguage": source, "targetLanguage": target, "translationMode": "turbo",
            "networkProxy": proxy, "textNetworkProxy": textProxy,
            "speechTranslationMode": speechTranslationMode, "xaiSpeechMode": provider == "xAIRealtime" && speechMode == "asrMt" ? "asrMt" : "voice", "alibabaWorkspaceId": workspaceID]
        if provider == "alibabaCloud" && speechMode == "simultaneous" { configuration["alibabaRealtimeMode"] = "liveTranslate" }
        if provider == "xAIRealtime" && speechMode == "simultaneous" { configuration["xaiSpeechMode"] = "voice" }
        if let text { configuration["textCredentials"] = text }
        return try JSONSerialization.data(withJSONObject: ["input": input.rawValue, "configuration": configuration])
    }
}

struct RunningAudioApplication: Identifiable { let id: Int32; let name: String }

public struct EchoDiagnostics: Codable {
    public let frames: UInt64
    public let matchedReferences: UInt64
    public let missingReferences: UInt64
    public let resets: UInt64
    public let inputRms: Double?
    public let outputRms: Double?
    public let referenceRms: Double?
    public let reductionDb: Double?
}
