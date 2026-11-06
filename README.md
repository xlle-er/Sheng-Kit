# Sheng Kit

用 Rust 构建的实时字幕 SDK：把音频流转成实时字幕与译文，统一接入本地与云端的识别、翻译服务，并通过稳定的 C ABI 提供给原生应用；附带一套 SwiftUI／AppKit 的 Mac 界面与采集组件。

A real-time captioning SDK built in Rust. It turns audio streams into live captions and translations across on-device and cloud speech services, exposes them through a stable C ABI, and ships SwiftUI/AppKit components for macOS capture and UI.

## 功能

- **音频输入**：系统声音（macOS 14.2+ 使用 Core Audio Process Tap，macOS 13 使用 ScreenCaptureKit）、指定应用、指定麦克风、双音源；双音源时用 SpeexDSP 消除麦克风里的外放回声。
- **语音识别**：Apple 本地识别，以及阿里云、火山引擎、腾讯云、百度、OpenAI Realtime、Azure OpenAI、Google Gemini Live、xAI、自定义 DashScope／OpenAI 兼容识别。
- **文字翻译**：Apple 本地翻译、OpenAI 兼容服务、DeepL、DeepLX、xAI、Qwen-MT；也可使用识别服务自带的同声传译。
- **字幕**：原文草稿实时显示、原译文配对、有界的阅读缓冲；连接意外中断时保留说到一半的句子，并按退避重连。
- **ShengKit 界面**：SwiftUI／AppKit 的主窗口、字幕浮窗、服务配置、会话记录（默认不保存）、诊断与八种界面语言。
- **C ABI v1**：不透明句柄、复制传入的 PCM、有界快照与确认字幕队列、异步资源操作，见 [`sheng_engine.h`](Sources/CShengEngine/include/sheng_engine.h)。

凭据保存在本机私有文件中，不进入日志或诊断信息；音频与字幕只在用户主动开启时采集和保存。

## 结构

| 目录 | 内容 |
|---|---|
| `rust/sheng-core` | 字幕状态机：草稿／定稿／预览、配对、排序、退避。无 I/O |
| `rust/sheng-runtime` | 请求校验、服务与语言目录、音频通道与回声处理、网络传输、翻译编排、各服务客户端；`apple/` 为编入静态库的 Apple Speech／Translation 桥接 |
| `rust/sheng-engine` | C ABI，产出 `libsheng_engine.a` |
| `Sources/ShengKit` | SwiftUI／AppKit 界面、采集、私有存储 |
| `rust/testdata` | 测试用的参考数据（语言目录、ABI 与请求校验） |

## 构建

需要 macOS 与 Xcode 26 或更新版本（编译 Apple 桥接）、Rust stable（见 `rust-toolchain.toml`）、Swift 5.9+。部署目标 macOS 13；Apple 本地识别和翻译需要 Apple 芯片与 macOS 26+。

```bash
cargo build --locked -p sheng-engine
./script/check.sh   # fmt、clippy、Rust 与 Swift 测试、cargo deny、第三方声明
```

Swift 包链接 `target/debug` 下的静态库；发布构建设置 `SHENG_ENGINE_PROFILE=release` 并先构建 Rust release。需先构建 Rust 静态库，再构建 Swift 包。

## 许可

Apache License 2.0，见 [`LICENSE`](LICENSE) 与 [`NOTICE`](NOTICE)（Copyright 2026 xlle-er）。第三方组件声明见 [`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md)。
