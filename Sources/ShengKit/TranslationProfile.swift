import Foundation

/// A text-translation service configured on its own, combined with any speech profile at start.
public struct TranslationProfile: Codable, Identifiable, Equatable {
    public var id = UUID()
    public var name = "Apple 本地翻译"
    /// Same identifiers as the engine's `textCredentials.kind`: apple, openAICompatible, deepL, deepLX, xAI, chatMock.
    public var kind = "apple"
    public var endpoint = ""
    public var model = ""
    public var proxyMode = "system"
    public var proxyURL = ""
    public init() {}

    public static let kinds: [(String, String)] = ServiceProfile.translators.filter { $0.0 != "followService" }
    var kindName: String { Self.kinds.first { $0.0 == kind }?.1 ?? kind }
    var needsKey: Bool { ["openAICompatible", "deepL", "xAI"].contains(kind) }
    var hasEndpoint: Bool { ["openAICompatible", "deepLX", "chatMock"].contains(kind) }
    var hasModel: Bool { ["openAICompatible", "chatMock", "xAI"].contains(kind) }

    func duplicate() -> TranslationProfile {
        var copy = self; copy.id = UUID(); copy.name = String(name.prefix(55)) + L10n.text(" 副本"); return copy
    }
}

/// One saved source → target pair offered as a quick choice for a speech profile.
public struct LanguagePair: Codable, Equatable, Hashable {
    public var source: String
    public var target: String
    public init(source: String, target: String) { self.source = source; self.target = target }
}

extension ServiceProfile {
    /// Alibaba's recognition route can translate with its own Qwen-MT instead of a separate profile.
    var offersServiceTranslation: Bool { provider == "alibabaCloud" && speechMode == "asrMt" }

    /// The speech profile with the chosen translation copied in; `nil` means the speech service's own translation.
    func combined(with translation: TranslationProfile?) -> ServiceProfile {
        var profile = self
        guard let translation else { profile.textProvider = "followService"; return profile }
        profile.textProvider = translation.kind
        profile.textEndpoint = translation.endpoint
        profile.textModel = translation.model
        profile.textProxyMode = translation.proxyMode
        profile.textProxyURL = translation.proxyURL
        return profile
    }
}

/// Selection value meaning “use the speech service's own translation”.
public let serviceTranslationSelection = "service"

extension PrivateStore {
    private var translationFile: URL { root.appendingPathComponent("translation-profiles.json") }
    var hasTranslationProfiles: Bool { FileManager.default.fileExists(atPath: translationFile.path) }
    func loadTranslationProfiles() throws -> [TranslationProfile] {
        guard hasTranslationProfiles else { return [] }
        return try JSONDecoder().decode([TranslationProfile].self, from: Data(contentsOf: translationFile))
    }
    func saveTranslationProfiles(_ profiles: [TranslationProfile]) throws {
        try write(JSONEncoder().encode(profiles), name: "translation-profiles.json")
    }
}

extension SessionModel {
    public var selectedTranslationProfile: TranslationProfile? { translationProfiles.first { $0.id.uuidString == selectedTranslation } }

    /// The translation actually used with `speech`; `nil` when the speech service translates itself.
    func translation(for speech: ServiceProfile) -> TranslationProfile? {
        if speech.offersServiceTranslation && selectedTranslation == serviceTranslationSelection { return nil }
        return selectedTranslationProfile
    }
    public var effectiveService: ServiceProfile? { selectedService.map(effective) }
    func effective(_ speech: ServiceProfile) -> ServiceProfile { speech.combined(with: translation(for: speech)) }

    /// Speech credentials with the translation profile's key in the text slot the request builder reads.
    func effectiveCredentials(for speech: ServiceProfile) throws -> ServiceCredentials {
        var credentials = try store.credentials(for: speech.id)
        credentials.textKey = try translation(for: speech).map { try store.credentials(for: $0.id).apiKey } ?? ""
        return credentials
    }

    /// Saving never depends on the current language pair; unsupported pairs are reported, not blocked.
    public func saveSpeechProfile(_ profile: ServiceProfile, credentials: ServiceCredentials) throws {
        guard !profile.name.trimmingCharacters(in: .whitespaces).isEmpty, profile.name.count <= 64 else { throw EngineError(code: "configuration_invalid") }
        var next = profiles
        if let index = next.firstIndex(where: { $0.id == profile.id }) { next[index] = profile } else { next.append(profile) }
        try store.saveCredentials(credentials, for: profile.id)
        try store.saveProfiles(next)
        profiles = next
        if !profiles.contains(where: { $0.id == selectedProfile }) { selectedProfile = profile.id }
    }
    /// Stores a saved speech profile's language presets at once, leaving its other unsaved edits alone.
    public func saveLanguagePresets(_ presets: [LanguagePair], for id: UUID) throws {
        guard let index = profiles.firstIndex(where: { $0.id == id }) else { return }
        var next = profiles
        var profile = next[index]
        profile.languagePresets = presets
        profile.presetSource = nil; profile.presetTarget = nil
        next[index] = profile
        try store.saveProfiles(next)
        profiles = next
    }
    public func saveTranslationProfile(_ profile: TranslationProfile, key: String) throws {
        guard !profile.name.trimmingCharacters(in: .whitespaces).isEmpty, profile.name.count <= 64 else { throw EngineError(code: "configuration_invalid") }
        var next = translationProfiles
        if let index = next.firstIndex(where: { $0.id == profile.id }) { next[index] = profile } else { next.append(profile) }
        var credentials = ServiceCredentials(); credentials.apiKey = key
        try store.saveCredentials(credentials, for: profile.id)
        try store.saveTranslationProfiles(next)
        translationProfiles = next
        if selectedTranslationProfile == nil && selectedTranslation != serviceTranslationSelection { selectedTranslation = profile.id.uuidString }
    }
    public func deleteTranslationProfile(_ profile: TranslationProfile) throws {
        try store.deleteCredentials(for: profile.id)
        let next = translationProfiles.filter { $0.id != profile.id }
        try store.saveTranslationProfiles(next)
        translationProfiles = next
        if selectedTranslation == profile.id.uuidString { selectedTranslation = next.first?.id.uuidString ?? "" }
    }

    /// First run after the split: each distinct translation setting of the old combined profiles becomes its own
    /// profile (with its key moved over), next to a default Apple translation.
    func loadTranslationProfiles() {
        do {
            if store.hasTranslationProfiles {
                translationProfiles = try store.loadTranslationProfiles()
                if selectedTranslationProfile == nil && selectedTranslation != serviceTranslationSelection {
                    selectedTranslation = translationProfiles.first?.id.uuidString ?? ""
                }
                return
            }
            var created = [TranslationProfile()]
            var selection = created[0].id.uuidString
            for speech in profiles where speech.usesIndependentText && !["apple", "followService"].contains(speech.textProvider) {
                var translation = TranslationProfile()
                translation.name = speech.translatorName; translation.kind = speech.textProvider
                translation.endpoint = speech.textEndpoint; translation.model = speech.textModel
                translation.proxyMode = speech.textProxyMode ?? speech.proxyMode; translation.proxyURL = speech.textProxyURL ?? speech.proxyURL
                var key = ServiceCredentials(); key.apiKey = try store.credentials(for: speech.id).textKey
                try store.saveCredentials(key, for: translation.id)
                created.append(translation)
                if speech.id == selectedProfile || selection == created[0].id.uuidString { selection = translation.id.uuidString }
            }
            if let selected = profiles.first(where: { $0.id == selectedProfile }), selected.offersServiceTranslation, selected.textProvider == "followService" {
                selection = serviceTranslationSelection
            }
            try store.saveTranslationProfiles(created)
            translationProfiles = created; selectedTranslation = selection
        } catch { self.error = L10n.text("翻译配置读取失败，请重试。") }
    }
}
