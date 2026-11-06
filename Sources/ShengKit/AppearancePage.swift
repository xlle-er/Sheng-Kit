import SwiftUI

struct AppearancePage: View {
    @ObservedObject var model: SessionModel
    @ObservedObject var overlay: OverlayController

    var body: some View {
        HStack(spacing: 0) {
            ScrollView {
                VStack(alignment: .leading, spacing: 16) {
                    PageHeader("字幕与浮窗", subtitle: "修改立即生效，右侧预览同步变化")
                    text
                    colors
                    behaviour
                    motion
                }
                .padding(.horizontal, 26).padding(.vertical, 22)
            }
            Divider()
            VStack(alignment: .leading, spacing: 8) {
                Text("实时预览").font(.system(size: 13, weight: .semibold))
                Text("文字为示例，不会写入会话").font(.system(size: 11.5)).foregroundStyle(.secondary)
                OverlaySample(model: model, scale: 0.6).frame(height: 330)
                Spacer()
            }
            .padding(20)
            .frame(width: 330)
            .background(ShengStyle.sidebar.opacity(0.5))
        }
        .background(ShengStyle.canvas)
    }

    private var text: some View {
        SettingsGroup("文字") {
            SettingRow("显示内容") { SegmentedChoice(title: "显示内容", selection: $model.displayMode, options: [("bilingual", "双语"), ("translation", "仅译文"), ("original", "仅原文")]) }
            SettingRow("字体") { ChoiceControl(title: "字体", selection: $model.fontName, options: FontCatalog.options).frame(width: 200) }
            SettingRow("字号") {
                HStack {
                    Slider(value: $model.fontSize, in: 14...64).frame(width: 180).accessibilityLabel("字号")
                    Text("\(Int(model.fontSize))").font(.system(size: 12, design: .monospaced)).foregroundStyle(.secondary).frame(width: 24, alignment: .trailing)
                }
            }
            SettingRow("对齐") { SegmentedChoice(title: "对齐方式", selection: $model.subtitleAlignment, options: [("left", "左"), ("center", "居中"), ("right", "右")]) }
            SettingRow("只显示最近几句", detail: model.customSubtitleCount ? nil : "关闭时显示全部当前字幕") {
                HStack(spacing: 8) {
                    if model.customSubtitleCount {
                        Stepper(value: $model.visibleLines, in: 1...10) { Text("\(model.visibleLines)").monospacedDigit() }.fixedSize()
                    }
                    ToggleSwitch(title: "限制最近字幕条数", isOn: $model.customSubtitleCount)
                }
            }
            SwitchSetting(title: "更早显示草稿", detail: "识别中的句子以灰色先显示", isOn: $model.showDrafts)
            SwitchSetting(title: "显示确认时间", isOn: $model.showTime)
            SwitchSetting(title: "显示句间分隔线", isOn: $model.showDividers)
        }
    }

    private var colors: some View {
        SettingsGroup("颜色") {
            SettingRow("系统声音") { ColorSwatches(title: "系统声音字幕颜色", hex: $model.foregroundHex, presets: ["FFFFFF", "FFE08A", "9FD3FF", "B8F0C2"], allowsAutomatic: true) }
            SettingRow("麦克风") { ColorSwatches(title: "麦克风字幕颜色", hex: $model.microphoneHex, presets: ["F3CE78", "FFFFFF", "9FD3FF", "B8F0C2"]) }
            SettingRow("浮窗背景") { SegmentedChoice(title: "浮窗背景", selection: $model.overlayBackground, options: [("dark", "深色"), ("light", "浅色"), ("system", "跟随外观")]) }
            SettingRow("背景不透明度") {
                HStack {
                    Slider(value: $model.overlayOpacity, in: 0...1).frame(width: 180).accessibilityLabel("背景不透明度")
                    Text("\(Int((model.overlayOpacity * 100).rounded()))%").font(.system(size: 12, design: .monospaced)).foregroundStyle(.secondary).frame(width: 40, alignment: .trailing)
                }
            }
        }
    }

    private var behaviour: some View {
        SettingsGroup("浮窗行为") {
            SwitchSetting(title: "沉浸模式", detail: "隐藏背景，只保留带阴影的文字", isOn: $model.immersive)
            SwitchSetting(title: "点击穿透", detail: "鼠标可以点到浮窗下面的窗口；可从菜单栏恢复", isOn: $model.clickThrough)
            SwitchSetting(title: "锁定位置与大小", isOn: $model.overlayLocked)
            SwitchSetting(title: "显示来源标签", detail: "在每句前标出系统声音或麦克风", isOn: $model.showSourceLabel)
            SwitchSetting(title: "在浮窗显示服务与语言", isOn: $model.showSessionInfo)
            SettingRow("位置") { Button("恢复默认位置") { overlay.resetPosition() }.buttonStyle(BorderedButtonStyle()) }
        }
    }

    private var motion: some View {
        SettingsGroup("动效") {
            SwitchSetting(title: "字幕滚动动画", isOn: $model.animateCaptions)
            SettingRow("状态动效", detail: "表示连接活动，不代表音量；始终遵循系统“减少动态效果”") {
                SegmentedChoice(title: "状态动效", selection: $model.pulseAnimation, options: [("system", "跟随系统"), ("on", "开启"), ("off", "关闭")])
            }
            SettingRow("状态样式") { SegmentedChoice(title: "状态样式", selection: $model.pulseStyle, options: [("syllable", "呼吸圆环"), ("ribbon", "流动线条")]) }
        }
    }
}

private enum FontCatalog {
    static let options = [("", "系统默认")] + NSFontManager.shared.availableFontFamilies.sorted().map { ($0, $0) }
}
