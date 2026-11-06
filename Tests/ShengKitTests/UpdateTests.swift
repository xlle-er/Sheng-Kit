import XCTest
import CryptoKit
@testable import ShengKit

final class UpdateTests: XCTestCase {
    private let repository = "owner/App-macos"

    private func manifest(version: String = "1.2.0", archive: String = "App-1.2.0.zip", size: Int = 1000) -> Data {
        Data(#"{"version":"\#(version)","archive":"\#(archive)","size":\#(size),"notes":"Fixes"}"#.utf8)
    }

    func testVersionsCompareNumerically() {
        XCTAssertEqual(AppVersion("v1.10.0"), AppVersion("1.10"))
        XCTAssertTrue(AppVersion("1.2.0")! < AppVersion("1.10.0")!)
        XCTAssertTrue(AppVersion("1.0.0")! < AppVersion("v1.0.1")!)
        XCTAssertNil(AppVersion("latest"))
        XCTAssertNil(AppVersion("1.0-beta"))
    }

    func testManifestPointsAtSignedArchiveInTheRelease() throws {
        let release = try UpdateRelease.decode(manifest(), repository: repository)
        XCTAssertEqual(release.version, AppVersion("1.2.0"))
        XCTAssertEqual(release.archive.absoluteString, "https://github.com/\(repository)/releases/download/v1.2.0/App-1.2.0.zip")
        XCTAssertEqual(release.signature.absoluteString, "https://github.com/\(repository)/releases/download/v1.2.0/App-1.2.0.zip.sig")
        XCTAssertEqual(release.page?.absoluteString, "https://github.com/\(repository)/releases/tag/v1.2.0")
        XCTAssertEqual(release.notes, "Fixes")
    }

    func testManifestURLAvoidsTheRateLimitedAPI() {
        let key = Curve25519.Signing.PrivateKey().publicKey.rawRepresentation.base64EncodedString()
        let configuration = UpdateConfiguration(info: ["ShengUpdateRepository": repository, "ShengUpdatePublicKey": key, "CFBundleShortVersionString": "1.0.0"])
        XCTAssertEqual(configuration?.manifestURL.absoluteString, "https://github.com/\(repository)/releases/latest/download/update.json")
    }

    func testArchiveNamesThatEscapeTheReleaseAreRejected() {
        for name in ["../evil.zip", "dir/App.zip", "App.tar.gz", "", "a b.zip"] {
            XCTAssertThrowsError(try UpdateRelease.decode(manifest(archive: name), repository: repository), name)
        }
    }

    func testBadVersionOrSizeIsRejected() {
        XCTAssertThrowsError(try UpdateRelease.decode(manifest(version: "nightly"), repository: repository))
        XCTAssertThrowsError(try UpdateRelease.decode(manifest(size: 0), repository: repository))
        XCTAssertThrowsError(try UpdateRelease.decode(manifest(size: 900_000_000), repository: repository))
        XCTAssertThrowsError(try UpdateRelease.decode(Data("not json".utf8), repository: repository))
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
