import XCTest
import AppKit
import Carbon
@testable import ShengKit

final class RedesignTests: XCTestCase {
    private func folder() throws -> URL {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: directory) }; return directory
    }
    private func session(in root: URL, created: Date, bytes: Int = 4) throws -> URL {
        let directory = root.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        try Data(repeating: 1, count: bytes).write(to: directory.appendingPathComponent("captions.jsonl"))
        try FileManager.default.setAttributes([.creationDate: created], ofItemAtPath: directory.path)
        return directory
    }

    func testPickerNeverShowsInternalValueWhenSelectionIsMissing() {
        let options = [("zh", "中文"), ("en", "英语")]
        XCTAssertEqual(ChoiceControl.displayLabel(selection: "en", options: options), "英语")
        XCTAssertEqual(ChoiceControl.displayLabel(selection: "004339B3-FD6A", options: options), "请选择")
    }

    @MainActor func testNewDisplayPreferencesRoundTripAndDefault() throws {
        let root = try folder()
        let model = SessionModel(product: .shengJian, store: try PrivateStore(product: .shengJian, root: root))
        XCTAssertEqual(model.overlayBackground, "dark")
        XCTAssertTrue(model.showSourceLabel); XCTAssertTrue(model.showSessionInfo)
        XCTAssertFalse(model.showOverlayOnLaunch); XCTAssertEqual(model.sessionRetentionDays, 0)
        model.overlayBackground = "light"; model.showSourceLabel = false; model.showSessionInfo = false
        model.showOverlayOnLaunch = true; model.sessionRetentionDays = 30
        model.savePreferences()
        let reloaded = SessionModel(product: .shengJian, store: try PrivateStore(product: .shengJian, root: root))
        XCTAssertEqual(reloaded.overlayBackground, "light")
        XCTAssertFalse(reloaded.showSourceLabel); XCTAssertFalse(reloaded.showSessionInfo)
        XCTAssertTrue(reloaded.showOverlayOnLaunch); XCTAssertEqual(reloaded.sessionRetentionDays, 30)
    }

    func testRetentionDeletesOnlyOlderSessionsAndNeverTheActiveOne() async throws {
        let root = try folder(), now = Date()
        let old = try session(in: root, created: now.addingTimeInterval(-40 * 86_400))
        let active = try session(in: root, created: now.addingTimeInterval(-50 * 86_400))
        let recent = try session(in: root, created: now.addingTimeInterval(-2 * 86_400))
        let removed = try await SessionLibrary.shared.prune(root: root, olderThanDays: 30, now: now, keeping: active.lastPathComponent)
        XCTAssertEqual(removed, 1)
        XCTAssertFalse(FileManager.default.fileExists(atPath: old.path))
        XCTAssertTrue(FileManager.default.fileExists(atPath: active.path))
        XCTAssertTrue(FileManager.default.fileExists(atPath: recent.path))
        let untouched = try await SessionLibrary.shared.prune(root: root, olderThanDays: 0, now: now.addingTimeInterval(1e9), keeping: nil)
        XCTAssertEqual(untouched, 0)
    }

    func testStorageUsageSumsSavedSessions() async throws {
        let root = try folder()
        _ = try session(in: root, created: Date(), bytes: 100)
        _ = try session(in: root, created: Date(), bytes: 28)
        let usage = try await SessionLibrary.shared.usage(root: root)
        XCTAssertEqual(usage, 128)
    }

    func testRecordedShortcutRequiresCommandOptionOrControl() {
        let recorded = ShortcutBinding(keyCode: 1, flags: [.option, .shift])
        XCTAssertEqual(recorded, ShortcutBinding(keyCode: 1, modifiers: UInt32(optionKey | shiftKey)))
        XCTAssertNil(ShortcutBinding(keyCode: 1, flags: [.shift]))
        XCTAssertNil(ShortcutBinding(keyCode: 122, flags: [.command]))
    }

    func testOverlayShortcutIsRegisteredWithOthers() {
        var preferences = ShortcutPreferences()
        preferences.overlay = ShortcutBinding(keyCode: 31, modifiers: UInt32(optionKey | shiftKey))
        XCTAssertTrue(preferences.actions.contains { $0.0 == "overlay" && $0.1 != nil })
    }

    func testServiceReadinessExplainsWhatIsMissing() {
        var cloud = ServiceProfile(); cloud.provider = "openAIRealtime"; cloud.speechMode = "simultaneous"
        XCTAssertEqual(cloud.readiness(credentials: ServiceCredentials(), source: "auto", target: "en"), .missingCredential)
        var key = ServiceCredentials(); key.apiKey = "test-key"
        XCTAssertEqual(cloud.readiness(credentials: key, source: "auto", target: "en"), .ready)
        XCTAssertEqual(cloud.readiness(credentials: key, source: "zh", target: "en"), .unsupportedLanguage)
        var tencent = ServiceProfile(); tencent.provider = "tencentCloud"
        var partial = ServiceCredentials(); partial.secretID = "id"
        XCTAssertEqual(tencent.readiness(credentials: partial, source: "zh", target: "en"), .missingCredential)
        XCTAssertEqual(ServiceProfile().readiness(credentials: ServiceCredentials(), source: "zh", target: "en"), .ready)
    }
}
