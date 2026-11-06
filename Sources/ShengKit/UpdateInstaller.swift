import AppKit

/// Downloads, verifies and swaps in a new app bundle. Nothing is replaced until the archive's
/// signature, bundle identifier, version and code signature have all been checked.
enum UpdateInstaller {
    static func download(_ url: URL, limit: Int64) async throws -> Data {
        var request = URLRequest(url: url, cachePolicy: .reloadIgnoringLocalCacheData, timeoutInterval: 120)
        request.setValue("application/octet-stream", forHTTPHeaderField: "Accept")
        let (data, response): (Data, URLResponse)
        do { (data, response) = try await URLSession.shared.data(for: request) } catch is CancellationError { throw CancellationError() } catch { throw UpdateError.network }
        guard let http = response as? HTTPURLResponse, http.statusCode == 200, response.url?.scheme == "https" else { throw UpdateError.network }
        guard Int64(data.count) <= limit else { throw UpdateError.tooLarge }
        return data
    }

    /// Where the running app can be replaced; explains why not otherwise.
    static func checkLocation(_ app: URL) throws {
        if app.path.contains("/AppTranslocation/") {
            throw UpdateError.notInstallable(L10n.text("请先把应用移到“应用程序”文件夹并重新打开，再安装更新。"))
        }
        guard FileManager.default.isWritableFile(atPath: app.deletingLastPathComponent().path) else {
            throw UpdateError.notInstallable(L10n.text("没有写入应用所在文件夹的权限，请从发布页手动下载新版本。"))
        }
    }

    /// Unpacks the verified archive and checks the app inside; returns the staged bundle.
    static func stage(_ archive: Data, version: AppVersion, bundleIdentifier: String) throws -> URL {
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent("update-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
        let zip = folder.appendingPathComponent("update.zip")
        try archive.write(to: zip, options: .atomic)
        let expanded = folder.appendingPathComponent("expanded")
        guard run("/usr/bin/ditto", ["-x", "-k", zip.path, expanded.path]) else { throw UpdateError.installFailed }
        let apps = (try? FileManager.default.contentsOfDirectory(at: expanded, includingPropertiesForKeys: nil))?.filter { $0.pathExtension == "app" } ?? []
        guard apps.count == 1, let app = apps.first, let info = Bundle(url: app)?.infoDictionary else { throw UpdateError.installFailed }
        guard info["CFBundleIdentifier"] as? String == bundleIdentifier,
              (info["CFBundleShortVersionString"] as? String).flatMap(AppVersion.init) == version else {
            throw UpdateError.notInstallable(L10n.text("安装包与当前应用不一致，已停止更新。"))
        }
        guard run("/usr/bin/codesign", ["--verify", "--deep", "--strict", app.path]) else { throw UpdateError.badSignature }
        return app
    }

    /// Replaces the bundle in place; the running process keeps its already-open files.
    static func replace(_ current: URL, with staged: URL) throws {
        do { _ = try FileManager.default.replaceItemAt(current, withItemAt: staged, backupItemName: nil, options: []) }
        catch { throw UpdateError.installFailed }
    }

    /// Opens the new copy once this process has exited.
    static func relaunch(_ app: URL) {
        let pid = ProcessInfo.processInfo.processIdentifier
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/bin/sh")
        process.arguments = ["-c", "while /bin/kill -0 \"$1\" 2>/dev/null; do /bin/sleep 0.2; done; /usr/bin/open \"$2\"", "sh", String(pid), app.path]
        try? process.run()
    }

    private static func run(_ tool: String, _ arguments: [String]) -> Bool {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: tool); process.arguments = arguments
        process.standardOutput = FileHandle.nullDevice; process.standardError = FileHandle.nullDevice
        do { try process.run() } catch { return false }
        process.waitUntilExit()
        return process.terminationStatus == 0
    }
}
