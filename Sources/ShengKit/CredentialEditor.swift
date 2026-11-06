import SwiftUI

/// One secret field: the value sits on the title's line and fills the row, the actions line up on the right below.
/// Save and Check are always shown and only enabled once there is something to save or check.
struct CredentialEditor: View {
    let title: String
    @Binding var value: String?
    let present: Bool
    let reveal: () throws -> String
    var onSave: (() -> Void)? = nil
    var onCheck: (() -> Void)? = nil
    @State private var visible = false
    @State private var failed = false

    private var typed: Bool { !(value ?? "").isEmpty }

    var body: some View {
        VStack(spacing: 0) {
            Divider().opacity(0.6)
            VStack(alignment: .leading, spacing: 8) {
                HStack(spacing: 12) {
                    Text(LocalizedStringKey(title)).font(.system(size: 12.5)).frame(minWidth: 72, alignment: .leading)
                    field.frame(maxWidth: .infinity, alignment: .leading)
                }
                if failed { Text("凭据读取失败，请重试。").font(.system(size: 11)).foregroundStyle(.orange) }
                // One titled row when it fits, then two titled rows (editing above, save/check below), then icons.
                ViewThatFits(in: .horizontal) {
                    HStack(spacing: 6) { Spacer(minLength: 0); primary; secondary }.labelStyle(.titleAndIcon)
                    VStack(alignment: .trailing, spacing: 6) {
                        HStack(spacing: 6) { Spacer(minLength: 0); secondary }
                        HStack(spacing: 6) { Spacer(minLength: 0); primary }
                    }.labelStyle(.titleAndIcon)
                    HStack(spacing: 6) { Spacer(minLength: 0); primary; secondary }.labelStyle(.iconOnly)
                }
            }
            .padding(.vertical, 8)
        }
    }

    @ViewBuilder private var field: some View {
        if value != nil {
            Group { if visible { TextField("", text: binding) } else { SecureField("", text: binding) } }
                .textFieldStyle(.plain).font(.system(size: 12.5, design: .monospaced))
                .padding(.horizontal, 9).frame(height: 28)
                .background(ShengStyle.field, in: RoundedRectangle(cornerRadius: 6))
                .overlay(RoundedRectangle(cornerRadius: 6).strokeBorder(ShengStyle.border.opacity(0.6)))
                .accessibilityLabel(Text(LocalizedStringKey(title)))
        } else {
            HStack(spacing: 8) {
                if present { Text("••••••••••••").font(.system(size: 12.5, design: .monospaced)).foregroundStyle(.secondary) }
                StatusChip(text: present ? "已保存" : "未设置", tone: present ? .ok : .warning)
            }
            .padding(.horizontal, 9).frame(maxWidth: .infinity, minHeight: 28, alignment: .leading)
            .background(ShengStyle.field.opacity(0.5), in: RoundedRectangle(cornerRadius: 6))
        }
    }

    @ViewBuilder private var primary: some View {
        if let onSave { ActionButton("保存", icon: "square.and.arrow.down") { onSave() }.disabled(!typed) }
        if let onCheck { ActionButton("检查", icon: "checkmark.shield") { onCheck() }.disabled(!typed && !present) }
    }

    @ViewBuilder private var secondary: some View {
        ActionButton(present || value != nil ? "替换" : "填写", icon: "pencil") { value = ""; visible = false }
        ActionButton(visible ? "隐藏" : "查看", icon: visible ? "eye.slash" : "eye") {
            do { if value == nil { value = try reveal() }; visible.toggle() } catch { failed = true }
        }.disabled(!present && !typed)
        ActionButton("粘贴", icon: "doc.on.clipboard") { value = NSPasteboard.general.string(forType: .string) ?? ""; visible = false }
        ActionButton("清除", icon: "xmark.circle") { value = ""; visible = false }.disabled(!present && value == nil)
    }

    private var binding: Binding<String> { Binding(get: { value ?? "" }, set: { value = $0 }) }
}

extension ServiceCredentials {
    func value(_ key: String) -> String { switch key { case "apiKey": return apiKey; case "secretID": return secretID; case "secretKey": return secretKey; default: return textKey } }
    mutating func set(_ key: String, _ value: String) { switch key { case "apiKey": apiKey = value; case "secretID": secretID = value; case "secretKey": secretKey = value; default: textKey = value } }
}

/// Links to the provider's own console and setup guide; only opened when clicked.
struct ProviderHelp: View {
    let provider: String
    var textProvider = ""
    var textEndpoint = ""
    private var links: [ProviderLink] {
        let catalog = ProviderCatalog.bundled
        return catalog.services.filter { $0.id == provider || $0.id == textProvider } + catalog.presets.filter { $0.endpoint == textEndpoint && !textEndpoint.isEmpty }
    }
    var body: some View {
        Menu {
            ForEach(links) { item in
                Section(item.label) {
                    if let url = URL(string: item.apiKeyUrl), url.scheme == "https" { Link("获取 API Key", destination: url) }
                    if let url = URL(string: item.guideUrl), url.scheme == "https" { Link("配置指引", destination: url) }
                }
            }
        } label: { Label("帮助", systemImage: "questionmark.circle") }
            .menuStyle(.borderlessButton).fixedSize().disabled(links.isEmpty)
    }
}
