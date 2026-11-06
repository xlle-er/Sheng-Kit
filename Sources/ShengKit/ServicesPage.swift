import SwiftUI

/// One card in a profile list column.
struct ProfileCard: Identifiable {
    let id: UUID
    let name: String
    let subtitle: String
    let inUse: Bool
    let draft: Bool
    let warning: String?
}

/// Left column shared by the speech and translation pages: cards, an add button and a hint.
struct ProfileListColumn: View {
    let title: String
    let cards: [ProfileCard]
    let selected: UUID
    let add: () -> Void
    let select: (UUID) -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Text(LocalizedStringKey(title)).font(.system(size: 15, weight: .semibold)).lineLimit(1)
                Spacer(minLength: 4)
                ActionButton("添加", icon: "plus") { add() }.labelStyle(.iconOnly)
            }.padding(.horizontal, 4).padding(.bottom, 6)
            ScrollView { VStack(spacing: 8) { ForEach(cards) { card($0) } } }
            Text("点击卡片编辑；“设为当前”后，字幕页会使用这个配置。")
                .font(.system(size: 11)).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
        }
        .padding(14)
        .frame(width: 230)
        .background(ShengStyle.sidebar.opacity(0.5))
    }

    private func card(_ item: ProfileCard) -> some View {
        let isSelected = item.id == selected
        return Button { if !isSelected { select(item.id) } } label: {
            VStack(alignment: .leading, spacing: 4) {
                HStack(spacing: 6) {
                    Text(item.name).font(.system(size: 13, weight: .semibold)).lineLimit(1)
                    Spacer(minLength: 4)
                    if item.draft { StatusChip(text: "未保存", tone: .neutral) } else if item.inUse { StatusChip(text: "使用中", tone: .accent) }
                }
                Text(LocalizedStringKey(item.subtitle)).font(.system(size: 11.5)).foregroundStyle(.secondary).lineLimit(1)
                if let warning = item.warning { StatusChip(text: warning, tone: .warning) }
            }
            .padding(10)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(ShengStyle.surface, in: RoundedRectangle(cornerRadius: 10))
            .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(isSelected ? ShengStyle.accent : ShengStyle.border.opacity(0.6), lineWidth: isSelected ? 1.5 : 1))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityAddTraits(isSelected ? .isSelected : [])
    }
}

/// Speech recognition profiles.
struct ServicesPage: View {
    @ObservedObject var model: SessionModel
    @Binding var editing: ServiceProfile

    var body: some View {
        HStack(spacing: 0) {
            ProfileListColumn(title: "语音识别", cards: cards, selected: editing.id,
                              add: { editing = ServiceProfile(); editing.name = L10n.text("新配置") },
                              select: { id in if let profile = model.profiles.first(where: { $0.id == id }) { editing = profile } })
            Divider()
            ScrollView { ServiceEditor(model: model, profile: $editing).padding(.horizontal, 24).padding(.vertical, 22) }
        }
        .background(ShengStyle.canvas)
    }

    private var cards: [ProfileCard] {
        var list = model.profiles.map { card($0, draft: false) }
        if !model.profiles.contains(where: { $0.id == editing.id }) { list.append(card(editing, draft: true)) }
        return list
    }
    /// Only missing credentials are flagged here; language support depends on the subtitle page's choice.
    private func card(_ profile: ServiceProfile, draft: Bool) -> ProfileCard {
        let credentials = (try? model.store.credentials(for: profile.id)) ?? ServiceCredentials()
        let missing = profile.lacksCredential(credentials)
        return ProfileCard(id: profile.id, name: profile.name, subtitle: profile.providerName, inUse: profile.id == model.selectedProfile, draft: draft,
                           warning: !draft && missing ? ServiceReadiness.missingCredential.title : nil)
    }
}

/// Text translation profiles.
struct TranslationPage: View {
    @ObservedObject var model: SessionModel
    @State private var editing = TranslationProfile()
    @State private var loaded = false

    var body: some View {
        HStack(spacing: 0) {
            ProfileListColumn(title: "文字翻译", cards: cards, selected: editing.id,
                              add: { editing = TranslationProfile(); editing.name = L10n.text("新配置"); editing.kind = "openAICompatible" },
                              select: { id in if let profile = model.translationProfiles.first(where: { $0.id == id }) { editing = profile } })
            Divider()
            ScrollView { TranslationEditor(model: model, profile: $editing).padding(.horizontal, 24).padding(.vertical, 22) }
        }
        .background(ShengStyle.canvas)
        .onAppear {
            guard !loaded else { return }
            loaded = true
            editing = model.selectedTranslationProfile ?? model.translationProfiles.first ?? TranslationProfile()
        }
    }

    private var cards: [ProfileCard] {
        var list = model.translationProfiles.map { card($0, draft: false) }
        if !model.translationProfiles.contains(where: { $0.id == editing.id }) { list.append(card(editing, draft: true)) }
        return list
    }
    private func card(_ profile: TranslationProfile, draft: Bool) -> ProfileCard {
        let missing = profile.needsKey && ((try? model.store.credentials(for: profile.id).apiKey) ?? "").isEmpty
        return ProfileCard(id: profile.id, name: profile.name, subtitle: profile.kindName, inUse: profile.id.uuidString == model.selectedTranslation, draft: draft,
                           warning: !draft && missing ? ServiceReadiness.missingCredential.title : nil)
    }
}
