import Foundation
import CShengEngine

public struct EngineError: LocalizedError {
    public let code: String
    /// Failures a fresh connection can fix; the session reconnects with back-off on these only.
    static let retryableCodes: Set<String> = ["transport_error", "provider_event_backlog_overflow", "translation_backlog_overflow", "apple_speech_runtime_service_unavailable", "apple_speech_runtime_timeout", "apple_speech_stream_closed"]
    public var isRetryable: Bool { Self.retryableCodes.contains(code) }
    public var errorDescription: String? { switch code {
    case "configuration_invalid", "invalid_configuration": return L10n.text("服务配置不支持所选语言或模式，请检查配置。")
    case "provider_connection_failed": return L10n.text("服务连接失败，请检查网络、凭据及语言资源。")
    case "probe_timeout": return L10n.text("服务检查超时，请重试或检查本地语言资源与网络。")
    case "provider_request_failed": return L10n.text("识别或翻译服务返回错误，请检查配置后重试。")
    case "echo_initialization_failed", "echo_processing_failed": return L10n.text("双音源回声处理无法继续，请重新开启字幕。")
    case "audio_backpressure": return L10n.text("音频处理落后，已停止会话。请检查网络后重试。")
    case "speech_assets_missing", "translation_assets_missing": return L10n.text("请先在设置中下载所选方向的 Apple 语言资源。")
    case "apple_unavailable": return L10n.text("本机服务或所选配置不可用，请检查系统条件与服务设置。")
    case "credential_authentication_failed": return L10n.text("服务拒绝了凭据，请检查 API Key 和服务权限。")
    case "transport_error": return L10n.text("连接中断，重试后仍无法恢复，请检查网络。")
    case "screen_capture_permission_denied": return L10n.named("系统声音采集尚未获授权。请在系统设置 → 隐私与安全性 → 屏幕与系统音频录制中允许%1$@，然后重新打开%1$@。")
    case "audio_application_unavailable": return L10n.text("所选应用当前没有可采集的音频进程，请开始播放声音后重试，或选择所有应用。")
    case "system_audio_format_unavailable": return L10n.text("当前音频输出格式不可用，请切换输出设备后重试。")
    case "system_audio_permission_denied": return L10n.named("系统音频录制尚未获授权。请在系统设置 → 隐私与安全性 → 屏幕与系统音频录制 → 仅系统音频录制中允许%@。")
    case "capture_entitlements_missing": return L10n.text("当前构建缺少系统音频采集所需的签名权限，请重新构建应用。")
    case "capture_service_unavailable": return L10n.named("系统音频采集服务暂不可用，请重新打开%@后重试。")
    case "system_audio_unavailable": return L10n.text("系统音频流无法启动，请检查当前音频输出设备后重试。")
    case "microphone_permission_denied": return L10n.named("请在系统设置中允许%@使用麦克风。")
    case "capture_busy": return L10n.text("另一个声笺或声席正在采集，请先停止它的会话。")
    default: return L10n.text("操作未完成") + " (\(code))"
    } }
}
public final class Engine: @unchecked Sendable {
    private let handle: UInt64
    public init() { precondition(sheng_abi_version() == 1); handle = sheng_engine_create() }
    deinit { sheng_engine_destroy(handle) }
    private func read<T: Decodable>(_ pointer: UnsafeMutablePointer<CChar>?, as: T.Type) throws -> T {
        guard let pointer else { throw EngineError(code: "engine_unavailable") }
        defer { sheng_string_free(pointer) }
        return try JSONDecoder().decode(T.self, from: Data(String(cString: pointer).utf8))
    }
    public static func languageCatalog() -> [String: [String]] {
        guard let pointer = sheng_language_catalog() else { return [:] }
        defer { sheng_string_free(pointer) }
        return (try? JSONDecoder().decode([String: [String]].self, from: Data(String(cString: pointer).utf8))) ?? [:]
    }
    public func validate(request: Data) throws {
        struct Reply: Decodable { let ok: Bool; let error: String? }
        let reply = try String(decoding: request, as: UTF8.self).withCString { try read(sheng_validate($0), as: Reply.self) }
        if !reply.ok { throw EngineError(code: reply.error ?? "invalid_configuration") }
    }
    public func start(request: Data) throws {
        struct Reply: Decodable { let ok: Bool; let error: String? }
        guard let text = String(data: request, encoding: .utf8) else { throw EngineError(code: "invalid_request") }
        let reply = try text.withCString { try read(sheng_engine_start(handle, $0), as: Reply.self) }
        if !reply.ok { throw EngineError(code: reply.error ?? "start_failed") }
    }
    public func push(_ data: Data, source: AudioSource) -> Bool {
        data.withUnsafeBytes { bytes in sheng_engine_push_pcm(handle, source.lane, bytes.bindMemory(to: UInt8.self).baseAddress, data.count) == 0 }
    }
    public static func retryDelay(attempt: UInt32) -> Int64 { sheng_retry_delay(attempt) }
    public func stop() { sheng_engine_stop(handle) }
    public func clear() { sheng_engine_clear(handle) }
    public func snapshot() throws -> EngineSnapshot { try read(sheng_engine_snapshot(handle), as: EngineSnapshot.self) }
    public func drain() throws -> [Caption] { try read(sheng_engine_drain_confirmed(handle), as: [Caption].self) }
}

public enum ServiceProbe {
    public static func check(profile: ServiceProfile, source: String, target: String, text: Bool, secrets: ServiceCredentials = ServiceCredentials()) async throws -> String {
        let request = try profile.request(input: .system, source: source, target: target, secrets: secrets)
        let id = String(decoding: request, as: UTF8.self).withCString { sheng_probe($0, text ? 1 : 0) }
        guard id != 0 else { throw EngineError(code: "configuration_invalid") }
        defer { sheng_resources_cancel(id) }
        let deadline = Date().addingTimeInterval(20)
        while true {
            guard Date() < deadline else { throw EngineError(code: "probe_timeout") }
            try Task.checkCancellation()
            guard let pointer = sheng_resources_poll(id) else { throw EngineError(code: "probe_failed") }
            let data = Data(String(cString: pointer).utf8); sheng_string_free(pointer)
            let result = try JSONSerialization.jsonObject(with: data) as? [String: Any] ?? [:]
            if result["state"] as? String == "done" {
                let value = result["value"] as? [String: Any] ?? [:]
                let duration = value["elapsedMs"] as? Int ?? 0
                let status = L10n.text(value["service"] as? String == "available" ? "连接正常" : "连接失败")
                let reason = value["reason"] as? String
                return "\(status) · \(duration) ms" + (reason.map { " · \($0)" } ?? "")
            }
            if result["state"] as? String == "failed" { throw EngineError(code: result["error"] as? String ?? "probe_failed") }
            try await Task.sleep(nanoseconds: 200_000_000)
        }
    }
}
