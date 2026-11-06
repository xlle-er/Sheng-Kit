import Foundation

/// Capability queries are pure functions of the credential-free request, but views evaluate them on every render.
final class ProfileCapabilityCache: @unchecked Sendable {
    static let shared = ProfileCapabilityCache()
    static let capacity = 128
    private let lock = NSLock()
    private var values: [String: ProfileCapabilities] = [:]
    private(set) var misses = 0
    func value(for request: String) -> ProfileCapabilities? {
        lock.withLock {
            if let value = values[request] { return value }
            misses += 1; return nil
        }
    }
    func store(_ value: ProfileCapabilities, for request: String) {
        lock.withLock {
            if values.count >= Self.capacity { values.removeAll() }
            values[request] = value
        }
    }
}
