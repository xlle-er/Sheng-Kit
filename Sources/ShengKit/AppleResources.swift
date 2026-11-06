import SwiftUI
import CShengEngine

public enum AppleResources {
    private static func resourceStatus(_ code: String) -> String {
        switch code { case "installed": return L10n.text("已安装"); case "supported", "supportedNotInstalled", "notInstalled": return L10n.text("可下载"); case "downloading": return L10n.text("正在下载"); default: return L10n.text("不可用") }
    }
    static func inspect(source: String, target: String) async throws -> AppleResourceReport {
        let id = source.withCString { s in target.withCString { t in sheng_resources_start(0, s, t) } }
        guard id != 0 else { throw EngineError(code: "resource_busy") }
        defer { sheng_resources_cancel(id) }
        let deadline = Date().addingTimeInterval(25)
        while Date() < deadline {
            try Task.checkCancellation()
            guard let pointer = sheng_resources_poll(id) else { throw EngineError(code: "resource_unavailable") }
            let data = Data(String(cString: pointer).utf8); sheng_string_free(pointer)
            let result = try JSONSerialization.jsonObject(with: data) as? [String: Any] ?? [:]
            if result["state"] as? String == "failed" { throw EngineError(code: result["error"] as? String ?? "resource_unavailable") }
            if result["state"] as? String == "done" {
                let value = result["value"] as? [String: Any] ?? [:]
                let speech = value["speech"] as? [String: Any] ?? [:]
                let rows = value["languages"] as? [[String: String]] ?? []
                return AppleResourceReport(available: speech["available"] as? Bool == true, source: source, target: target, translation: value["translation"] as? String ?? "unavailable", languages: rows.compactMap { row in guard let code = row["code"], let status = row["status"] else { return nil }; return AppleResourceLanguage(code: code, status: status) })
            }
            try await Task.sleep(nanoseconds: 250_000_000)
        }
        throw EngineError(code: "probe_timeout")
    }
    public static func perform(kind: UInt32, source: String, target: String) async throws -> String {
        let id = source.withCString { s in target.withCString { t in sheng_resources_start(kind, s, t) } }
        guard id != 0 else { throw EngineError(code: "resource_busy") }
        defer { sheng_resources_cancel(id) }
        let deadline = Date().addingTimeInterval(kind == 1 || kind == 2 ? 600 : 25)
        while true {
            guard Date() < deadline else { throw EngineError(code: "probe_timeout") }
            try Task.checkCancellation()
            guard let pointer = sheng_resources_poll(id) else { throw EngineError(code: "resource_unavailable") }
            let data = Data(String(cString: pointer).utf8); sheng_string_free(pointer)
            let value = try JSONSerialization.jsonObject(with: data) as? [String: Any] ?? [:]
            switch value["state"] as? String {
            case "done":
                guard kind == 0 else { return L10n.text("准备完成") }
                let details = value["value"] as? [String: Any] ?? [:]
                let speech = details["speech"] as? [String: Any] ?? [:]
                guard speech["available"] as? Bool == true else { return L10n.text("本机不支持 Apple 本地服务，请使用云服务。") }
                let locales = speech["locales"] as? [[String: Any]] ?? []
                let installed = locales.filter { $0["status"] as? String == "installed" }.count
                let lines = locales.map { "\($0["identifier"] as? String ?? "") · \(resourceStatus($0["status"] as? String ?? "unavailable"))" }.joined(separator: "\n")
                return L10n.text("已安装识别语言") + ": \(installed) · " + L10n.text("翻译语言资源") + ": " + resourceStatus(details["translation"] as? String ?? "unavailable") + "\n" + lines
            case "failed": throw EngineError(code: value["error"] as? String ?? "resource_unavailable")
            default: try await Task.sleep(nanoseconds: 250_000_000)
            }
        }
    }
}
struct AppleResourceLanguage { let code: String; let status: String }
struct AppleResourceReport {
    let available: Bool
    let source: String
    let target: String
    let translation: String
    let languages: [AppleResourceLanguage]
}
