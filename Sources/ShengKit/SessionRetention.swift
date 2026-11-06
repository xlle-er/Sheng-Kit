import Foundation

extension SessionModel {
    var sessionsRoot: URL { store.root.appendingPathComponent("Sessions") }

    /// Applies the opt-in retention period once per launch; the session being written is always kept.
    func applySessionRetention() {
        let days = sessionRetentionDays, root = sessionsRoot, active = activeSessionID
        guard days > 0 else { return }
        Task {
            do { _ = try await SessionLibrary.shared.prune(root: root, olderThanDays: days, keeping: active) }
            catch { self.error = L10n.text("旧会话清理失败，已保留全部记录。") }
        }
    }
}
