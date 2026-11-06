import SwiftUI

/// The software update rows in General: automatic checks, the current state and the install action.
struct UpdateSettings: View {
    @ObservedObject var updater: AppUpdater
    /// Installing quits the app, so a running session is called out first.
    let sessionActive: Bool
    @State private var confirming = false

    var body: some View {
        if updater.configuration == nil {
            SettingRow("软件更新", detail: "当前版本未配置更新渠道") { EmptyView() }
        } else {
            SwitchSetting(title: "自动检查更新", detail: "每天最多一次，只读取 GitHub 上公开的版本信息；下载和安装需要你确认", isOn: $updater.automaticChecks)
            SettingRow("软件更新", detail: detail) { action }
            if let release = updater.available, !release.notes.isEmpty {
                Text(release.notes).font(.system(size: 12)).foregroundStyle(.secondary).textSelection(.enabled)
                    .frame(maxWidth: .infinity, alignment: .leading).padding(.bottom, 10)
            }
        }
    }

    private var detail: String? {
        switch updater.state {
        case .checking: return L10n.text("正在检查…")
        case .upToDate: return L10n.text("已是最新版本")
        case .available(let release): return L10n.text("发现新版本") + " " + release.version.description
        case .downloading: return L10n.text("正在下载并校验…")
        case .installing: return L10n.text("正在安装，完成后会自动重新打开")
        case .failed(let message): return message
        case .idle: return updater.lastCheck.map { L10n.text("上次检查") + " " + $0.formatted(date: .abbreviated, time: .shortened) }
        }
    }

    @ViewBuilder private var action: some View {
        if let release = updater.available {
            AdaptiveActions {
                if let page = release.page { ActionButton("发布说明", icon: "doc.text") { NSWorkspace.shared.open(page) } }
                ActionButton("下载并安装", icon: "arrow.down.circle") { if sessionActive { confirming = true } else { updater.install() } }
            }
            .confirmationDialog("安装更新会停止当前字幕并重新打开应用。", isPresented: $confirming) {
                Button("停止并安装") { updater.install() }
            }
        } else if updater.isBusy {
            ProgressView().controlSize(.small)
        } else {
            ActionButton("检查更新", icon: "arrow.clockwise") { updater.check() }
        }
    }
}

/// A small sidebar notice when a newer release is waiting.
struct UpdateBadge: View {
    @ObservedObject var updater: AppUpdater
    let open: () -> Void
    var body: some View {
        if let release = updater.available {
            Button(action: open) {
                Label(L10n.text("新版本") + " " + release.version.description, systemImage: "arrow.down.circle.fill")
                    .font(.system(size: 12, weight: .medium)).foregroundStyle(ShengStyle.accent)
                    .padding(.horizontal, 10).padding(.vertical, 7)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(ShengStyle.accent.opacity(0.10), in: RoundedRectangle(cornerRadius: 8))
            }
            .buttonStyle(.plain).padding(.bottom, 6)
        }
    }
}
