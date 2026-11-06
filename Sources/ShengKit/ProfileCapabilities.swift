import Foundation
import CShengEngine

public struct ProfileValidationIssue: Codable, Equatable { public let field: String; public let code: String }
public struct ProfileCapabilities: Codable {
    public let sources: [String]
    public let targets: [String]
    public let modes: [String]
    public let issues: [ProfileValidationIssue]
}
public struct ServiceCredentials: Codable, Equatable {
    public var apiKey = "", secretID = "", secretKey = "", textKey = ""
    public init() {}
}
struct ProviderLink: Decodable, Identifiable {
    let id: String
    let label: String
    let apiKeyUrl: String
    let guideUrl: String
    let endpoint: String?
    let model: String?
}
struct ProviderCatalog: Decodable {
    let presets: [ProviderLink]
    let services: [ProviderLink]
    static var bundled: ProviderCatalog {
        guard let pointer = sheng_provider_catalog() else { return ProviderCatalog(presets: [], services: []) }
        defer { sheng_string_free(pointer) }
        return (try? JSONDecoder().decode(Self.self, from: Data(String(cString: pointer).utf8))) ?? ProviderCatalog(presets: [], services: [])
    }
}
extension ServiceProfile {
    public func capabilities(source: String, target: String) throws -> ProfileCapabilities {
        var metadata: [String: Any] = ["id": id.uuidString, "name": name, "provider": provider,
            "qwenMtModel": qwenMtModel, "speechTranslationMode": speechTranslationMode,
            "xaiSpeechMode": provider == "xAIRealtime" && speechMode == "asrMt" ? "asrMt" : "voice",
            "alibabaRealtimeMode": provider == "alibabaCloud" && speechMode == "simultaneous" ? "liveTranslate" : "disabled"]
        if usesIndependentText { metadata["textTranslation"] = textProvider }
        if let languages = customSpeechLanguages { metadata["customSpeechSourceLanguages"] = languages }
        let data = try JSONSerialization.data(withJSONObject: ["profile": metadata, "source": source, "target": target], options: .sortedKeys)
        let request = String(decoding: data, as: UTF8.self)
        if let cached = ProfileCapabilityCache.shared.value(for: request) { return cached }
        let value = try request.withCString { request in
            guard let pointer = sheng_profile_capabilities(request) else { throw EngineError(code: "configuration_invalid") }
            defer { sheng_string_free(pointer) }
            return try JSONDecoder().decode(ProfileCapabilities.self, from: Data(String(cString: pointer).utf8))
        }
        ProfileCapabilityCache.shared.store(value, for: request)
        return value
    }
    public func requireSupported(source: String, target: String) throws {
        guard try capabilities(source: source, target: target).issues.isEmpty else { throw EngineError(code: "configuration_invalid") }
    }
    public func duplicate() -> ServiceProfile {
        var copy = self; copy.id = UUID(); copy.name = String(name.prefix(55)) + L10n.text(" 副本"); return copy
    }
}
extension SessionModel {
    public var selectedService: ServiceProfile? { profiles.first { $0.id == selectedProfile } }
    public func languageOptions(source: Bool) -> [(String, String)] {
        guard let profile = effectiveService, let caps = try? profile.capabilities(source: sourceLanguage, target: targetLanguage) else { return [] }
        let codes = (source ? caps.sources : caps.targets).filter { code in
            guard source, profile.provider == "appleSpeech", let report = appleResources else { return true }
            return report.available && report.languages.contains { $0.code == code }
        }
        return codes.map { code in
            (code, (code == "auto" ? L10n.text("自动") : code == "original" ? L10n.text("仅原文") : locale.localizedString(forIdentifier: code.replacingOccurrences(of: "_", with: "-")) ?? code) + (source && profile.provider == "appleSpeech" && appleResources?.languages.first(where: { $0.code == code })?.status != "installed" && appleResources != nil ? " · " + L10n.text("可下载") : ""))
        }
    }
    public func selectService(_ id: UUID) async {
        guard let p = profiles.first(where: { $0.id == id }) else { return }
        let preset = p.languagePresets.first
        await reconfigure(profileID: id, source: preset?.source ?? sourceLanguage, target: preset?.target ?? targetLanguage)
    }
}
