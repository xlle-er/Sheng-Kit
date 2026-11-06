// swift-tools-version: 5.9
import PackageDescription
import Foundation
let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent().path
let engineProfile = ProcessInfo.processInfo.environment["SHENG_ENGINE_PROFILE"] ?? "debug"
let engineLibrary = ProcessInfo.processInfo.environment["SHENG_ENGINE_LIBRARY_DIR"] ?? root + "/target/" + engineProfile
let package = Package(
    name: "SharedKit", platforms: [.macOS(.v13)],
    products: [.library(name: "ShengKit", targets: ["ShengKit"])],
    targets: [
        .systemLibrary(name: "CShengEngine"),
        .target(name: "ShengKit", dependencies: ["CShengEngine"], linkerSettings: [
            .unsafeFlags(["-L", engineLibrary]), .linkedFramework("SystemConfiguration"), .linkedFramework("Security"),
            // Apple bridge (step 4a). Translation exists from macOS 15 and is loaded weakly.
            .linkedFramework("AVFoundation"), .linkedFramework("Speech"), .linkedFramework("AppKit"),
            .linkedFramework("SwiftUI"), .unsafeFlags(["-Xlinker", "-weak_framework", "-Xlinker", "Translation"])
        ]),
        .testTarget(name: "ShengKitTests", dependencies: ["ShengKit"])
    ]
)
