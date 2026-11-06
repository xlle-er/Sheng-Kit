# Apple 桥 Swift 源目录（步骤 4a）

步骤 4a 在此放入 Swift 源文件（`*.swift`）。`rust/sheng-runtime/build.rs` 在 macOS 上用 `xcrun swiftc` 把本目录全部
`*.swift` 编译成静态库 `libsheng_apple_bridge.a`（模块名 `ShengAppleBridge`）并链接进 `sheng-runtime`；
本目录没有 `*.swift` 时 build.rs 什么都不做。

- 目标架构取自 Cargo 目标（`aarch64` → `arm64`，`x86_64`）；部署版本取 `MACOSX_DEPLOYMENT_TARGET`，未设置时为 `13.0`（与 `Package.swift` 一致）。
- Rust 侧通过 `extern "C"` 调用 Swift 中 `@_cdecl` 导出的函数；声明放在 `src/services/apple/` 下。
- Swift 运行库从 SDK 的 `usr/lib/swift` 和系统 `/usr/lib/swift` 链接。
