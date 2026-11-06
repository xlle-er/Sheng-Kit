import XCTest
@testable import ShengKit

final class TranslationSplitTests: XCTestCase {
    private func folder() throws -> URL {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: directory) }; return directory
    }
    private func request(_ data: Data) throws -> [String: Any] {
        let json = try JSONSerialization.jsonObject(with: data) as? [String: Any] ?? [:]
        return json["configuration"] as? [String: Any] ?? [:]
    }

    func testCombiningCopiesTheTranslationProfileIntoTheSpeechProfile() {
        var translation = TranslationProfile(); translation.kind = "openAICompatible"
        translation.endpoint = "https://example.test/v1/chat/completions"; translation.model = "test-model"
        translation.proxyMode = "custom"; translation.proxyURL = "http://127.0.0.1:9"
        let combined = ServiceProfile().combined(with: translation)
        XCTAssertEqual(combined.textProvider, "openAICompatible")
        XCTAssertEqual(combined.textEndpoint, translation.endpoint)
        XCTAssertEqual(combined.textModel, "test-model")
        XCTAssertEqual(combined.textProxyMode, "custom")
        XCTAssertEqual(combined.textProxyURL, "http://127.0.0.1:9")
        XCTAssertEqual(ServiceProfile().combined(with: nil).textProvider, "followService")
    }

    @MainActor func testExistingCombinedProfilesAreSplitOnceIntoTranslationProfiles() throws {
        let root = try folder(), store = try PrivateStore(product: .shengJian, root: root)
        var speech = ServiceProfile(); speech.name = "Cloud"; speech.provider = "openAIRealtime"; speech.speechMode = "asrMt"
        speech.textProvider = "deepL"
        var secrets = ServiceCredentials(); secrets.apiKey = "speech-key"; secrets.textKey = "text-key"
        try store.saveProfiles([speech]); try store.saveCredentials(secrets, for: speech.id)

        let model = SessionModel(product: .shengJian, store: store)

        let deepL = try XCTUnwrap(model.translationProfiles.first { $0.kind == "deepL" })
        XCTAssertTrue(model.translationProfiles.contains { $0.kind == "apple" })
        XCTAssertEqual(try store.credentials(for: deepL.id).apiKey, "text-key")
        XCTAssertEqual(model.selectedTranslation, deepL.id.uuidString)
        let reloaded = SessionModel(product: .shengJian, store: store)
        XCTAssertEqual(reloaded.translationProfiles.count, model.translationProfiles.count)
    }

    @MainActor func testSessionRequestUsesTheSelectedTranslationProfileAndItsKey() throws {
        let root = try folder(), store = try PrivateStore(product: .shengJian, root: root)
        let model = SessionModel(product: .shengJian, store: store)
        var speech = ServiceProfile(); speech.provider = "openAIRealtime"; speech.speechMode = "asrMt"
        var speechKey = ServiceCredentials(); speechKey.apiKey = "speech-key"
        var translation = TranslationProfile(); translation.name = "DeepL"; translation.kind = "deepL"
        var textKey = ServiceCredentials(); textKey.apiKey = "text-key"
        try store.saveCredentials(speechKey, for: speech.id); try store.saveCredentials(textKey, for: translation.id)
        model.profiles = [speech]; model.selectedProfile = speech.id
        model.translationProfiles = [translation]; model.selectedTranslation = translation.id.uuidString

        let effective = try XCTUnwrap(model.effectiveService)
        let configuration = try request(effective.request(input: .system, source: "auto", target: "zh", secrets: model.effectiveCredentials(for: speech)))

        let text = try XCTUnwrap(configuration["textCredentials"] as? [String: Any])
        XCTAssertEqual(text["kind"] as? String, "deepL")
        XCTAssertEqual(text["apiKey"] as? String, "text-key")
    }

    @MainActor func testAProfileIsSavedEvenWhenTheCurrentLanguagePairIsUnsupported() throws {
        let root = try folder(), store = try PrivateStore(product: .shengJian, root: root)
        let model = SessionModel(product: .shengJian, store: store)
        model.sourceLanguage = "zh"; model.targetLanguage = "en"
        var gemini = ServiceProfile(); gemini.name = "Gemini"; gemini.provider = "googleGeminiLive"; gemini.speechMode = "simultaneous"
        XCTAssertNotEqual(gemini.readiness(credentials: ServiceCredentials(), source: "zh", target: "en"), .ready)

        try model.saveSpeechProfile(gemini, credentials: ServiceCredentials())

        XCTAssertTrue(try store.loadProfiles().contains { $0.id == gemini.id })
    }

    func testLanguagePresetsRoundTripAndAdoptTheOldSinglePreset() throws {
        var profile = ServiceProfile()
        profile.languagePresets = [LanguagePair(source: "en", target: "zh"), LanguagePair(source: "ja", target: "zh")]
        let decoded = try JSONDecoder().decode(ServiceProfile.self, from: JSONEncoder().encode(profile))
        XCTAssertEqual(decoded.languagePresets, profile.languagePresets)

        var legacy = ServiceProfile(); legacy.presetSource = "en"; legacy.presetTarget = "zh"
        let adopted = try JSONDecoder().decode(ServiceProfile.self, from: JSONEncoder().encode(legacy))
        XCTAssertEqual(adopted.languagePresets, [LanguagePair(source: "en", target: "zh")])
    }

    @MainActor func testReadinessAsksForATranslationProfileWhenTheRouteNeedsOne() throws {
        let root = try folder()
        let model = SessionModel(product: .shengJian, store: try PrivateStore(product: .shengJian, root: root))
        model.translationProfiles = []; model.selectedTranslation = ""
        XCTAssertEqual(model.readiness(of: ServiceProfile()), .missingTranslation)
    }
}
