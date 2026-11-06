import AppKit
import SwiftUI
import Translation

/// Hidden SwiftUI host that obtains a `TranslationSession` (apple.md §2). The window has no
/// visible content, is created only for one piece of work and is released right after it.
@available(macOS 15, *)
@MainActor
final class TranslationHost {
    typealias Work = @MainActor (TranslationSession) async throws -> String

    /// Work that has not finished by then fails; the host is released either way.
    static let requestTimeout: Duration = .seconds(30)
    /// Preparing may wait for the user to confirm a download and for the download itself.
    static let prepareTimeout: Duration = .seconds(600)

    private var window: NSWindow?
    private var continuation: CheckedContinuation<String, Error>?

    static func perform(
        source: String, target: String, timeout: Duration = requestTimeout, work: @escaping Work
    ) async throws -> String {
        let configuration = TranslationSession.Configuration(
            source: Locale.Language(identifier: source), target: Locale.Language(identifier: target)
        )
        let host = TranslationHost()
        return try await withCheckedThrowingContinuation { continuation in
            host.open(configuration, timeout: timeout, work: work, continuation: continuation)
        }
    }

    private func open(
        _ configuration: TranslationSession.Configuration,
        timeout: Duration,
        work: @escaping Work,
        continuation: CheckedContinuation<String, Error>
    ) {
        self.continuation = continuation
        let view = HostView(configuration: configuration) { [self] session in
            do {
                complete(.success(try await work(session)))
            } catch {
                complete(.failure(error))
            }
        }
        let window = NSWindow(
            contentRect: NSRect(x: -10_000, y: -10_000, width: 1, height: 1),
            styleMask: [.borderless], backing: .buffered, defer: false
        )
        window.isReleasedWhenClosed = false
        window.alphaValue = 0
        window.ignoresMouseEvents = true
        window.hasShadow = false
        window.collectionBehavior = [.transient, .ignoresCycle]
        window.contentView = NSHostingView(rootView: view)
        window.orderFrontRegardless()
        self.window = window
        Task { @MainActor [self] in
            try? await Task.sleep(for: timeout)
            complete(.failure(HostTimeout()))
        }
    }

    private func complete(_ result: Result<String, Error>) {
        guard let continuation else { return }
        self.continuation = nil
        window?.orderOut(nil)
        window?.contentView = nil
        window?.close()
        window = nil
        continuation.resume(with: result)
    }
}

struct HostTimeout: Error {}

@available(macOS 15, *)
private struct HostView: View {
    let configuration: TranslationSession.Configuration
    let action: @MainActor (TranslationSession) async -> Void

    var body: some View {
        Color.clear
            .frame(width: 1, height: 1)
            .translationTask(configuration) { session in await action(session) }
    }
}
