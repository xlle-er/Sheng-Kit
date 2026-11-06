import AppKit

@MainActor
public final class AppLifecycle: NSObject, NSApplicationDelegate {
    public weak var model: SessionModel? { didSet { registerShortcut(); if let model { NSApp.setActivationPolicy(model.showInDock ? .regular : .accessory) } } }
    private let shortcuts = ShortcutRegistry()
    private var terminationSignal: DispatchSourceSignal?
    private var preparingTermination = false
    private var readyToTerminate = false
    private func registerShortcut() {
        model?.onShortcutsChange = { [weak self] preferences in
            guard let self else { return }
            try self.shortcuts.replace(preferences) { [weak self] action in self?.model?.performAction(action) }
        }
        do { if let model { try model.onShortcutsChange?(model.shortcuts) } }
        catch { model?.error = L10n.text("全局快捷键注册失败，仍可使用窗口和菜单操作。") }
    }
    public override init() { super.init() }
    public func applicationDidFinishLaunching(_ notification: Notification) {
        signal(SIGTERM, SIG_IGN)
        let source = DispatchSource.makeSignalSource(signal: SIGTERM, queue: .main)
        source.setEventHandler { NSApp.terminate(nil) }
        source.resume(); terminationSignal = source
        NSApp.setActivationPolicy(model?.showInDock == false ? .accessory : .regular)
        NSApp.activate(ignoringOtherApps: true)
    }
    public func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        if readyToTerminate { return .terminateNow }
        guard let model else { return .terminateNow }
        if !preparingTermination {
            preparingTermination = true
            Task { @MainActor in
                await model.stop()
                model.flushPreferences()
                readyToTerminate = true
                sender.terminate(nil)
            }
        }
        // terminateLater spins an AppKit loop. When quit originated in a main
        // dispatch callback (including SIGTERM), that loop cannot run the
        // MainActor cleanup queued behind it. Return first, then quit once ready.
        return .terminateCancel
    }
}
