import Foundation
import Darwin

/// One capture owner across the two NEW apps. Does not inspect any old app or lock.
final class CaptureLease {
    private var descriptor: Int32 = -1
    func acquire() throws {
        guard descriptor == -1 else { return }
        let folder = FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent("Library/Application Support/app.xlle.sharedkit")
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
        let fd = Darwin.open(folder.appendingPathComponent("capture.lock").path, O_CREAT | O_RDWR | O_NOFOLLOW, 0o600)
        guard fd >= 0 else { throw EngineError(code: "capture_lock_unavailable") }
        guard flock(fd, LOCK_EX | LOCK_NB) == 0 else { Darwin.close(fd); throw EngineError(code: "capture_busy") }
        descriptor = fd
    }
    func release() { if descriptor >= 0 { flock(descriptor, LOCK_UN); Darwin.close(descriptor); descriptor = -1 } }
    deinit { release() }
}
