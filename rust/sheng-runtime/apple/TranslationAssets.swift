import Foundation
import Translation

/// Apple translation: language availability, preparation and single requests.
/// Translation never downloads; only `prepare` (an explicit user action) may.
enum TranslationAssets {
    /// Failure names for `translate`; Rust maps them to translation errors.
    enum Failure: String, Error {
        case unavailable
        case unsupported
        case assetsMissing = "assets_missing"
        case failed
    }

    @available(macOS 15, *)
    static func name(_ status: LanguageAvailability.Status) -> String {
        switch status {
        case .installed: return "installed"
        case .supported: return "supported"
        case .unsupported: return "unsupported"
        @unknown default: return "unsupported"
        }
    }

    /// `{"available":Bool,"languages":[maximal identifiers]}`.
    static func languages() async -> String {
        guard #available(macOS 15, *) else {
            return json(["available": false, "languages": [String]()])
        }
        let languages = await LanguageAvailability().supportedLanguages.map(\.maximalIdentifier)
        return json(["available": true, "languages": languages.sorted()])
    }

    /// `installed`, `supported`, `unsupported` or `unavailable` (no Translation framework).
    static func status(source: String, target: String) async -> String {
        guard #available(macOS 15, *) else { return "unavailable" }
        let status = await LanguageAvailability().status(
            from: Locale.Language(identifier: source), to: Locale.Language(identifier: target)
        )
        return name(status)
    }

    /// Asks the system to download the pair; true once it is installed.
    static func prepare(source: String, target: String) async -> Bool {
        guard #available(macOS 15, *) else { return false }
        do {
            _ = try await TranslationHost.perform(
                source: source, target: target, timeout: TranslationHost.prepareTimeout
            ) { session in
                try await session.prepareTranslation()
                return ""
            }
        } catch {
            return false
        }
        return await status(source: source, target: target) == "installed"
    }

    static func translate(_ text: String, source: String, target: String) async -> Result<String, Failure> {
        guard #available(macOS 15, *) else { return .failure(.unavailable) }
        switch await status(source: source, target: target) {
        case "installed": break
        case "supported": return .failure(.assetsMissing)
        default: return .failure(.unsupported)
        }
        do {
            let translated = try await TranslationHost.perform(source: source, target: target) { session in
                try await session.translate(text).targetText
            }
            return .success(translated)
        } catch {
            return .failure(.failed)
        }
    }
}
