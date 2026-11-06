import XCTest
@testable import ShengKit

final class FeatureCompletionTests: XCTestCase {
    private func folder() throws -> URL {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: directory) }; return directory
    }
    private func transcript(_ directory: URL, count: Int) throws -> URL {
        let file = directory.appendingPathComponent("captions.jsonl")
        var data = Data()
        for i in 0..<count {
            let caption = Caption(id: "synthetic-\(i)", source: i % 2 == 0 ? .system : .microphone, original: "Original-\(i)", translation: "Translation-\(i)", timestampMs: UInt64(i * 1000), sourceLanguage: "en", targetLanguage: "zh")
            data.append(try JSONEncoder().encode(caption)); data.append(10)
        }; try data.write(to: file); return file
    }
    func testMetadataEvolutionDoesNotReadCredentials() throws {
        let profile = try JSONDecoder().decode(ServiceProfile.self, from: Data("{\"name\":\"Existing\",\"proxyMode\":\"direct\"}".utf8))
        XCTAssertEqual(profile.qwenMtModel, "lite"); XCTAssertNil(profile.textProxyMode)
        var secrets = ServiceCredentials(); secrets.apiKey = "synthetic-speech"; secrets.textKey = "synthetic-text"
        let data = try profile.request(input: .system, source: "zh", target: "en", secrets: secrets)
        let value = try JSONSerialization.jsonObject(with: data) as! [String: Any]
        let config = value["configuration"] as! [String: Any]
        XCTAssertEqual((config["textNetworkProxy"] as! [String: Any])["mode"] as? String, "direct")
        XCTAssertFalse(String(decoding: try JSONEncoder().encode(profile), as: UTF8.self).contains("synthetic"))
    }
    func testIndependentProxiesAndQwenModelReachRust() throws {
        var profile = ServiceProfile(); profile.provider = "alibabaCloud"; profile.textProvider = "followService"
        profile.qwenMtModel = "plus"; profile.proxyMode = "direct"; profile.textProxyMode = "custom"; profile.textProxyURL = "http://127.0.0.1:18999"
        var secrets = ServiceCredentials(); secrets.apiKey = "synthetic"
        let data = try profile.request(input: .both, source: "en", target: "zh", secrets: secrets)
        try Engine().validate(request: data)
        let config = (try JSONSerialization.jsonObject(with: data) as! [String: Any])["configuration"] as! [String: Any]
        XCTAssertEqual(config["qwenMtModel"] as? String, "plus")
        XCTAssertEqual(config["translationMode"] as? String, "turbo")
        XCTAssertNotEqual((config["networkProxy"] as! [String: Any])["mode"] as? String, (config["textNetworkProxy"] as! [String: Any])["mode"] as? String)
    }
    func testNativeCapabilitiesFilterLanguageAndCustomDeclaration() throws {
        var profile = ServiceProfile(); profile.provider = "tencentCloud"
        let caps = try profile.capabilities(source: "zh", target: "en")
        XCTAssertFalse(caps.sources.isEmpty); XCTAssertLessThan(caps.sources.count, LanguageCatalog.sources.count)
        XCTAssertThrowsError(try profile.requireSupported(source: "zu", target: "en"))
        profile.provider = "customOpenAIASR"; profile.customSpeechLanguages = ["en"]
        let custom = try profile.capabilities(source: "en", target: "zh")
        XCTAssertTrue(custom.sources.contains("en")); XCTAssertFalse(custom.sources.contains("ja"))
    }
    func testEveryNativeProviderExposesAuthoritativeCatalogWithoutCredentials() throws {
        for (provider, _) in ServiceProfile.providers {
            var profile = ServiceProfile(); profile.provider = provider
            let caps = try profile.capabilities(source: "zh", target: "en")
            XCTAssertFalse(caps.sources.isEmpty, provider); XCTAssertFalse(caps.targets.isEmpty, provider)
            XCTAssertEqual(caps.modes, ["turbo"], provider)
        }
        XCTAssertEqual(ProviderCatalog.bundled.presets.count, 5)
    }
    func testDuplicateHasNewIdentityAndNoSecretFields() throws {
        let original = ServiceProfile(), copy = original.duplicate()
        XCTAssertNotEqual(original.id, copy.id)
        let value = try JSONSerialization.jsonObject(with: JSONEncoder().encode(copy)) as! [String: Any]
        for key in ["apiKey","secretKey","secretID","textKey"] { XCTAssertNil(value[key]) }
    }
    func testLongSessionSearchPaginationRenameAndContentExport() async throws {
        let root = try folder(), directory = root.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let source = try transcript(directory, count: 1234)
        var listed = try await SessionLibrary.shared.list(root: root)
        XCTAssertEqual(listed.count, 1); XCTAssertTrue(listed[0].hasTranscript)
        try await SessionLibrary.shared.rename(listed[0], title: "Synthetic title")
        listed = try await SessionLibrary.shared.list(root: root, query: "Synthetic title")
        XCTAssertEqual(listed.first?.title, "Synthetic title")
        let found = try await SessionLibrary.shared.list(root: root, query: "Translation-1233")
        XCTAssertEqual(found.count, 1)
        let hits = try await SessionLibrary.shared.search(listed[0], query: "Original-")
        XCTAssertEqual(hits.count, 50)
        let next = try await SessionLibrary.shared.search(listed[0], query: "Original-", offset: hits.last!.id + 1)
        XCTAssertEqual(next.first?.caption.id, "synthetic-50")
        let page = try await SessionLibrary.shared.page(listed[0], offset: next[0].id)
        XCTAssertEqual(page.captions.first?.id, "synthetic-50")
        var options = TranscriptExportOptions(); options.content = .translation; options.timestamps = true
        let destination = root.appendingPathComponent("output.txt")
        try TranscriptExport.write(from: source, to: destination, options: options)
        let text = try String(contentsOf: destination)
        XCTAssertTrue(text.contains("Translation-1233")); XCTAssertFalse(text.contains("Original-")); XCTAssertTrue(text.contains("1970-"))
    }
    func testRenameLimitAndMissingAudioTranscript() async throws {
        let root = try folder(), directory = root.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let session = try await SessionLibrary.shared.list(root: root)[0]
        XCTAssertFalse(session.hasTranscript); XCTAssertTrue(session.audioSources.isEmpty)
        do { try await SessionLibrary.shared.rename(session, title: String(repeating: "x", count: 121)); XCTFail("Accepted long title") } catch {}
    }
    func testFailedExportPreservesDestinationAndCleansTemporaryFile() throws {
        let root = try folder(), source = root.appendingPathComponent("broken.jsonl"), output = root.appendingPathComponent("output.txt")
        try Data("bad json\n".utf8).write(to: source); try Data("original destination".utf8).write(to: output)
        XCTAssertThrowsError(try TranscriptExport.write(from: source, to: output, markdown: false))
        XCTAssertEqual(try String(contentsOf: output), "original destination")
        XCTAssertFalse(try FileManager.default.contentsOfDirectory(atPath: root.path).contains { $0.hasPrefix(".sheng-export") })
    }
    func testCancellationLeavesExistingDestinationUnchanged() async throws {
        let root = try folder(), source = try transcript(root, count: 1200), output = root.appendingPathComponent("output.txt")
        try Data("existing".utf8).write(to: output)
        let task = Task {
            try await Task.sleep(nanoseconds: 100_000_000)
            try TranscriptExport.write(from: source, to: output, markdown: true)
        }
        task.cancel()
        do { try await task.value; XCTFail("Ignored cancellation") } catch is CancellationError {}
        XCTAssertEqual(try String(contentsOf: output), "existing")
    }
    func testSuccessfulExportAtomicallyReplacesDestinationAndAudioMatches() async throws {
        let root = try folder(), directory = root.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let source = try transcript(directory, count: 2), destination = root.appendingPathComponent("existing.txt")
        try Data("old".utf8).write(to: destination)
        try TranscriptExport.write(from: source, to: destination, markdown: false)
        XCTAssertTrue(try String(contentsOf: destination).contains("Original-1"))
        let wave = directory.appendingPathComponent("system.wav")
        let writer = try WAVWriter(url: wave, rate: 16000); try writer.append(Data(repeating: 0, count: 320)); try writer.close()
        let session = try await SessionLibrary.shared.list(root: root)[0]
        let audio = root.appendingPathComponent("audio.wav")
        try await SessionLibrary.shared.exportAudio(session, source: .system, to: audio)
        XCTAssertEqual(try Data(contentsOf: wave), try Data(contentsOf: audio))
    }
    func testHealthThresholdsAndIndependentSources() {
        let tracker = CaptureHealthTracker(); tracker.begin(input: .both, now: 0)
        XCTAssertTrue(tracker.snapshot(now: 0).allSatisfy { $0.state == "waiting" })
        XCTAssertTrue(tracker.snapshot(now: 5).allSatisfy { $0.state == "noPCM" })
        let silence = Data(repeating: 0, count: 320)
        tracker.receive(silence, source: .microphone, now: 9)
        XCTAssertEqual(tracker.snapshot(now: 10).first { $0.source == .microphone }?.state, "quiet")
        var voice = Data()
        for _ in 0..<160 { voice.append(contentsOf: [0,64]) }
        tracker.receive(voice, source: .system, now: 10)
        XCTAssertEqual(tracker.snapshot(now: 10).first { $0.source == .system }?.state, "signal")
        tracker.fail(); XCTAssertTrue(tracker.snapshot(now: 11).allSatisfy { $0.state == "failed" })
    }
    func testSilentLaneIsNotFlaggedWhileOtherLaneHasSound() {
        let tracker = CaptureHealthTracker(); tracker.begin(input: .both, now: 0)
        var voice = Data()
        for _ in 0..<160 { voice.append(contentsOf: [0,64]) }
        let silence = Data(repeating: 0, count: 320)
        tracker.receive(silence, source: .system, now: 11)
        tracker.receive(voice, source: .microphone, now: 11)
        let lanes = tracker.snapshot(now: 11)
        XCTAssertEqual(lanes.first { $0.source == .microphone }?.state, "signal")
        XCTAssertEqual(lanes.first { $0.source == .system }?.state, "receiving")
        XCTAssertTrue(lanes.allSatisfy { $0.notice == nil })
    }
    @MainActor func testLanguagePresetsSaveImmediatelyWithoutOtherEdits() throws {
        let root = try folder()
        let model = SessionModel(product: .shengJian, store: try PrivateStore(product: .shengJian, root: root))
        var profile = ServiceProfile(); profile.name = "saved"
        try model.saveSpeechProfile(profile, credentials: ServiceCredentials())
        profile.name = "unsaved edit"
        try model.saveLanguagePresets([LanguagePair(source: "zh", target: "en")], for: profile.id)
        let reloaded = SessionModel(product: .shengJian, store: try PrivateStore(product: .shengJian, root: root))
        let stored = try XCTUnwrap(reloaded.profiles.first { $0.id == profile.id })
        XCTAssertEqual(stored.languagePresets, [LanguagePair(source: "zh", target: "en")])
        XCTAssertEqual(stored.name, "saved")
    }
    @MainActor func testSwappedLanguagesTurnAnyPairAround() throws {
        let model = SessionModel(product: .shengJian, store: try PrivateStore(product: .shengJian, root: folder()))
        model.sourceLanguage = "ja"; model.targetLanguage = "fr"
        XCTAssertEqual(model.swappedLanguages, LanguagePair(source: "fr", target: "ja"))
        model.sourceLanguage = "auto"
        XCTAssertNil(model.swappedLanguages)
        model.sourceLanguage = "ja"; model.targetLanguage = "original"
        XCTAssertNil(model.swappedLanguages)
    }
    func testOnlyConnectionFailuresAreRetried() {
        XCTAssertTrue(EngineError(code: "transport_error").isRetryable)
        XCTAssertFalse(EngineError(code: "credential_authentication_failed").isRetryable)
    }
    @MainActor func testSharedTextNamesTheRunningProduct() throws {
        _ = SessionModel(product: .shengXi, store: try PrivateStore(product: .shengXi, root: folder()))
        XCTAssertEqual(L10n.named("开始使用%@"), "开始使用声席")
        _ = SessionModel(product: .shengJian, store: try PrivateStore(product: .shengJian, root: folder()))
        XCTAssertEqual(L10n.named("开始使用%@"), "开始使用声笺")
    }
    func testSessionsGroupByDayNewestFirst() {
        var calendar = Calendar(identifier: .gregorian); calendar.timeZone = TimeZone(identifier: "UTC")!
        let now = Date(timeIntervalSince1970: 1_800_000_000)
        func item(_ id: String, hoursAgo: Double) -> SavedSession { SavedSession(id: id, directory: URL(fileURLWithPath: "/tmp/\(id)"), date: now.addingTimeInterval(-hoursAgo * 3600)) }
        let days = SessionDay.group([item("old", hoursAgo: 24 * 5), item("a", hoursAgo: 1), item("b", hoursAgo: 0.5), item("y", hoursAgo: 24)], now: now, calendar: calendar, locale: Locale(identifier: "zh-Hans"))
        XCTAssertEqual(days.map(\.title).prefix(2), ["今天", "昨天"])
        XCTAssertEqual(days.first?.sessions.map(\.id), ["b", "a"])
        XCTAssertEqual(days.count, 3)
    }
    @MainActor func testDiagnosticsExcludeSecretsAndPersonalMetadata() throws {
        let model = SessionModel(product: .shengJian, store: try PrivateStore(product: .shengJian, root: folder()))
        var profile = ServiceProfile(); profile.name = "synthetic-private-title"; profile.endpoint = "https://private.invalid"; model.profiles = [profile]; model.selectedProfile = profile.id
        let text = model.diagnosticText
        XCTAssertFalse(text.contains("synthetic-private-title")); XCTAssertFalse(text.contains("private.invalid")); XCTAssertFalse(text.contains(model.store.root.path))
    }
    @MainActor func testShortcutConflictKeepsPreviousRegistration() throws {
        final class Registration {}
        var fail = false
        let registry = ShortcutRegistry { _, _ in if fail { throw EngineError(code: "shortcut_in_use") }; return Registration() }
        let original = ShortcutPreferences(); try registry.replace(original) { _ in }
        fail = true; var changed = original; changed.session?.keyCode = 2
        XCTAssertThrowsError(try registry.replace(changed) { _ in })
        XCTAssertEqual(registry.active["session"], original.session)
    }
    @MainActor func testRetentionOffAndClearDoNotDeleteSavedFiles() async throws {
        let root = try folder(), store = try PrivateStore(product: .shengJian, root: root)
        let archive = try SessionArchive(store: store, rate: 16000, input: .system, history: true, recording: false, sourceLanguage: "en", targetLanguage: "zh", failure: {})
        await archive.close()
        let model = SessionModel(product: .shengJian, store: store)
        model.saveHistory = true; model.saveHistory = false; model.clearDisplayedCaptions()
        XCTAssertTrue(FileManager.default.fileExists(atPath: archive.directory.appendingPathComponent("captions.jsonl").path))
    }
}
