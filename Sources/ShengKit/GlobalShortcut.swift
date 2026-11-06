import Carbon
import Foundation

public final class GlobalShortcut {
    private var hotKey: EventHotKeyRef?
    private var handlerRef: EventHandlerRef?
    private static var nextID: UInt32 = 1
    private let identifier: UInt32
    private let action: () -> Void
    public init(binding: ShortcutBinding = ShortcutBinding(keyCode: UInt32(kVK_ANSI_S), modifiers: UInt32(optionKey | shiftKey)), action: @escaping () -> Void) throws {
        identifier = Self.nextID; Self.nextID += 1
        self.action = action
        var type = EventTypeSpec(eventClass: OSType(kEventClassKeyboard), eventKind: UInt32(kEventHotKeyPressed))
        let installed = InstallEventHandler(GetApplicationEventTarget(), { _, event, context in
            guard let context, let event else { return OSStatus(eventNotHandledErr) }
            var id = EventHotKeyID()
            let status = GetEventParameter(event, EventParamName(kEventParamDirectObject), EventParamType(typeEventHotKeyID), nil, MemoryLayout<EventHotKeyID>.size, nil, &id)
            guard status == noErr, id.signature == 0x53484E47, id.id == Unmanaged<GlobalShortcut>.fromOpaque(context).takeUnretainedValue().identifier else { return OSStatus(eventNotHandledErr) }
            Unmanaged<GlobalShortcut>.fromOpaque(context).takeUnretainedValue().action()
            return noErr
        }, 1, &type, Unmanaged.passUnretained(self).toOpaque(), &handlerRef)
        guard installed == noErr else { throw EngineError(code: "shortcut_unavailable") }
        let id = EventHotKeyID(signature: 0x53484E47, id: identifier)
        let status = RegisterEventHotKey(binding.keyCode, binding.modifiers, id, GetApplicationEventTarget(), 0, &hotKey)
        guard status == noErr else { if let handlerRef { RemoveEventHandler(handlerRef); self.handlerRef = nil }; throw EngineError(code: "shortcut_in_use") }
    }
    deinit { if let hotKey { UnregisterEventHotKey(hotKey) }; if let handlerRef { RemoveEventHandler(handlerRef) } }
}
