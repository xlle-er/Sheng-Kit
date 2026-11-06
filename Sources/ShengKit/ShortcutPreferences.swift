import SwiftUI
import Carbon

public struct ShortcutBinding: Codable, Equatable {
    public var keyCode: UInt32
    public var modifiers: UInt32
    public init(keyCode: UInt32, modifiers: UInt32) { self.keyCode = keyCode; self.modifiers = modifiers }
    static let keys: [(String, UInt32)] = [("A",0),("B",11),("C",8),("D",2),("E",14),("F",3),("G",5),("H",4),("I",34),("J",38),("K",40),("L",37),("M",46),("N",45),("O",31),("P",35),("Q",12),("R",15),("S",1),("T",17),("U",32),("V",9),("W",13),("X",7),("Y",16),("Z",6),("Space",49)]
    /// A recorded chord; global shortcuts need ⌘, ⌥ or ⌃ so plain typing is never captured.
    public init?(keyCode: UInt16, flags: NSEvent.ModifierFlags) {
        guard Self.keys.contains(where: { $0.1 == UInt32(keyCode) }), !flags.isDisjoint(with: [.command, .option, .control]) else { return nil }
        let map: [(NSEvent.ModifierFlags, Int)] = [(.command, cmdKey), (.option, optionKey), (.shift, shiftKey), (.control, controlKey)]
        self.init(keyCode: UInt32(keyCode), modifiers: map.reduce(UInt32(0)) { flags.contains($1.0) ? $0 | UInt32($1.1) : $0 })
    }
    /// Modifier glyphs in the macOS menu order (⌃⌥⇧⌘), followed by the key name.
    public var symbols: [String] {
        let glyphs: [(String, Int)] = [("⌃", controlKey), ("⌥", optionKey), ("⇧", shiftKey), ("⌘", cmdKey)]
        return glyphs.filter { modifiers & UInt32($0.1) != 0 }.map(\.0) + [Self.keys.first { $0.1 == keyCode }?.0 ?? "?"]
    }
}
public struct ShortcutPreferences: Codable, Equatable {
    public var session: ShortcutBinding? = ShortcutBinding(keyCode: UInt32(kVK_ANSI_S), modifiers: UInt32(optionKey | shiftKey))
    public var display: ShortcutBinding? = nil
    public var immersive: ShortcutBinding? = nil
    public var overlay: ShortcutBinding? = nil
    public init() {}
    var actions: [(String, ShortcutBinding?)] { [("session",session),("display",display),("immersive",immersive),("overlay",overlay)] }
}
@MainActor final class ShortcutRegistry {
    private var values: [String: (ShortcutBinding, AnyObject)] = [:]
    private let register: (ShortcutBinding, @escaping () -> Void) throws -> AnyObject
    init(register: @escaping (ShortcutBinding, @escaping () -> Void) throws -> AnyObject = { try GlobalShortcut(binding: $0, action: $1) }) { self.register = register }
    var active: [String: ShortcutBinding] { values.mapValues { $0.0 } }
    // Register every changed chord before releasing old registrations.
    func replace(_ preferences: ShortcutPreferences, action: @escaping (String) -> Void) throws {
        let chords = preferences.actions.compactMap(\.1)
        guard Set(chords.map { "\($0.keyCode):\($0.modifiers)" }).count == chords.count else { throw EngineError(code: "shortcut_in_use") }
        var next: [String: (ShortcutBinding, AnyObject)] = [:]
        for (name, binding) in preferences.actions {
            guard let binding else { continue }
            if let existing = values[name], existing.0 == binding { next[name] = existing }
            else { next[name] = (binding, try register(binding) { action(name) }) }
        }
        values = next
    }
}
extension SessionModel {
    public func applyShortcuts(_ preferences: ShortcutPreferences) throws {
        for (_, item) in preferences.actions { if let item { guard item.modifiers & UInt32(cmdKey | optionKey | controlKey) != 0 else { throw EngineError(code: "shortcut_unavailable") } } }
        try onShortcutsChange?(preferences)
        let previous = shortcuts
        shortcuts = preferences
        do { try store.write(JSONEncoder().encode(preferences), name: "shortcuts-v1.json") }
        catch { shortcuts = previous; try? onShortcutsChange?(previous); throw error }
    }
    public func completeGuide() { guideCompleted = true; do { try store.write(Data("true".utf8), name: "guide-v1.json") } catch { self.error = L10n.text("偏好设置保存失败，请重试。") } }
    public func performAction(_ action: String) {
        guard !isBusy, !isReconnecting else { return }
        switch action {
        case "display": cycleDisplayMode()
        case "immersive": immersive.toggle(); savePreferences()
        case "overlay": onToggleOverlay?()
        default: Task { if isCapturing { await stop(pause: true) } else { await start() } }
        }
    }
}
