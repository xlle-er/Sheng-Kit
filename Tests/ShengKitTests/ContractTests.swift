import XCTest
import ScreenCaptureKit
import AVFoundation
@testable import ShengKit

final class ContractTests: XCTestCase {
    func testAppleRequestAndCloudRoutesValidateWithoutOpeningNetwork() throws {
        let engine = Engine()
        var profile = ServiceProfile(); var secrets = ServiceCredentials()
        try engine.validate(request: profile.request(input: .both, source: "zh", target: "en", secrets: secrets))
        profile.provider = "openAIRealtime"; secrets.apiKey = "test-offline"; profile.speechMode = "asrMt"
        try engine.validate(request: profile.request(input: .system, source: "en", target: "zh", secrets: secrets))
        profile.provider = "alibabaCloud"; profile.speechMode = "asrMt"
        try engine.validate(request: profile.request(input: .microphone, source: "zh", target: "en", secrets: secrets))
    }
    func testFullLanguageCatalogComesFromCore() {
        XCTAssertGreaterThan(Engine.languageCatalog()["sources"]?.count ?? 0, 100)
        XCTAssertGreaterThan(Engine.languageCatalog()["targets"]?.count ?? 0, 100)
    }
    func testCredentialsAreSeparateAndPrivate() throws {
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: folder) }
        let store = try PrivateStore(product: .shengJian, root: folder)
        let profile = ServiceProfile(); var secrets = ServiceCredentials(); secrets.apiKey = "offline-secret-sentinel"
        try store.saveCredentials(secrets, for: profile.id)
        try store.saveProfiles([profile])
        let metadata = try String(contentsOf: folder.appendingPathComponent("profiles.json"))
        XCTAssertFalse(metadata.contains(secrets.apiKey))
        XCTAssertEqual(try store.credentials(for: profile.id).apiKey, secrets.apiKey)
        let attributes = try FileManager.default.attributesOfItem(atPath: folder.appendingPathComponent("credentials.json").path)
        XCTAssertEqual((attributes[.posixPermissions] as? NSNumber)?.intValue, 0o600)
    }
    func testIndependentHandlesAndInvalidConfiguration() throws {
        let first = Engine(); let second = Engine()
        XCTAssertThrowsError(try first.start(request: Data("{}".utf8)))
        XCTAssertEqual(try second.snapshot().status, "idle")
        XCTAssertEqual(try second.drain().count, 0)
    }
}

extension ContractTests {
    func testStreamingExportPreservesAllCaptionsAndSourceDirection() throws {
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: folder) }
        let source = folder.appendingPathComponent("captions.jsonl")
        let captions = (0..<40).map { Caption(id: "synthetic-\($0)", source: $0 % 2 == 0 ? .system : .microphone, original: "Synthetic original \($0)", translation: "Synthetic translation \($0)", timestampMs: UInt64($0), sourceLanguage: "en", targetLanguage: "zh") }
        var data = Data()
        for caption in captions { data.append(try JSONEncoder().encode(caption)); data.append(10) }
        try data.write(to: source)
        let destination = folder.appendingPathComponent("export.md")
        try TranscriptExport.write(from: source, to: destination, markdown: true)
        let exported = try String(contentsOf: destination)
        XCTAssertTrue(exported.contains("Synthetic original 39"))
        XCTAssertTrue(exported.contains("Synthetic original 0"))
        XCTAssertTrue(exported.contains("麦克风"))
    }
    func testWAVHeaderAndSelectedSourcePCM() throws {
        let file = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString + ".wav")
        defer { try? FileManager.default.removeItem(at: file) }
        let writer = try WAVWriter(url: file, rate: 16000)
        let pcm = Data(repeating: 0, count: 3200)
        try writer.append(pcm); try writer.close()
        let data = try Data(contentsOf: file)
        XCTAssertEqual(data.count, 3244)
        XCTAssertEqual(String(data: data.prefix(4), encoding: .utf8), "RIFF")
        XCTAssertEqual(String(data: data.subdata(in: 8..<12), encoding: .utf8), "WAVE")
        XCTAssertEqual(data.suffix(3200), pcm)
    }
    func testRetryScheduleIsBounded() {
        XCTAssertGreaterThanOrEqual(Engine.retryDelay(attempt: 0), 0)
        XCTAssertEqual(Engine.retryDelay(attempt: 4), -1)
    }
    @MainActor func testNewProductsStartWithoutCaptureOrRetention() throws {
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: folder) }
        let model = SessionModel(product: .shengJian, store: try PrivateStore(product: .shengJian, root: folder))
        XCTAssertFalse(model.isCapturing); XCTAssertFalse(model.saveHistory); XCTAssertFalse(model.recordAudio)
        XCTAssertEqual(model.input, .system)
    }
}

extension ContractTests {
    func testEverySpeechProviderHasAValidNativeConfigurationContract() throws {
        let engine = Engine()
        for (provider, _) in ServiceProfile.providers {
            var profile = ServiceProfile(); var secrets = ServiceCredentials()
            profile.provider = provider; secrets.apiKey = "synthetic-offline-key"
            profile.appID = "123456"; secrets.secretID = "synthetic-id"; secrets.secretKey = "synthetic-secret"
            profile.model = "synthetic-model"; profile.deployment = "translation"; profile.transcriptionDeployment = "transcription"
            profile.endpoint = provider == "azureOpenAIRealtime" ? "https://synthetic.openai.azure.com" : "ws://127.0.0.1:18009"
            do { try engine.validate(request: profile.request(input: .both, source: "zh", target: "en", secrets: secrets)) }
            catch { XCTFail("Native configuration contract rejected provider \(provider): \(error.localizedDescription)") }
        }
    }
    func testTranscriptReaderPagesWithoutDroppingReadAhead() throws {
        let file = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: file) }
        var content = Data()
        for i in 0..<123 {
            let row = Caption(id: "row-\(i)", source: .system, original: "Synthetic \(i)", translation: "Translation \(i)", timestampMs: UInt64(i), sourceLanguage: "en", targetLanguage: "zh")
            content.append(try JSONEncoder().encode(row)); content.append(10)
        }
        try content.write(to: file)
        var cursor: UInt64 = 0, ids: [String] = []
        while true { let page = try TranscriptReader.page(file: file, offset: cursor); ids.append(contentsOf: page.captions.map(\.id)); cursor = page.nextOffset; if !page.hasMore { break } }
        XCTAssertEqual(ids, (0..<123).map { "row-\($0)" })
    }
}

extension ContractTests {
    func testReadingBufferUpdatesPairsAndBoundsMemory() {
        var buffer = CaptionPresentation()
        for index in 0..<600 {
            buffer.append(Caption(id: "caption-\(index)", source: index % 2 == 0 ? .system : .microphone, original: "Original \(index)", translation: "", timestampMs: UInt64(index), sourceLanguage: "en", targetLanguage: "zh"))
        }
        XCTAssertEqual(buffer.captions.count, 500)
        XCTAssertEqual(buffer.captions.first?.id, "caption-100")
        buffer.append(Caption(id: "caption-599", source: .microphone, original: "Original 599", translation: "Updated translation", timestampMs: 599, sourceLanguage: "en", targetLanguage: "zh"))
        XCTAssertEqual(buffer.captions.count, 500)
        XCTAssertEqual(buffer.captions.last?.translation, "Updated translation")
        buffer.append(Caption(id: "large", source: .system, original: String(repeating: "x", count: CaptionPresentation.byteLimit + 1), translation: "", timestampMs: 600, sourceLanguage: "en", targetLanguage: "zh"))
        XCTAssertEqual(buffer.captions.count, 500)
        XCTAssertEqual(buffer.captions.last?.translation, "Updated translation")
    }
    func testClearDoesNotReplayCachedFinalOrDraft() {
        var buffer = CaptionPresentation()
        let caption = Caption(id: "old", source: .system, original: "Old original", translation: "Old translation", timestampMs: 1, sourceLanguage: "en", targetLanguage: "zh")
        let draft = CaptionTrack(source: .microphone, original: "Pending", translation: "", pendingOriginal: "Next", isFinal: false)
        buffer.append(caption)
        let snapshot = EngineSnapshot(protocol: 1, revision: 1, generation: 1, runId: "test", status: "listening", error: nil, sampleRate: 16000, settled: false, tracks: [draft], recent: [caption])
        buffer.clear(snapshot: snapshot)
        buffer.append(caption)
        XCTAssertTrue(buffer.captions.isEmpty)
        XCTAssertFalse(buffer.shows(draft))
        XCTAssertTrue(buffer.shows(CaptionTrack(source: .microphone, original: "New words", translation: "", pendingOriginal: "", isFinal: false)))
        buffer.append(Caption(id: "new", source: .system, original: "Old original", translation: "Old translation", timestampMs: 2, sourceLanguage: "en", targetLanguage: "zh"))
        XCTAssertEqual(buffer.captions.count, 1)
    }
}

extension ContractTests {
    @MainActor func testStartWithoutSelectedProfileReportsActionableFailure() async throws {
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: folder) }
        let model = SessionModel(product: .shengJian, store: try PrivateStore(product: .shengJian, root: folder))
        model.profiles = []
        await model.start()
        XCTAssertNotNil(model.error)
        XCTAssertFalse(model.isBusy)
        XCTAssertFalse(model.isCapturing)
    }
}

extension ContractTests {
    func testCapturePermissionErrorsStayActionable() {
        let permission = CaptureFailure.classify(NSError(domain: SCStreamErrorDomain, code: -3801, userInfo: [NSLocalizedDescriptionKey: "private-context-sentinel"]))
        XCTAssertEqual(permission.code, "screen_capture_permission_denied")
        XCTAssertTrue(permission.localizedDescription.contains("授权"))
        XCTAssertFalse(permission.localizedDescription.contains("private-context-sentinel"))
        let engine = CaptureFailure.classify(EngineError(code: "microphone_permission_denied"))
        XCTAssertEqual(engine.code, "microphone_permission_denied")
    }
}

extension ContractTests {
    func testNative48kStereoConvertsToServiceMonoPCM() throws {
        let format = try XCTUnwrap(AVAudioFormat(commonFormat: .pcmFormatFloat32, sampleRate: 48000, channels: 2, interleaved: false))
        let buffer = try XCTUnwrap(AVAudioPCMBuffer(pcmFormat: format, frameCapacity: 4800))
        buffer.frameLength = 4800
        let channels = try XCTUnwrap(buffer.floatChannelData)
        for channel in 0..<2 { for frame in 0..<4800 { channels[channel][frame] = Float(sin(Double(frame) * 0.05)) * 0.1 } }
        let converter = PCMConverter(rate: 16000)
        var bytes = 0
        for _ in 0..<10 {
            let pcm = try XCTUnwrap(converter.convert(buffer))
            XCTAssertEqual(pcm.count % 2, 0)
            bytes += pcm.count
        }
        XCTAssertGreaterThan(bytes, 31000)
        XCTAssertLessThanOrEqual(bytes, 32000)
    }
}
