import Foundation
import Speech

/// Speech recognition assets (apple.md §1): which locales exist and whether they are installed.
/// Nothing here downloads unless `prepare` is called for an explicit user action.
enum SpeechAssets {
    /// Status strings shared with ShengKit (`installed` means ready to use).
    @available(macOS 26, *)
    static func name(_ status: AssetInventory.Status) -> String {
        switch status {
        case .installed: return "installed"
        case .downloading: return "downloading"
        case .supported: return "supported"
        case .unsupported: return "unsupported"
        @unknown default: return "unknown"
        }
    }

    @available(macOS 26, *)
    static func transcriber(_ locale: Locale) -> SpeechTranscriber {
        SpeechTranscriber(
            locale: locale,
            transcriptionOptions: [],
            reportingOptions: [.volatileResults, .fastResults],
            attributeOptions: []
        )
    }

    @available(macOS 26, *)
    static func status(_ identifier: String) async -> AssetInventory.Status {
        await AssetInventory.status(forModules: [transcriber(Locale(identifier: identifier))])
    }

    /// `{"available":Bool,"locales":[{"identifier","status"}]}`.
    static func inventory() async -> String {
        guard #available(macOS 26, *), SpeechTranscriber.isAvailable else {
            return json(["available": false, "locales": [Any]()])
        }
        var rows: [[String: String]] = []
        for locale in await SpeechTranscriber.supportedLocales {
            let identifier = locale.identifier(.bcp47)
            let status = await AssetInventory.status(forModules: [transcriber(locale)])
            rows.append(["identifier": identifier, "status": name(status)])
        }
        rows.sort { ($0["identifier"] ?? "") < ($1["identifier"] ?? "") }
        return json(["available": true, "locales": rows])
    }

    /// Downloads and installs the recognizer for `identifier`; true once it is installed.
    static func prepare(_ identifier: String) async -> Bool {
        guard #available(macOS 26, *), SpeechTranscriber.isAvailable else { return false }
        let module = transcriber(Locale(identifier: identifier))
        do {
            if let request = try await AssetInventory.assetInstallationRequest(supporting: [module]) {
                try await request.downloadAndInstall()
            }
        } catch {
            return false
        }
        return await AssetInventory.status(forModules: [module]) == .installed
    }
}
