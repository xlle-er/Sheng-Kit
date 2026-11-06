import XCTest
@testable import ShengKit

final class PerformanceTests: XCTestCase {
    private func folder() throws -> URL {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: directory) }; return directory
    }
    private func lane(_ state: String, buffers: UInt64) -> CaptureHealthSnapshot {
        CaptureHealthSnapshot(source: .system, state: state, receivedBuffers: buffers, rmsDb: -30, secondsSincePCM: 0.01)
    }

    func testHealthCountersAloneDoNotRepublishWithinInterval() {
        let previous = [lane("signal", buffers: 10)], next = [lane("signal", buffers: 11)]
        XCTAssertFalse(CaptureHealthSnapshot.needsPublish(previous: previous, next: next, sinceLastPublish: 0.1))
        XCTAssertTrue(CaptureHealthSnapshot.needsPublish(previous: previous, next: next, sinceLastPublish: CaptureHealthSnapshot.counterRefreshInterval))
    }

    func testHealthStateChangePublishesImmediately() {
        XCTAssertTrue(CaptureHealthSnapshot.needsPublish(previous: [lane("signal", buffers: 10)], next: [lane("quiet", buffers: 11)], sinceLastPublish: 0))
        XCTAssertTrue(CaptureHealthSnapshot.needsPublish(previous: [], next: [lane("waiting", buffers: 0)], sinceLastPublish: 0))
    }

    func testCapabilityQueriesAreCachedPerProfileAndLanguagePair() throws {
        var profile = ServiceProfile(); profile.name = "cache-\(UUID().uuidString)"
        let before = ProfileCapabilityCache.shared.misses
        let first = try profile.capabilities(source: "zh", target: "en")
        let second = try profile.capabilities(source: "zh", target: "en")
        XCTAssertEqual(ProfileCapabilityCache.shared.misses, before + 1)
        XCTAssertEqual(first.sources, second.sources)
        _ = try profile.capabilities(source: "en", target: "zh")
        profile.provider = "openAIRealtime"
        _ = try profile.capabilities(source: "en", target: "zh")
        XCTAssertEqual(ProfileCapabilityCache.shared.misses, before + 3)
    }

    @MainActor func testIdleModelDoesNotPollEngine() throws {
        let model = SessionModel(product: .shengJian, store: try PrivateStore(product: .shengJian, root: folder()))
        XCTAssertFalse(model.isPolling)
    }

    @MainActor func testScheduledPreferenceSavesCoalesceIntoOneDelayedWrite() async throws {
        let root = try folder()
        let model = SessionModel(product: .shengJian, store: try PrivateStore(product: .shengJian, root: root))
        let file = root.appendingPathComponent("preferences-v1.json")
        for size in [20.0, 30.0, 40.0] { model.fontSize = size; model.schedulePreferencesSave() }
        XCTAssertFalse(FileManager.default.fileExists(atPath: file.path))
        try await Task.sleep(nanoseconds: 800_000_000)
        let saved = try JSONDecoder().decode(Preferences.self, from: Data(contentsOf: file))
        XCTAssertEqual(saved.fontSize, 40)
    }

    @MainActor func testFlushWritesPendingPreferencesImmediately() throws {
        let root = try folder()
        let model = SessionModel(product: .shengJian, store: try PrivateStore(product: .shengJian, root: root))
        model.overlayOpacity = 0.5; model.schedulePreferencesSave(); model.flushPreferences()
        let saved = try JSONDecoder().decode(Preferences.self, from: Data(contentsOf: root.appendingPathComponent("preferences-v1.json")))
        XCTAssertEqual(saved.overlayOpacity, 0.5)
    }
}
