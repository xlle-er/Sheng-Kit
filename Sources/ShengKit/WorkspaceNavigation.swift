import SwiftUI

/// App-owned navigation keeps menu commands and the main configuration window in sync.
@MainActor
public final class WorkspaceNavigation: ObservableObject {
    @Published public var page = "live"
    public init() {}
    @discardableResult public func revealMainWindow() -> Bool {
        guard let window = NSApp.windows.first(where: { $0.identifier?.rawValue.hasPrefix("main-AppWindow") == true }) else { return false }
        window.deminiaturize(nil); window.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
        return true
    }
}
