import SwiftUI

/// Explicit sample of the overlay's look; separate from live and archived caption state.
struct OverlaySample: View {
    @ObservedObject var model: SessionModel
    var scale: Double = 0.75
    @Environment(\.colorScheme) private var colorScheme

    private var dark: Bool {
        switch model.overlayBackground {
        case "dark": return true
        case "light": return false
        default: return model.theme == "system" ? colorScheme == .dark : model.theme == "dark"
        }
    }
    private var alignment: HorizontalAlignment { model.subtitleAlignment == "center" ? .center : model.subtitleAlignment == "right" ? .trailing : .leading }
    private var textAlignment: TextAlignment { model.subtitleAlignment == "center" ? .center : model.subtitleAlignment == "right" ? .trailing : .leading }
    private var size: Double { max(11, model.fontSize * scale) }
    private var systemColor: Color {
        if !model.foregroundHex.isEmpty { return Color(subtitleHex: model.foregroundHex) }
        return model.immersive || dark ? .white : Color(white: 0.11)
    }

    var body: some View {
        ZStack {
            RoundedRectangle(cornerRadius: 10).fill(Color(red: 0.23, green: 0.24, blue: 0.27))
            VStack(alignment: alignment, spacing: 6) {
                Text("示例文字").font(.system(size: 10)).foregroundStyle(secondary)
                if model.showSessionInfo {
                    Text("Apple 本地 · 中文 → 英语").font(.system(size: 10.5)).foregroundStyle(secondary)
                }
                lane(source: "系统声音", original: "我们下周再讨论这个方案的细节。", translation: "Let's discuss the details of this plan next week.", color: systemColor)
                if model.showDividers && !model.immersive { Divider().opacity(0.5) }
                lane(source: "麦克风", original: "好的，我先整理一下要点。", translation: "Sure, I'll put together the key points first.", color: Color(subtitleHex: model.microphoneHex))
            }
            .multilineTextAlignment(textAlignment)
            // Wrap the sample instead of truncating it at large font sizes.
            .fixedSize(horizontal: false, vertical: true)
            .frame(maxWidth: .infinity, alignment: Alignment(horizontal: alignment, vertical: .center))
            .padding(.horizontal, 16).padding(.vertical, 12)
            .background {
                if !model.immersive {
                    RoundedRectangle(cornerRadius: 12).fill((dark ? Color(white: 0.1) : Color(white: 0.97)).opacity(model.overlayOpacity))
                }
            }
            .shadow(color: model.immersive ? .black.opacity(0.85) : .clear, radius: 2, y: 1)
            .padding(14)
        }
        .accessibilityElement(children: .combine)
        .accessibilityLabel("浮窗预览，示例文字")
    }

    private var secondary: Color { (model.immersive || dark ? Color.white : Color.black).opacity(0.6) }

    @ViewBuilder private func lane(source: String, original: String, translation: String, color: Color) -> some View {
        VStack(alignment: alignment, spacing: 3) {
            if model.showSourceLabel { Text(LocalizedStringKey(source)).font(.system(size: 10)).foregroundStyle(secondary) }
            if model.displayMode != "translation" {
                Text(verbatim: original).font(font(size * (model.displayMode == "bilingual" ? 0.82 : 1)))
                    .foregroundStyle(model.displayMode == "bilingual" ? color.opacity(0.72) : color)
            }
            if model.displayMode != "original" { Text(translation).font(font(size)).foregroundStyle(color) }
        }
    }
    private func font(_ size: Double) -> Font { model.fontName.isEmpty ? .system(size: size, weight: .medium) : .custom(model.fontName, size: size) }
}
