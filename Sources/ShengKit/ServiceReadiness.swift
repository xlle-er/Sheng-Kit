import Foundation

/// What still blocks a profile from starting, shown on profile cards and the subtitle page.
public enum ServiceReadiness: Equatable {
    case ready, missingCredential, unsupportedLanguage, missingTranslation

    var title: String {
        switch self {
        case .ready: return "可以开始"
        case .missingCredential: return "缺少凭据"
        case .unsupportedLanguage: return "语言组合不受支持"
        case .missingTranslation: return "缺少翻译配置"
        }
    }
}

extension ServiceProfile {
    private static let optionalTextKeys: Set<String> = ["apple", "followService", "deepLX", "chatMock"]

    public func readiness(credentials: ServiceCredentials, source: String, target: String) -> ServiceReadiness {
        guard (try? capabilities(source: source, target: target).issues.isEmpty) == true else { return .unsupportedLanguage }
        switch provider {
        case "appleSpeech": break
        case "tencentCloud":
            if credentials.secretID.isEmpty || credentials.secretKey.isEmpty { return .missingCredential }
        default:
            if credentials.apiKey.isEmpty { return .missingCredential }
        }
        let needsTextKey = target != "original" && usesIndependentText && !Self.optionalTextKeys.contains(textProvider)
        return needsTextKey && credentials.textKey.isEmpty ? .missingCredential : .ready
    }

    /// Whether a key the speech service always needs is missing, independent of languages and translation.
    func lacksCredential(_ credentials: ServiceCredentials) -> Bool {
        switch provider {
        case "appleSpeech": return false
        case "tencentCloud": return credentials.secretID.isEmpty || credentials.secretKey.isEmpty
        default: return credentials.apiKey.isEmpty
        }
    }
    var providerName: String { Self.providers.first { $0.0 == provider }?.1 ?? provider }
    var translatorName: String { Self.translators.first { $0.0 == textProvider }?.1 ?? textProvider }
}

extension SessionModel {
    /// Readiness of `profile` with the currently selected translation, for the current language pair.
    func readiness(of profile: ServiceProfile) -> ServiceReadiness {
        if profile.usesIndependentText && targetLanguage != "original" && translation(for: profile) == nil && !(profile.offersServiceTranslation && selectedTranslation == serviceTranslationSelection) {
            return .missingTranslation
        }
        let credentials = (try? effectiveCredentials(for: profile)) ?? ServiceCredentials()
        return effective(profile).readiness(credentials: credentials, source: sourceLanguage, target: targetLanguage)
    }
}
