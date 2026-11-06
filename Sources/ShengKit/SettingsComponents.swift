import SwiftUI

/// Building blocks for the settings pages: a titled card of rows, each row a label with its control on the right.
struct SettingsGroup<Content: View>: View {
    let title: String
    @ViewBuilder let content: () -> Content
    init(_ title: String, @ViewBuilder content: @escaping () -> Content) {
        self.title = title
        self.content = content
    }
    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(LocalizedStringKey(title))
                .font(.system(size: 11, weight: .semibold))
                .foregroundStyle(.secondary)
                .padding(.leading, 2)
            // Each row draws a divider above itself; clipping hides the first one.
            VStack(alignment: .leading, spacing: 0) { content() }
                .padding(.top, -1)
                .padding(.horizontal, 14)
                .background(ShengStyle.surface)
                .clipShape(RoundedRectangle(cornerRadius: 12))
                .overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(ShengStyle.border.opacity(0.55)))
        }
    }
}

struct SettingRow<Trailing: View>: View {
    let title: String
    var detail: String? = nil
    @ViewBuilder let trailing: () -> Trailing
    init(_ title: String, detail: String? = nil, @ViewBuilder trailing: @escaping () -> Trailing) {
        self.title = title
        self.detail = detail
        self.trailing = trailing
    }
    var body: some View {
        VStack(spacing: 0) {
            Divider().opacity(0.6)
            // Side by side when there is room; otherwise the control moves under its label instead of clipping.
            ViewThatFits(in: .horizontal) {
                HStack(spacing: 12) {
                    label
                    Spacer(minLength: 8)
                    trailing()
                }
                VStack(alignment: .leading, spacing: 8) {
                    label
                    trailing()
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            }
            .padding(.vertical, 8)
            .frame(minHeight: 40)
        }
    }
    private var label: some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(LocalizedStringKey(title)).font(.system(size: 12.5))
            if let detail {
                Text(LocalizedStringKey(detail)).font(.system(size: 11)).foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
    }
}

struct ToggleSwitch: View {
    let title: String
    @Binding var isOn: Bool
    var body: some View {
        Button { isOn.toggle() } label: {
            Capsule().fill(isOn ? ShengStyle.button : ShengStyle.border).frame(width: 31, height: 18)
                .overlay(Circle().fill(.white).frame(width: 14, height: 14).shadow(radius: 0.5).offset(x: isOn ? 6.5 : -6.5))
                .animation(.easeInOut(duration: 0.15), value: isOn)
        }
        .buttonStyle(.plain)
        .accessibilityLabel(Text(LocalizedStringKey(title)))
        .accessibilityValue(Text(LocalizedStringKey(isOn ? "开启" : "关闭")))
    }
}

/// A row whose only control is a switch; the whole row toggles.
struct SwitchSetting: View {
    let title: String
    var detail: String? = nil
    @Binding var isOn: Bool
    var body: some View {
        SettingRow(title, detail: detail) { ToggleSwitch(title: title, isOn: $isOn) }
            .contentShape(Rectangle())
            .onTapGesture { isOn.toggle() }
    }
}

struct SegmentedChoice: View {
    let title: String
    @Binding var selection: String
    let options: [(String, String)]
    var body: some View {
        HStack(spacing: 2) {
            ForEach(options, id: \.0) { option in
                let selected = option.0 == selection
                Button { selection = option.0 } label: {
                    Text(LocalizedStringKey(option.1))
                        .font(.system(size: 12, weight: selected ? .semibold : .regular))
                        .foregroundStyle(selected ? Color.primary : Color.secondary)
                        // Shrink rather than truncate in narrow cards and longer localizations.
                        .lineLimit(1).minimumScaleFactor(0.75)
                        .padding(.horizontal, 8).frame(minWidth: 36).frame(height: 24)
                        .background(selected ? ShengStyle.surface : .clear, in: RoundedRectangle(cornerRadius: 6))
                        .shadow(color: .black.opacity(selected ? 0.12 : 0), radius: 1, y: 0.5)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityAddTraits(selected ? .isSelected : [])
            }
        }
        .padding(2)
        .background(ShengStyle.field, in: RoundedRectangle(cornerRadius: 8))
        .accessibilityElement(children: .contain)
        .accessibilityLabel(Text(LocalizedStringKey(title)))
    }
}

struct StatusChip: View {
    enum Tone { case ok, warning, accent, neutral }
    let text: String
    var tone: Tone = .neutral
    private var colors: (Color, Color) {
        switch tone {
        case .ok: return (Color.green.opacity(0.14), Color(red: 0.12, green: 0.42, blue: 0.2))
        case .warning: return (Color.orange.opacity(0.16), Color(red: 0.55, green: 0.29, blue: 0.05))
        case .accent: return (ShengStyle.accent.opacity(0.12), ShengStyle.accent)
        case .neutral: return (Color.primary.opacity(0.06), Color.secondary)
        }
    }
    var body: some View {
        Text(LocalizedStringKey(text)).font(.system(size: 11, weight: .medium)).lineLimit(1)
            .padding(.horizontal, 7).padding(.vertical, 2)
            .foregroundStyle(colors.1)
            .background(colors.0, in: Capsule())
    }
}

/// A bordered secondary button, so actions never read as plain labels.
struct BorderedButtonStyle: ButtonStyle {
    var destructive = false
    func makeBody(configuration: Configuration) -> some View {
        Surface(configuration: configuration, destructive: destructive)
    }
    private struct Surface: View {
        let configuration: ButtonStyle.Configuration
        let destructive: Bool
        @Environment(\.isEnabled) private var enabled
        var body: some View {
            configuration.label.font(.system(size: 12, weight: .medium))
                .foregroundStyle(destructive ? Color.red : Color.primary)
                .padding(.horizontal, 10).frame(minWidth: 26, minHeight: 26)
                .background(ShengStyle.surface.opacity(configuration.isPressed ? 0.7 : 1), in: RoundedRectangle(cornerRadius: 6))
                .overlay(RoundedRectangle(cornerRadius: 6).strokeBorder(ShengStyle.border))
                .opacity(enabled ? 1 : 0.45)
                .contentShape(RoundedRectangle(cornerRadius: 6))
        }
    }
}

struct TextInput: View {
    let title: String
    @Binding var text: String
    var placeholder = ""
    @FocusState private var focused: Bool
    var body: some View {
        TextField(LocalizedStringKey(placeholder), text: $text)
            .textFieldStyle(.plain).focused($focused).font(.system(size: 12.5))
            .padding(.horizontal, 9).frame(height: 28)
            .background(ShengStyle.field, in: RoundedRectangle(cornerRadius: 6))
            .overlay(RoundedRectangle(cornerRadius: 6).strokeBorder(focused ? ShengStyle.accent.opacity(0.8) : ShengStyle.border.opacity(0.6), lineWidth: focused ? 1.5 : 1))
            .frame(maxWidth: 320)
            .accessibilityLabel(Text(LocalizedStringKey(title)))
    }
}

/// Preset colours plus a custom picker; an empty hex means "follow the theme".
struct ColorSwatches: View {
    let title: String
    @Binding var hex: String
    let presets: [String]
    var allowsAutomatic = false
    var body: some View {
        HStack(spacing: 8) {
            if allowsAutomatic {
                swatch(selected: hex.isEmpty, label: "自动") {
                    Circle().fill(LinearGradient(colors: [.white, .black], startPoint: .topLeading, endPoint: .bottomTrailing))
                } action: { hex = "" }
            }
            ForEach(presets, id: \.self) { value in
                swatch(selected: hex.uppercased() == value, label: "#" + value) { Circle().fill(Color(subtitleHex: value)) } action: { hex = value }
            }
            ColorPicker(LocalizedStringKey(title), selection: Binding(get: { Color(subtitleHex: hex.isEmpty ? "FFFFFF" : hex) }, set: { hex = $0.hexString ?? hex }), supportsOpacity: false)
                .labelsHidden()
        }
    }
    private func swatch<S: View>(selected: Bool, label: String, @ViewBuilder fill: () -> S, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            fill().frame(width: 18, height: 18)
                .overlay(Circle().strokeBorder(Color.primary.opacity(0.18)))
                .padding(2)
                .overlay(Circle().strokeBorder(selected ? ShengStyle.accent : .clear, lineWidth: 2))
        }
        .buttonStyle(.plain)
        .accessibilityLabel(Text(LocalizedStringKey(label)))
        .accessibilityAddTraits(selected ? .isSelected : [])
    }
}

extension Color {
    var hexString: String? {
        guard let color = NSColor(self).usingColorSpace(.sRGB) else { return nil }
        return String(format: "%02X%02X%02X", Int((color.redComponent * 255).rounded()), Int((color.greenComponent * 255).rounded()), Int((color.blueComponent * 255).rounded()))
    }
}

struct PageHeader<Trailing: View>: View {
    let title: String
    var subtitle: String? = nil
    @ViewBuilder let trailing: () -> Trailing
    init(_ title: String, subtitle: String? = nil, @ViewBuilder trailing: @escaping () -> Trailing) {
        self.title = title
        self.subtitle = subtitle
        self.trailing = trailing
    }
    var body: some View {
        HStack(alignment: .bottom) {
            VStack(alignment: .leading, spacing: 4) {
                Text(LocalizedStringKey(title)).font(.system(size: 20, weight: .semibold))
                if let subtitle { Text(LocalizedStringKey(subtitle)).font(.system(size: 12)).foregroundStyle(.secondary) }
            }
            Spacer()
            trailing()
        }
    }
}

extension PageHeader where Trailing == EmptyView {
    init(_ title: String, subtitle: String? = nil) { self.init(title, subtitle: subtitle) { EmptyView() } }
}

/// A bordered action with an icon; inside `AdaptiveActions` it collapses to the icon when space runs out.
struct ActionButton: View {
    let title: String
    let icon: String
    var destructive = false
    let action: () -> Void
    init(_ title: String, icon: String, destructive: Bool = false, action: @escaping () -> Void) {
        self.title = title; self.icon = icon; self.destructive = destructive; self.action = action
    }
    var body: some View {
        Button(action: action) { Label(LocalizedStringKey(title), systemImage: icon) }
            .buttonStyle(BorderedButtonStyle(destructive: destructive))
            // Never squeeze the title away; surrounding rows wrap or switch to icons instead.
            .fixedSize()
            .help(Text(LocalizedStringKey(title)))
            .accessibilityLabel(Text(LocalizedStringKey(title)))
    }
}

/// A row of buttons that shows titles when they fit, icons only when they do not, and wraps as a last resort.
struct AdaptiveActions<Content: View>: View {
    @ViewBuilder let content: () -> Content
    var body: some View {
        ViewThatFits(in: .horizontal) {
            HStack(spacing: 6) { content() }.labelStyle(.titleAndIcon)
            HStack(spacing: 6) { content() }.labelStyle(.iconOnly)
            VStack(alignment: .leading, spacing: 6) { content() }.labelStyle(.iconOnly)
        }
    }
}

/// Lets the borderless subtitle panel be dragged from an area that SwiftUI controls would otherwise swallow.
struct WindowDragArea: NSViewRepresentable {
    final class DragView: NSView {
        override var mouseDownCanMoveWindow: Bool { true }
        override func mouseDown(with event: NSEvent) { window?.performDrag(with: event) }
    }
    func makeNSView(context: Context) -> NSView { DragView() }
    func updateNSView(_ view: NSView, context: Context) {}
}
