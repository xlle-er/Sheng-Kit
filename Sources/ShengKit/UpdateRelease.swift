import Foundation
import CryptoKit

/// A dotted numeric version such as `1.2.0`; a leading `v` is accepted, missing parts count as 0.
public struct AppVersion: Comparable, CustomStringConvertible {
    let parts: [Int]
    public init?(_ text: String) {
        let trimmed = text.hasPrefix("v") ? String(text.dropFirst()) : text
        let pieces = trimmed.split(separator: ".", omittingEmptySubsequences: false)
        guard !pieces.isEmpty, pieces.count <= 4 else { return nil }
        var values: [Int] = []
        for piece in pieces {
            guard !piece.isEmpty, piece.allSatisfy(\.isNumber), let value = Int(piece) else { return nil }
            values.append(value)
        }
        while values.count < 3 { values.append(0) }
        parts = values
    }
    public var description: String { parts.map(String.init).joined(separator: ".") }
    public static func == (lhs: AppVersion, rhs: AppVersion) -> Bool { lhs.padded(to: rhs) == rhs.padded(to: lhs) }
    public static func < (lhs: AppVersion, rhs: AppVersion) -> Bool { lhs.padded(to: rhs).lexicographicallyPrecedes(rhs.padded(to: lhs)) }
    private func padded(to other: AppVersion) -> [Int] { parts + Array(repeating: 0, count: max(0, other.parts.count - parts.count)) }
}

/// Where updates come from, read from the app's Info.plist so the shared SDK names no product.
public struct UpdateConfiguration {
    /// `owner/name` of the GitHub repository whose releases carry the app.
    public let repository: String
    /// Base64 raw Ed25519 public key that signs every release archive.
    public let publicKey: String
    public let currentVersion: AppVersion
    public let bundleIdentifier: String

    public init?(info: [String: Any]) {
        guard let repository = info["ShengUpdateRepository"] as? String, Self.isRepository(repository),
              let key = info["ShengUpdatePublicKey"] as? String, (try? UpdateVerifier.publicKey(key)) != nil,
              let version = (info["CFBundleShortVersionString"] as? String).flatMap(AppVersion.init) else { return nil }
        self.repository = repository; publicKey = key; currentVersion = version
        bundleIdentifier = info["CFBundleIdentifier"] as? String ?? ""
    }
    public static var main: UpdateConfiguration? { UpdateConfiguration(info: Bundle.main.infoDictionary ?? [:]) }
    /// A small manifest attached to every release. GitHub redirects this link to the newest release's
    /// copy without the API's per-address rate limit.
    var manifestURL: URL { URL(string: "https://github.com/\(repository)/releases/latest/download/update.json")! }
    private static func isRepository(_ value: String) -> Bool {
        let parts = value.split(separator: "/", omittingEmptySubsequences: false)
        let allowed = CharacterSet.alphanumerics.union(CharacterSet(charactersIn: "-_."))
        return parts.count == 2 && parts.allSatisfy { !$0.isEmpty && $0.unicodeScalars.allSatisfy(allowed.contains) }
    }
}

public enum UpdateError: LocalizedError, Equatable {
    case malformedRelease, missingAsset, untrustedAsset, badSignature, tooLarge, network, notInstallable(String), installFailed
    public var errorDescription: String? {
        switch self {
        case .malformedRelease: return L10n.text("无法读取最新版本信息。")
        case .missingAsset: return L10n.text("最新版本缺少安装包或签名文件。")
        case .untrustedAsset: return L10n.text("安装包地址不在发布仓库中，已停止更新。")
        case .badSignature: return L10n.text("安装包签名校验失败，已停止更新。")
        case .tooLarge: return L10n.text("安装包大小异常，已停止更新。")
        case .network: return L10n.text("无法连接到 GitHub，请检查网络后重试。")
        case .notInstallable(let reason): return reason
        case .installFailed: return L10n.text("安装新版本失败，当前版本未改动。")
        }
    }
}

/// The newest published release and its signed archive.
public struct UpdateRelease: Equatable {
    public let version: AppVersion
    public let title: String
    public let notes: String
    public let page: URL?
    let archive: URL
    let signature: URL
    let archiveSize: Int64
    /// Archives above this are refused before downloading.
    static let sizeLimit: Int64 = 500 * 1024 * 1024

    /// `update.json`: `{"version":"1.2.0","archive":"App-1.2.0.zip","size":123,"notes":"…"}`; the signature is `archive + ".sig"`.
    private struct Manifest: Decodable { let version: String; let archive: String; let size: Int64; let notes: String? }

    static func decode(_ data: Data, repository: String) throws -> UpdateRelease {
        guard let manifest = try? JSONDecoder().decode(Manifest.self, from: data),
              let version = AppVersion(manifest.version) else { throw UpdateError.malformedRelease }
        guard isPlainArchiveName(manifest.archive) else { throw UpdateError.untrustedAsset }
        guard manifest.size > 0, manifest.size <= sizeLimit else { throw UpdateError.tooLarge }
        let tag = "v" + version.description
        let base = "https://github.com/\(repository)/releases"
        guard let archive = URL(string: "\(base)/download/\(tag)/\(manifest.archive)"),
              let signature = URL(string: "\(base)/download/\(tag)/\(manifest.archive).sig") else { throw UpdateError.untrustedAsset }
        return UpdateRelease(version: version, title: tag, notes: manifest.notes ?? "", page: URL(string: "\(base)/tag/\(tag)"),
                             archive: archive, signature: signature, archiveSize: manifest.size)
    }
    /// A single file name inside the release: letters, digits, `.-_`, ending in `.zip`.
    private static func isPlainArchiveName(_ name: String) -> Bool {
        let allowed = CharacterSet.alphanumerics.union(CharacterSet(charactersIn: "-_."))
        return name.hasSuffix(".zip") && name.count > 4 && !name.hasPrefix(".") && name.unicodeScalars.allSatisfy(allowed.contains)
    }
}

public enum UpdateVerifier {
    static func publicKey(_ base64: String) throws -> Curve25519.Signing.PublicKey {
        guard let raw = Data(base64Encoded: base64), let key = try? Curve25519.Signing.PublicKey(rawRepresentation: raw) else { throw UpdateError.badSignature }
        return key
    }
    /// `signature` is the base64 text of the `.sig` asset.
    static func verify(_ archive: Data, signature: Data, publicKey: String) throws {
        let key = try self.publicKey(publicKey)
        let text = String(decoding: signature, as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines)
        guard let raw = Data(base64Encoded: text), key.isValidSignature(raw, for: archive) else { throw UpdateError.badSignature }
    }
}

enum UpdateSchedule {
    static let interval: TimeInterval = 24 * 3600
    static func isDue(lastCheck: Date?, now: Date = Date()) -> Bool { lastCheck.map { now.timeIntervalSince($0) >= interval } ?? true }
}
