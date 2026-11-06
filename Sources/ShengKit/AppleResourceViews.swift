import SwiftUI

/// Apple speech models: every language the Mac can recognize, each downloadable on its own.
struct SpeechResourcesView: View {
    @ObservedObject var model: SessionModel
    @State private var report: AppleResourceReport?
    @State private var search = ""
    @State private var working: String?
    @State private var message = ""
    @State private var task: Task<Void, Never>?

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            SettingRow("本机语音模型", detail: "点击语言下载；不会自动下载") {
                AdaptiveActions {
                    ActionButton("刷新", icon: "arrow.clockwise") { refresh() }.disabled(working != nil)
                }
            }
            if let report, !report.available {
                Text("这台 Mac 不支持 Apple 本地识别，请使用云服务。").font(.system(size: 12)).foregroundStyle(.secondary)
            } else if let report {
                TextField(LocalizedStringKey("搜索语言"), text: $search).textFieldStyle(.roundedBorder).frame(maxWidth: 260)
                LazyVGrid(columns: [GridItem(.adaptive(minimum: 132), spacing: 8)], alignment: .leading, spacing: 8) {
                    ForEach(languages(report)) { language in chip(language) }
                }
            } else {
                ProgressView().controlSize(.small)
            }
            if !message.isEmpty { Text(message).font(.system(size: 11)).foregroundStyle(.secondary) }
        }
        .padding(.bottom, 10)
        .onAppear { if report == nil { refresh() } }
        .onDisappear { task?.cancel() }
    }

    private struct Entry: Identifiable { let id: String; let name: String; let status: String }

    private func languages(_ report: AppleResourceReport) -> [Entry] {
        report.languages
            .map { Entry(id: $0.code, name: model.locale.localizedString(forIdentifier: $0.code.replacingOccurrences(of: "_", with: "-")) ?? $0.code, status: $0.status) }
            .filter { search.isEmpty || $0.name.localizedCaseInsensitiveContains(search) || $0.id.localizedCaseInsensitiveContains(search) }
            .sorted { ($0.status == "installed" ? 0 : 1, $0.name) < ($1.status == "installed" ? 0 : 1, $1.name) }
    }

    private func chip(_ language: Entry) -> some View {
        ResourceChip(name: language.name, status: language.status, busy: working == language.id || language.status == "downloading",
                     disabled: working != nil || model.active) { download(language.id) }
    }

    private func refresh() {
        task?.cancel()
        task = Task {
            do { report = try await AppleResources.inspect(source: model.sourceLanguage, target: "original") }
            catch { message = error.localizedDescription }
        }
    }

    private func download(_ code: String) {
        working = code; message = ""
        task = Task {
            do {
                _ = try await AppleResources.perform(kind: 1, source: code, target: "original")
                report = try await AppleResources.inspect(source: code, target: "original")
                model.appleResources = report
            } catch { message = error.localizedDescription }
            working = nil
        }
    }
}

/// One language in a resource grid: status icon and name; tapping downloads it when it is not installed.
struct ResourceChip: View {
    let name: String
    /// `nil` while the status is still being read.
    let status: String?
    let busy: Bool
    let disabled: Bool
    let action: () -> Void
    private var installed: Bool { status == "installed" }
    private var downloadable: Bool { status == "supported" || status == "supportedNotInstalled" || status == "notInstalled" }

    var body: some View {
        Button(action: action) {
            HStack(spacing: 6) {
                if busy || status == nil { ProgressView().controlSize(.mini) }
                else { Image(systemName: icon).foregroundStyle(tint) }
                Text(name).font(.system(size: 12)).lineLimit(1)
                Spacer(minLength: 0)
            }
            .padding(.horizontal, 9).frame(height: 28)
            .background(ShengStyle.field, in: RoundedRectangle(cornerRadius: 7))
            .opacity(installed || downloadable || busy || status == nil ? 1 : 0.5)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .disabled(!downloadable || busy || disabled)
        .help(Text(LocalizedStringKey(installed ? "已安装" : downloadable ? "下载" : "不支持")))
        .accessibilityLabel(Text(name))
        .accessibilityValue(Text(LocalizedStringKey(installed ? "已安装" : busy ? "正在下载" : downloadable ? "可下载" : "不支持")))
    }
    private var icon: String { installed ? "checkmark.circle.fill" : downloadable ? "arrow.down.circle" : "minus.circle" }
    private var tint: Color { installed ? .green : downloadable ? ShengStyle.accent : .secondary }
}

/// Apple translation models are per language pair: pick the source, then download any target from the grid.
struct TranslationResourcesView: View {
    @ObservedObject var model: SessionModel
    @State private var source = ""
    @State private var statuses: [String: String] = [:]
    /// Languages Apple maps to a local locale; only these are checked for translation support.
    @State private var candidates: [String] = []
    @State private var search = ""
    @State private var working: String?
    @State private var message = ""
    @State private var task: Task<Void, Never>?

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            SettingRow("从") {
                ChoiceControl(title: "源语言", selection: $source, options: languages.map { ($0, name($0)) }).frame(width: 180)
            }
            Text(LocalizedStringKey(checking ? "正在检查可用的翻译语言…" : "只列出 Apple 支持的语言；点击目标语言下载这一组翻译模型，不会自动下载"))
                .font(.system(size: 11)).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            TextField(LocalizedStringKey("搜索语言"), text: $search).textFieldStyle(.roundedBorder).frame(maxWidth: 260)
            LazyVGrid(columns: [GridItem(.adaptive(minimum: 132), spacing: 8)], alignment: .leading, spacing: 8) {
                ForEach(targets, id: \.self) { code in
                    ResourceChip(name: name(code), status: statuses[code], busy: working == code, disabled: working != nil || model.active) { download(code) }
                }
            }
            if !message.isEmpty { Text(message).font(.system(size: 11)).foregroundStyle(.secondary) }
        }
        .padding(.bottom, 10)
        .onAppear { if source.isEmpty { source = model.sourceLanguage == "auto" ? "en" : model.sourceLanguage }; readStatuses() }
        .onChange(of: source) { _ in readStatuses() }
        .onDisappear { task?.cancel() }
    }

    private var languages: [String] { candidates.isEmpty ? LanguageCatalog.targetCodes : candidates }
    private var checking: Bool { candidates.isEmpty || statuses.count < max(0, candidates.count - 1) }
    /// Supported pairs only: installed, downloadable or downloading; unsupported ones stay out of the grid.
    private var targets: [String] {
        candidates.filter { code in
            code != source && ["installed", "supported", "downloading"].contains(statuses[code] ?? "")
                && (search.isEmpty || name(code).localizedCaseInsensitiveContains(search) || code.localizedCaseInsensitiveContains(search))
        }
            .sorted { (statuses[$0] == "installed" ? 0 : 1, name($0)) < (statuses[$1] == "installed" ? 0 : 1, name($1)) }
    }
    private func name(_ code: String) -> String { model.locale.localizedString(forIdentifier: code.replacingOccurrences(of: "_", with: "-")) ?? code }

    /// Reads each pair's status one after another (the engine runs at most a few resource operations at once).
    private func readStatuses() {
        task?.cancel(); statuses = [:]; message = ""
        let from = source
        task = Task {
            if candidates.isEmpty {
                let report = try? await AppleResources.inspect(source: from, target: "original")
                candidates = report?.languages.map(\.code) ?? LanguageCatalog.targetCodes
            }
            for code in candidates where code != from {
                guard !Task.isCancelled else { return }
                let status = (try? await AppleResources.inspect(source: from, target: code).translation) ?? "unavailable"
                guard !Task.isCancelled, from == source else { return }
                statuses[code] = status
            }
        }
    }
    private func download(_ code: String) {
        working = code; message = ""
        let from = source
        Task {
            do {
                _ = try await AppleResources.perform(kind: 2, source: from, target: code)
                statuses[code] = try await AppleResources.inspect(source: from, target: code).translation
            } catch { message = error.localizedDescription }
            working = nil
        }
    }
}
