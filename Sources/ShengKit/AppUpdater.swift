import AppKit

/// Checks the app's GitHub Releases for a newer version and installs it on request.
/// Checking only reads public release metadata; nothing is downloaded or replaced without a click.
@MainActor public final class AppUpdater: ObservableObject {
    public enum State: Equatable {
        case idle, checking, upToDate, available(UpdateRelease), downloading, installing, failed(String)
    }
    @Published public private(set) var state: State = .idle
    @Published public var automaticChecks = true { didSet { save() } }
    @Published public private(set) var lastCheck: Date?
    public let configuration: UpdateConfiguration?
    private let file: URL
    private var task: Task<Void, Never>?

    private struct Saved: Codable { var automaticChecks: Bool; var lastCheck: Date? }

    public init(root: URL, configuration: UpdateConfiguration? = .main) {
        self.configuration = configuration
        file = root.appendingPathComponent("updates-v1.json")
        if let data = try? Data(contentsOf: file), let saved = try? JSONDecoder().decode(Saved.self, from: data) {
            automaticChecks = saved.automaticChecks; lastCheck = saved.lastCheck
        }
    }

    public var currentVersion: String { configuration?.currentVersion.description ?? (Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "—") }
    public var available: UpdateRelease? { if case .available(let release) = state { return release }; return nil }
    public var isBusy: Bool { [.checking, .downloading, .installing].contains(state) }

    /// Called at launch: checks quietly at most once a day when automatic checks are on.
    public func checkIfDue() {
        guard automaticChecks, configuration != nil, UpdateSchedule.isDue(lastCheck: lastCheck) else { return }
        check(quietly: true)
    }

    public func check(quietly: Bool = false) {
        guard let configuration, !isBusy else { return }
        state = .checking
        task = Task {
            do {
                var request = URLRequest(url: configuration.latestReleaseURL, cachePolicy: .reloadIgnoringLocalCacheData, timeoutInterval: 20)
                request.setValue("application/vnd.github+json", forHTTPHeaderField: "Accept")
                let (data, response): (Data, URLResponse)
                do { (data, response) = try await URLSession.shared.data(for: request) } catch { throw UpdateError.network }
                guard (response as? HTTPURLResponse)?.statusCode == 200 else { throw UpdateError.network }
                let release = try UpdateRelease.decode(data, repository: configuration.repository)
                lastCheck = Date(); save()
                state = configuration.currentVersion < release.version ? .available(release) : .upToDate
            } catch {
                state = quietly ? .idle : .failed(error.localizedDescription)
            }
        }
    }

    /// Downloads, verifies and installs the available release, then quits so the new copy opens.
    public func install() {
        guard let configuration, let release = available else { return }
        state = .downloading
        task = Task {
            do {
                let app = Bundle.main.bundleURL
                try UpdateInstaller.checkLocation(app)
                async let archive = UpdateInstaller.download(release.archive, limit: UpdateRelease.sizeLimit)
                async let signature = UpdateInstaller.download(release.signature, limit: 4096)
                let (bytes, sig) = try await (archive, signature)
                try UpdateVerifier.verify(bytes, signature: sig, publicKey: configuration.publicKey)
                state = .installing
                let staged = try UpdateInstaller.stage(bytes, version: release.version, bundleIdentifier: configuration.bundleIdentifier)
                try UpdateInstaller.replace(app, with: staged)
                UpdateInstaller.relaunch(app)
                NSApp.terminate(nil)
            } catch is CancellationError {
                state = .available(release)
            } catch {
                state = .failed(error.localizedDescription)
            }
        }
    }

    private func save() {
        guard let data = try? JSONEncoder().encode(Saved(automaticChecks: automaticChecks, lastCheck: lastCheck)) else { return }
        try? data.write(to: file, options: .atomic)
    }
}
