import Foundation

/// A bounded reading buffer, independent of opt-in disk archives.
struct CaptionPresentation {
    static let capacity = 500
    static let byteLimit = 2 * 1024 * 1024
    private(set) var captions: [Caption] = []
    private var dismissedIDs = Set<String>()
    private var dismissedDrafts: [AudioSource: String] = [:]
    mutating func append(_ caption: Caption) {
        guard !dismissedIDs.contains(caption.id), caption.original.utf8.count + caption.translation.utf8.count <= Self.byteLimit else { return }
        if let index = captions.firstIndex(where: { $0.id == caption.id }) { captions[index] = caption }
        else { captions.append(caption) }
        if captions.count > Self.capacity { captions.removeFirst(captions.count - Self.capacity) }
        var bytes = captions.reduce(0) { $0 + $1.original.utf8.count + $1.translation.utf8.count }
        while bytes > Self.byteLimit, !captions.isEmpty {
            let removed = captions.removeFirst(); bytes -= removed.original.utf8.count + removed.translation.utf8.count
        }
    }
    mutating func clear(snapshot: EngineSnapshot?) {
        dismissedIDs = Set(captions.map(\.id) + (snapshot?.recent.map(\.id) ?? []))
        captions.removeAll()
        dismissedDrafts = Dictionary(uniqueKeysWithValues: (snapshot?.tracks ?? []).map { ($0.source, Self.fingerprint($0)) })
    }
    func shows(_ track: CaptionTrack) -> Bool { dismissedDrafts[track.source] != Self.fingerprint(track) }
    private static func fingerprint(_ track: CaptionTrack) -> String { "\(track.original)\u{0}\(track.translation)\u{0}\(track.pendingOriginal)" }
}
