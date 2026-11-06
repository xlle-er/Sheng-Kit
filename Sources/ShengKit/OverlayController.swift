import SwiftUI
import AppKit
import Combine

@MainActor
public final class OverlayController: NSObject, ObservableObject, NSWindowDelegate {
    @Published public private(set) var visible = false
    /// Receives the workspace page to reveal.
    public var openConfiguration: ((String) -> Void)?
    func openServiceSettings() { openConfiguration?("services") }
    func openAppearanceSettings() { openConfiguration?("appearance") }
    private var panel: NSPanel?
    private weak var model: SessionModel?
    private var observation: AnyCancellable?
    private var expandedFrame: NSRect?
    private var resizingFrame: NSRect?
    private var wasCollapsed = false
    public override init() { super.init() }
    public func toggle(model: SessionModel) { if visible { hide() } else { show(model: model) } }
    public func hide() { panel?.orderOut(nil); visible = false }
    public func show(model: SessionModel) {
        self.model = model
        if panel == nil {
            let window = SubtitlePanel(contentRect: Self.defaultFrame(), styleMask: [.borderless, .nonactivatingPanel, .resizable], backing: .buffered, defer: false)
            window.title = L10n.named("%@字幕"); window.identifier = NSUserInterfaceItemIdentifier("ShengJianSubtitle")
            window.hidesOnDeactivate = false
            window.level = .floating
            window.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]
            window.isMovableByWindowBackground = true
            window.isOpaque = false; window.backgroundColor = .clear; window.hasShadow = true
            window.minSize = Self.expandedMinimum
            window.setFrameUsingName("ShengJianSubtitle")
            if !NSScreen.screens.contains(where: { $0.visibleFrame.intersects(window.frame) }) {
                window.setFrame(Self.defaultFrame(), display: false)
            }
            window.delegate = self
            let hosting = NSHostingView(rootView: OverlaySurface(model: model, controller: self))
            hosting.sizingOptions = []
            hosting.autoresizingMask = [.width, .height]
            window.contentView = hosting
            // objectWillChange fires before the value lands and on every caption revision; react only to window-affecting state.
            observation = model.objectWillChange.receive(on: RunLoop.main)
                .compactMap { [weak model] _ in model.map(OverlayWindowState.init) }
                .removeDuplicates()
                .sink { [weak self] _ in self?.configure() }
            panel = window
        }
        configure()
        if let panel, !model.overlayCollapsed {
            var frame = panel.frame
            if frame.height < Self.expandedMinimum.height { frame.origin.y = frame.maxY - Self.expandedMinimum.height; frame.size.height = Self.expandedMinimum.height; panel.setFrame(frame, display: false) }
        }
        panel?.orderFrontRegardless(); visible = true
    }
    private func configure() {
        guard let model, let panel else { return }
        panel.ignoresMouseEvents = model.clickThrough
        panel.isMovableByWindowBackground = !model.overlayLocked
        panel.hasShadow = !model.immersive || model.overlayCollapsed
        let scheme = model.overlayBackground == "system" ? model.theme : model.overlayBackground
        panel.appearance = scheme == "dark" ? NSAppearance(named: .darkAqua) : scheme == "light" ? NSAppearance(named: .aqua) : nil
        if model.overlayLocked || model.overlayCollapsed { panel.styleMask.remove(.resizable) } else { panel.styleMask.insert(.resizable) }
        if wasCollapsed != model.overlayCollapsed {
            if model.overlayCollapsed {
                expandedFrame = panel.frame
                panel.minSize = NSSize(width: 230, height: 54)
                let width = compactWidth(model.statusTitle)
                panel.setFrame(NSRect(x: panel.frame.midX - width / 2, y: panel.frame.maxY - 54, width: width, height: 54), display: true)
            } else {
                let minimum = Self.expandedMinimum
                panel.minSize = minimum
                if let expandedFrame { panel.setFrame(NSRect(x: panel.frame.midX - expandedFrame.width / 2, y: panel.frame.maxY - max(minimum.height, expandedFrame.height), width: max(minimum.width, expandedFrame.width), height: max(minimum.height, expandedFrame.height)), display: true) }
            }
            wasCollapsed = model.overlayCollapsed
        } else if model.overlayCollapsed {
            let width = compactWidth(model.statusTitle)
            if abs(panel.frame.width - width) > 1 {
                panel.setFrame(NSRect(x: panel.frame.midX - width / 2, y: panel.frame.minY, width: width, height: 54), display: true)
            }
        }
    }
    private func compactWidth(_ status: String) -> CGFloat {
        let text = NSAttributedString(string: status, attributes: [.font: NSFont.systemFont(ofSize: 11, weight: .medium)]).size().width
        return max(230, ceil(text + 210))
    }
    func resize(by translation: CGSize, ended: Bool) {
        guard let panel, model?.overlayLocked == false, model?.overlayCollapsed == false else { return }
        let origin = resizingFrame ?? panel.frame
        resizingFrame = origin
        let screen = panel.screen?.visibleFrame ?? origin
        let width = min(screen.width, max(Self.expandedMinimum.width, origin.width + translation.width))
        let height = min(screen.height, max(Self.expandedMinimum.height, origin.height + translation.height))
        panel.setFrame(NSRect(x: origin.minX, y: origin.maxY - height, width: width, height: height), display: true)
        if ended { resizingFrame = nil; saveFrame() }
    }
    var screenHeight: CGFloat? { (panel?.screen ?? NSScreen.main)?.visibleFrame.height }
    /// Small enough for a one-line subtitle strip.
    static let expandedMinimum = NSSize(width: 360, height: 120)
    /// Bottom-centre of the screen under the pointer, where the user is looking.
    static func defaultFrame() -> NSRect {
        let mouse = NSEvent.mouseLocation
        let screen = (NSScreen.screens.first { $0.frame.contains(mouse) } ?? NSScreen.main)?.visibleFrame ?? NSRect(x: 0, y: 0, width: 1200, height: 800)
        return NSRect(x: screen.midX - 380, y: screen.minY + 70, width: 760, height: 220)
    }
    public func resetPosition() {
        model?.overlayCollapsed = false
        guard let panel else { return }
        panel.setFrame(Self.defaultFrame(), display: true)
        saveFrame()
    }
    public func windowDidMove(_ notification: Notification) { saveFrame() }
    public func windowDidResize(_ notification: Notification) { saveFrame() }
    private func saveFrame() {
        guard model?.overlayCollapsed != true else { return }
        panel?.saveFrame(usingName: "ShengJianSubtitle")
    }
}

private struct OverlayWindowState: Equatable {
    let clickThrough: Bool, locked: Bool, immersive: Bool, collapsed: Bool, status: String, background: String, theme: String
    @MainActor init(_ model: SessionModel) {
        clickThrough = model.clickThrough; locked = model.overlayLocked; immersive = model.immersive
        collapsed = model.overlayCollapsed; status = model.statusTitle
        background = model.overlayBackground; theme = model.theme
    }
}

private final class SubtitlePanel: NSPanel {
    override var canBecomeKey: Bool { true }
    override var canBecomeMain: Bool { false }
}
