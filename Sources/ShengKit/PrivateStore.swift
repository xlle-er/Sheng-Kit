import Foundation

public final class PrivateStore {
    public let root: URL
    public init(product: Product, root: URL? = nil) throws {
        self.root = root ?? FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent("Library/Application Support/\(product.bundleID)")
        try FileManager.default.createDirectory(at: self.root, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
        let values = try self.root.resourceValues(forKeys: [.isSymbolicLinkKey])
        guard values.isSymbolicLink != true else { throw EngineError(code: "storage_symlink_rejected") }
        try FileManager.default.setAttributes([.posixPermissions: 0o700], ofItemAtPath: self.root.path)
    }
    public func write(_ data: Data, name: String) throws {
        guard !name.contains("/"), !name.hasPrefix(".") else { throw EngineError(code: "invalid_filename") }
        let url = root.appendingPathComponent(name)
        if FileManager.default.fileExists(atPath: url.path), try url.resourceValues(forKeys: [.isSymbolicLinkKey]).isSymbolicLink == true { throw EngineError(code: "storage_symlink_rejected") }
        try data.write(to: url, options: .atomic)
        try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: url.path)
    }
    public func loadProfiles() throws -> [ServiceProfile] {
        let url = root.appendingPathComponent("profiles.json")
        guard FileManager.default.fileExists(atPath: url.path) else { return [ServiceProfile()] }
        return try JSONDecoder().decode([ServiceProfile].self, from: Data(contentsOf: url))
    }
    public func saveProfiles(_ profiles: [ServiceProfile]) throws {
        try write(JSONEncoder().encode(profiles), name: "profiles.json")
    }
    public func credentials(for id: UUID) throws -> ServiceCredentials {
        try allCredentials()[id.uuidString] ?? ServiceCredentials()
    }
    private func allCredentials() throws -> [String: ServiceCredentials] {
        let url = root.appendingPathComponent("credentials.json")
        guard FileManager.default.fileExists(atPath: url.path) else { return [:] }
        return try JSONDecoder().decode([String: ServiceCredentials].self, from: Data(contentsOf: url))
    }
    public func saveCredentials(_ credentials: ServiceCredentials, for id: UUID) throws {
        var values = try allCredentials(); values[id.uuidString] = credentials
        try write(JSONEncoder().encode(values), name: "credentials.json")
    }
    public func deleteCredentials(for id: UUID) throws {
        var values = try allCredentials(); values.removeValue(forKey: id.uuidString)
        try write(JSONEncoder().encode(values), name: "credentials.json")
    }
}
