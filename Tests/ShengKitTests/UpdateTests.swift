import XCTest
import CryptoKit
@testable import ShengKit

final class UpdateTests: XCTestCase {
    private let repository = "owner/App-macos"

    private func releaseJSON(tag: String = "v1.2.0", assets: [(String, String)]) -> Data {
        let list = assets.map { #"{"name":"\#($0.0)","browser_download_url":"\#($0.1)","size":1000}"# }.joined(separator: ",")
        return Data(#"{"tag_name":"\#(tag)","name":"App \#(tag)","body":"Fixes","html_url":"https://github.com/owner/App-macos/releases/tag/\#(tag)","draft":false,"prerelease":false,"assets":[\#(list)]}"#.utf8)
    }
    private func url(_ name: String) -> String { "https://github.com/\(repository)/releases/download/v1.2.0/\(name)" }

    func testVersionsCompareNumerically() {
        XCTAssertEqual(AppVersion("v1.10.0"), AppVersion("1.10"))
        XCTAssertTrue(AppVersion("1.2.0")! < AppVersion("1.10.0")!)
        XCTAssertTrue(AppVersion("1.0.0")! < AppVersion("v1.0.1")!)
        XCTAssertNil(AppVersion("latest"))
        XCTAssertNil(AppVersion("1.0-beta"))
    }

    func testReleasePicksSignedZip() throws {
        let data = releaseJSON(assets: [("App-1.2.0.zip", url("App-1.2.0.zip")), ("App-1.2.0.zip.sig", url("App-1.2.0.zip.sig")), ("notes.txt", url("notes.txt"))])
        let release = try UpdateRelease.decode(data, repository: repository)
        XCTAssertEqual(release.version, AppVersion("1.2.0"))
        XCTAssertEqual(release.archive.lastPathComponent, "App-1.2.0.zip")
        XCTAssertEqual(release.signature.lastPathComponent, "App-1.2.0.zip.sig")
        XCTAssertEqual(release.notes, "Fixes")
    }

    func testReleaseWithoutSignatureIsRejected() {
        let data = releaseJSON(assets: [("App-1.2.0.zip", url("App-1.2.0.zip"))])
        XCTAssertThrowsError(try UpdateRelease.decode(data, repository: repository))
    }

    func testAssetsOutsideTheRepositoryAreRejected() {
        let elsewhere = "https://example.com/App-1.2.0.zip"
        let data = releaseJSON(assets: [("App-1.2.0.zip", elsewhere), ("App-1.2.0.zip.sig", url("App-1.2.0.zip.sig"))])
        XCTAssertThrowsError(try UpdateRelease.decode(data, repository: repository))
        let plain = releaseJSON(assets: [("App-1.2.0.zip", "http://github.com/\(repository)/releases/download/v1.2.0/App-1.2.0.zip"), ("App-1.2.0.zip.sig", url("App-1.2.0.zip.sig"))])
        XCTAssertThrowsError(try UpdateRelease.decode(plain, repository: repository))
    }

    func testUnparseableTagIsRejected() {
        XCTAssertThrowsError(try UpdateRelease.decode(releaseJSON(tag: "nightly", assets: []), repository: repository))
    }

    func testSignatureVerification() throws {
        let key = Curve25519.Signing.PrivateKey()
        let archive = Data("archive bytes".utf8)
        let signature = Data(try key.signature(for: archive).base64EncodedString().utf8)
        let publicKey = key.publicKey.rawRepresentation.base64EncodedString()
        XCTAssertNoThrow(try UpdateVerifier.verify(archive, signature: signature, publicKey: publicKey))
        XCTAssertThrowsError(try UpdateVerifier.verify(Data("tampered".utf8), signature: signature, publicKey: publicKey))
        XCTAssertThrowsError(try UpdateVerifier.verify(archive, signature: Data("bad".utf8), publicKey: publicKey))
        XCTAssertThrowsError(try UpdateVerifier.verify(archive, signature: signature, publicKey: "not-a-key"))
    }

    func testConfigurationNeedsRepositoryAndKey() {
        let key = Curve25519.Signing.PrivateKey().publicKey.rawRepresentation.base64EncodedString()
        let info: [String: Any] = ["ShengUpdateRepository": repository, "ShengUpdatePublicKey": key, "CFBundleShortVersionString": "1.0.0"]
        let configuration = UpdateConfiguration(info: info)
        XCTAssertEqual(configuration?.repository, repository)
        XCTAssertEqual(configuration?.currentVersion, AppVersion("1.0.0"))
        XCTAssertNil(UpdateConfiguration(info: ["ShengUpdateRepository": repository, "CFBundleShortVersionString": "1.0.0"]))
        XCTAssertNil(UpdateConfiguration(info: ["ShengUpdateRepository": "not a repo", "ShengUpdatePublicKey": key, "CFBundleShortVersionString": "1.0.0"]))
    }

    func testDailyAutomaticCheck() {
        let now = Date(timeIntervalSince1970: 1_000_000)
        XCTAssertTrue(UpdateSchedule.isDue(lastCheck: nil, now: now))
        XCTAssertFalse(UpdateSchedule.isDue(lastCheck: now.addingTimeInterval(-3600), now: now))
        XCTAssertTrue(UpdateSchedule.isDue(lastCheck: now.addingTimeInterval(-90_000), now: now))
    }
}
