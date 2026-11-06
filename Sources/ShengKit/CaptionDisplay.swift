import SwiftUI

public struct CaptionDisplay: View {
    @ObservedObject private var model: SessionModel
    private let readingHistory: Bool
    public init(model: SessionModel, readingHistory: Bool = false) { self.model = model; self.readingHistory = readingHistory }
    private var alignment: HorizontalAlignment { model.subtitleAlignment == "center" ? .center : model.subtitleAlignment == "right" ? .trailing : .leading }
    private var textAlignment: TextAlignment { model.subtitleAlignment == "center" ? .center : model.subtitleAlignment == "right" ? .trailing : .leading }
    private func subtitleColor(_ source: AudioSource) -> Color {
        if source == .microphone { return Color(subtitleHex: model.microphoneHex) }
        return model.foregroundHex.isEmpty ? (model.immersive ? .white : .primary) : Color(subtitleHex: model.foregroundHex)
    }
    public var body: some View {
        LazyVStack(alignment: alignment, spacing: 18) {
            ForEach(readingHistory || !model.customSubtitleCount ? model.presentation.captions : Array(model.presentation.captions.suffix(model.visibleLines))) { caption in
                VStack(alignment: alignment, spacing: 7) {
                    if model.showSourceLabel { Label(caption.source.title, systemImage: caption.source == .system ? "speaker.wave.2" : "mic").font(.caption).foregroundStyle(.secondary) }
                    if model.showTime { Text(Date(timeIntervalSince1970: Double(caption.timestampMs) / 1000), style: .time).font(.caption).foregroundStyle(.secondary) }
                    if model.displayMode != "translation" { Text(caption.original).foregroundStyle(model.displayMode == "bilingual" ? subtitleColor(caption.source).opacity(0.70) : subtitleColor(caption.source)) }
                    if model.displayMode != "original" && !caption.translation.isEmpty { Text(caption.translation).foregroundStyle(subtitleColor(caption.source)) }
                }.frame(maxWidth: .infinity, alignment: model.subtitleAlignment == "center" ? .center : model.subtitleAlignment == "right" ? .trailing : .leading)
                if model.showDividers && !model.immersive { Divider().opacity(0.5) }
            }
            ForEach(model.showDrafts && !readingHistory ? (model.snapshot?.tracks.filter { model.presentation.shows($0) } ?? []) : [], id: \.source) { track in
                let confirmed = model.snapshot?.recent.last(where: { $0.source == track.source })
                if !track.isFinal && !track.original.isEmpty && (confirmed?.original != track.original || confirmed?.translation != track.translation) {
                    VStack(alignment: alignment, spacing: 7) {
                        if model.displayMode != "translation" { Text(track.original).foregroundStyle(model.displayMode == "bilingual" ? subtitleColor(track.source).opacity(0.70) : subtitleColor(track.source)) }
                        if model.displayMode != "original" && !track.translation.isEmpty { Text(track.translation).foregroundStyle(subtitleColor(track.source)) }
                    }
                }
                if model.displayMode != "translation" && !track.pendingOriginal.isEmpty { Text(track.pendingOriginal).foregroundStyle(.secondary) }
            }
            if model.presentation.captions.isEmpty && (model.snapshot?.tracks.allSatisfy({ $0.original.isEmpty && $0.pendingOriginal.isEmpty }) ?? true) {
                Text(LocalizedStringKey(model.isCapturing ? "正在聆听…" : "字幕将在这里显示")).foregroundStyle(.secondary)
            }
        }
        .multilineTextAlignment(textAlignment)
        .font(model.fontName.isEmpty ? .system(size: model.fontSize) : .custom(model.fontName, size: model.fontSize))
        .foregroundStyle(model.foregroundHex.isEmpty ? (model.immersive ? Color.white : Color.primary) : Color(subtitleHex: model.foregroundHex))
        .shadow(color: model.immersive ? .black.opacity(0.85) : .clear, radius: 2, x: 0, y: 1)
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(20)
        .accessibilityElement(children: .contain)
    }
}

extension Color {
    /// First custom colour when leaving theme colours; plain white is unreadable on the light overlay background.
    static func customSubtitleStartHex(immersive: Bool, dark: Bool) -> String { immersive || dark ? "FFFFFF" : "1D1D1F" }
    init(subtitleHex: String) { let hex = UInt32(subtitleHex.trimmingCharacters(in: CharacterSet(charactersIn: "#")), radix: 16) ?? 0xFFFFFF; self.init(.sRGB, red: Double((hex >> 16) & 255) / 255, green: Double((hex >> 8) & 255) / 255, blue: Double(hex & 255) / 255, opacity: 1) }
}
