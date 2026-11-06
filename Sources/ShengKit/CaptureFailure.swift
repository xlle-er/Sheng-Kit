import Foundation
import ScreenCaptureKit
import AppKit

enum CaptureFailure {
    /// Preserve actionable errors; never include native descriptions/userInfo in logs.
    static func classify(_ error: Error) -> EngineError {
        if let error = error as? EngineError { return error }
        let native = error as NSError
        if native.domain == SCStreamErrorDomain {
            switch native.code {
            case -3801: return EngineError(code: "screen_capture_permission_denied")
            case -3803: return EngineError(code: "capture_entitlements_missing")
            case -3804, -3805, -3811: return EngineError(code: "capture_service_unavailable")
            case -3818: return EngineError(code: "system_audio_unavailable")
            default: return EngineError(code: "system_capture_\(native.code)")
            }
        }
        return EngineError(code: "audio_capture_\(native.code)")
    }
    static func openPermissions(microphone: Bool) {
        let destination = microphone ? "Privacy_Microphone" : "Privacy_ScreenCapture"
        if let url = URL(string: "x-apple.systempreferences:com.apple.preference.security?\(destination)") { NSWorkspace.shared.open(url) }
    }
}
