import XCTest
import Carbon
import SwiftUI
@testable import ShengKit

final class BugFixTests: XCTestCase {
    func testShortcutHintFollowsConfiguredBindingInMacModifierOrder() {
        XCTAssertEqual(ShortcutBinding(keyCode: 1, modifiers: UInt32(optionKey | shiftKey)).symbols, ["⌥", "⇧", "S"])
        XCTAssertEqual(ShortcutBinding(keyCode: 11, modifiers: UInt32(cmdKey | controlKey)).symbols, ["⌃", "⌘", "B"])
        XCTAssertEqual(ShortcutBinding(keyCode: 49, modifiers: UInt32(optionKey)).symbols, ["⌥", "Space"])
    }

    func testCustomSubtitleColorStartsReadableOnCurrentBackground() {
        XCTAssertEqual(Color.customSubtitleStartHex(immersive: false, dark: false), "1D1D1F")
        XCTAssertEqual(Color.customSubtitleStartHex(immersive: false, dark: true), "FFFFFF")
        XCTAssertEqual(Color.customSubtitleStartHex(immersive: true, dark: false), "FFFFFF")
    }

    @MainActor func testOverlayFailureOpensServiceSettings() {
        let controller = OverlayController()
        var opened: [String] = []
        controller.openConfiguration = { opened.append($0) }
        controller.openServiceSettings(); controller.openAppearanceSettings()
        XCTAssertEqual(opened, ["services", "appearance"])
    }
}
