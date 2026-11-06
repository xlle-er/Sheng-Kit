import SwiftUI

private func adaptive(_ light: (Double,Double,Double), _ dark: (Double,Double,Double)) -> Color {
    Color(nsColor: NSColor(name: nil) { appearance in
        let rgb = appearance.bestMatch(from: [.darkAqua,.aqua]) == .darkAqua ? dark : light
        return NSColor(srgbRed: rgb.0, green: rgb.1, blue: rgb.2, alpha: 1)
    })
}
enum ShengStyle {
    /// Sage green taken from the app icon; white text on it stays readable.
    static let button = Color(red: 0.306, green: 0.490, blue: 0.341)
    static let accent = adaptive((0.306,0.490,0.341),(0.588,0.776,0.627))
    static let canvas = adaptive((0.972,0.976,0.970),(0.082,0.090,0.085))
    static let sidebar = adaptive((0.937,0.951,0.937),(0.062,0.071,0.066))
    static let surface = adaptive((1,1,1),(0.106,0.116,0.110))
    static let field = adaptive((0.958,0.966,0.957),(0.130,0.143,0.135))
    static let border = adaptive((0.84,0.87,0.84),(0.24,0.27,0.25))
}
struct QuietButtonStyle: ButtonStyle {
    var prominent = false
    func makeBody(configuration: Configuration) -> some View {
        ButtonSurface(configuration: configuration, prominent: prominent)
    }
    private struct ButtonSurface: View {
        let configuration: ButtonStyle.Configuration
        let prominent: Bool
        @Environment(\.isEnabled) private var enabled
        @State private var hovered = false
        var body: some View {
            configuration.label.font(.system(size: 12, weight: prominent ? .semibold : .medium))
                .padding(.horizontal, prominent ? 14 : 9).padding(.vertical, 8)
                .foregroundStyle(prominent ? Color.white : Color.primary.opacity(0.8))
                .background(prominent ? ShengStyle.button.opacity(configuration.isPressed ? 0.8 : 1) : hovered ? ShengStyle.accent.opacity(0.08) : Color.clear, in: RoundedRectangle(cornerRadius: 9))
                .opacity(enabled ? 1 : 0.4)
                .contentShape(RoundedRectangle(cornerRadius: 9))
                .onHover { hovered = $0; if enabled { ($0 ? NSCursor.pointingHand : NSCursor.arrow).set() } }
        }
    }
}
struct Surface<Content: View>: View {
    @ViewBuilder let content: () -> Content
    var body: some View {
        content().padding(16).frame(maxWidth: .infinity, alignment: .leading)
            .background(ShengStyle.surface, in: RoundedRectangle(cornerRadius: 13))
            .overlay(RoundedRectangle(cornerRadius: 13).strokeBorder(ShengStyle.border.opacity(0.55)))
    }
}
struct FieldRow: View {
    let title: String
    @Binding var value: String
    var secure = false
    @FocusState private var focused: Bool
    var body: some View {
        HStack(spacing: 16) {
            Text(LocalizedStringKey(title)).font(.system(size: 12)).foregroundStyle(.secondary).frame(width: 100, alignment: .leading)
            Group { if secure { SecureField("", text: $value) } else { TextField("", text: $value) } }
                .textFieldStyle(.plain).focused($focused).font(.system(size: 12)).padding(.horizontal, 11).frame(height: 36)
                .background(ShengStyle.field, in: RoundedRectangle(cornerRadius: 7))
                .overlay(RoundedRectangle(cornerRadius: 7).strokeBorder(focused ? ShengStyle.accent.opacity(0.8) : ShengStyle.border.opacity(0.45), lineWidth: focused ? 1.5 : 1))
                .accessibilityLabel(Text(LocalizedStringKey(title)))
        }
    }
}
struct ChoiceControl: View {
    /// Identifiers (profile UUIDs, language codes) are never user-facing; an unmatched value asks for a choice.
    static func displayLabel(selection: String, options: [(String, String)]) -> String {
        options.first { $0.0 == selection }?.1 ?? "请选择"
    }
    let title: String
    @Binding var selection: String
    let options: [(String,String)]
    @State private var showing = false
    @State private var hovered = false
    @Environment(\.isEnabled) private var enabled
    var body: some View {
        Button { showing.toggle() } label: {
            HStack(spacing: 10) {
                Text(LocalizedStringKey(Self.displayLabel(selection: selection, options: options))).lineLimit(1).frame(maxWidth: .infinity, alignment: .leading)
                Image(systemName: "chevron.down").font(.system(size: 8, weight: .semibold)).foregroundStyle(.secondary)
            }.font(.system(size: 12, weight: .medium)).padding(.horizontal, 11).frame(height: 36)
                .background(ShengStyle.field, in: RoundedRectangle(cornerRadius: 8))
                .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(showing ? ShengStyle.accent.opacity(0.8) : ShengStyle.border.opacity(hovered ? 1 : 0.55)))
        }.buttonStyle(.plain).opacity(enabled ? 1 : 0.45).onHover { hovered = $0 }.accessibilityLabel(Text(LocalizedStringKey(title)))
            .accessibilityValue(L10n.text(Self.displayLabel(selection: selection, options: options)))
            .popover(isPresented: $showing) { ChoiceList(selection: $selection, showing: $showing, options: options).frame(width: 250) }
    }
}
private struct ChoiceList: View {
    @Binding var selection: String
    @Binding var showing: Bool
    let options: [(String,String)]
    @State private var search = ""
    var body: some View {
        VStack(spacing: 8) {
            if options.count > 12 { HStack { Image(systemName: "magnifyingglass").foregroundStyle(.secondary); TextField("搜索", text: $search).textFieldStyle(.plain) }.padding(10).background(ShengStyle.field, in: RoundedRectangle(cornerRadius: 7)) }
            ScrollView {
                LazyVStack(spacing: 3) {
                    ForEach(options.filter { search.isEmpty || L10n.text($0.1).localizedCaseInsensitiveContains(search) || $0.0.localizedCaseInsensitiveContains(search) }, id: \.0) { value in
                        Button { selection = value.0; showing = false } label: {
                            HStack { Text(LocalizedStringKey(value.1)).lineLimit(1); Spacer(); if selection == value.0 { Image(systemName: "checkmark").foregroundStyle(ShengStyle.accent) } }
                                .font(.system(size: 12)).padding(.horizontal, 10).padding(.vertical, 9)
                                .background(selection == value.0 ? ShengStyle.accent.opacity(0.09) : .clear, in: RoundedRectangle(cornerRadius: 7))
                        }.buttonStyle(.plain)
                    }
                }
            }.frame(maxHeight: min(300, CGFloat(options.count * 35)))
        }.padding(9)
    }
}
struct SelectRow: View {
    let title: String
    @Binding var selection: String
    let options: [(String,String)]
    var body: some View {
        HStack(spacing: 16) {
            Text(LocalizedStringKey(title)).font(.system(size: 12)).foregroundStyle(.secondary).frame(width: 100, alignment: .leading)
            ChoiceControl(title: title, selection: $selection, options: options)
        }
    }
}
struct SwitchRow: View {
    let title: String
    @Binding var value: Bool
    var body: some View {
        Button { value.toggle() } label: {
            HStack {
                Text(LocalizedStringKey(title)).font(.system(size: 12))
                Spacer()
                Capsule().fill(value ? ShengStyle.button : ShengStyle.border).frame(width: 31, height: 18)
                    .overlay(Circle().fill(.white).frame(width: 13, height: 13).offset(x: value ? 6.5 : -6.5))
            }.frame(minHeight: 31).contentShape(Rectangle())
        }.buttonStyle(.plain).accessibilityValue(value ? "开启" : "关闭").accessibilityAddTraits(value ? .isSelected : []).animation(.easeInOut(duration: 0.15), value: value)
    }
}

/// Shared presentation for truthful empty states; never inserts sample session data.
struct EmptyState: View {
    let icon: String
    let title: String
    let detail: String
    var actionTitle: String? = nil
    var action: (() -> Void)? = nil
    var body: some View {
        VStack(spacing: 12) {
            Image(systemName: icon).font(.system(size: 26, weight: .light))
                .foregroundStyle(ShengStyle.accent).frame(width: 64, height: 64)
                .background(ShengStyle.accent.opacity(0.08), in: RoundedRectangle(cornerRadius: 20))
            Text(LocalizedStringKey(title)).font(.system(size: 19, weight: .medium)).padding(.top, 4)
            Text(LocalizedStringKey(detail)).font(.system(size: 12)).foregroundStyle(.secondary)
                .multilineTextAlignment(.center).lineSpacing(4).frame(maxWidth: 310)
            if let actionTitle, let action { Button(LocalizedStringKey(actionTitle), action: action).buttonStyle(QuietButtonStyle(prominent: true)).padding(.top, 8) }
        }.padding(32).frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}
