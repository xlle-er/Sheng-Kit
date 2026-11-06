import SwiftUI
import AppKit

/// Observe a real wheel gesture within this view without intercepting scrolling.
struct ScrollIntentObserver: NSViewRepresentable {
    let onScroll: () -> Void
    func makeNSView(context: Context) -> WheelView { let view = WheelView(); view.onScroll = onScroll; return view }
    func updateNSView(_ view: WheelView, context: Context) { view.onScroll = onScroll }
    static func dismantleNSView(_ view: WheelView, coordinator: ()) { view.removeMonitor() }
    final class WheelView: NSView {
        var onScroll: (() -> Void)?
        private var monitor: Any?
        override func hitTest(_ point: NSPoint) -> NSView? { nil }
        override func viewDidMoveToWindow() {
            removeMonitor()
            guard window != nil else { return }
            monitor = NSEvent.addLocalMonitorForEvents(matching: .scrollWheel) { [weak self] event in
                guard let self, event.window === self.window, self.bounds.contains(self.convert(event.locationInWindow, from: nil)), abs(event.scrollingDeltaY) > 0 else { return event }
                self.onScroll?(); return event
            }
        }
        func removeMonitor() { if let monitor { NSEvent.removeMonitor(monitor) }; monitor = nil }
        deinit { removeMonitor() }
    }
}
