import SwiftUI
import AVFoundation

@MainActor final class SessionAudioPlayer: ObservableObject {
    @Published var playing = false
    @Published var position = 0.0
    @Published var duration = 0.0
    @Published var failure = false
    private var player: AVAudioPlayer?
    private var timer: Timer?
    func open(_ url: URL) {
        stop()
        do { player = try AVAudioPlayer(contentsOf: url); player?.prepareToPlay(); duration = player?.duration ?? 0; failure = false }
        catch { failure = true }
    }
    func toggle() {
        guard let player else { return }
        if player.isPlaying { player.pause(); playing = false; timer?.invalidate(); timer = nil }
        else {
            if player.currentTime >= player.duration { player.currentTime = 0 }
            playing = player.play()
            timer = Timer.scheduledTimer(withTimeInterval: 0.2, repeats: true) { [weak self] _ in Task { @MainActor in self?.refresh() } }
        }
    }
    private func refresh() { position = player?.currentTime ?? 0; playing = player?.isPlaying ?? false; if !playing { timer?.invalidate(); timer = nil } }
    func seek(_ value: Double) { player?.currentTime = min(max(value, 0), duration); position = player?.currentTime ?? 0 }
    func stop() { timer?.invalidate(); timer = nil; player?.stop(); player = nil; playing = false; position = 0; duration = 0 }
    deinit { timer?.invalidate() }
}
struct SessionAudioView: View {
    let session: SavedSession
    @State private var source = ""
    @StateObject private var player = SessionAudioPlayer()
    var body: some View {
        HStack(spacing: 12) {
            Button { player.toggle() } label: {
                Image(systemName: player.playing ? "pause.fill" : "play.fill").font(.system(size: 13)).frame(width: 30, height: 30)
                    .foregroundStyle(.white).background(ShengStyle.accent, in: Circle())
            }
            .buttonStyle(.plain).disabled(player.duration == 0)
            .accessibilityLabel(Text(LocalizedStringKey(player.playing ? "暂停" : "播放")))
            if session.audioSources.count > 1 {
                ChoiceControl(title: "音频", selection: $source, options: session.audioSources.map { ($0.rawValue, $0.title) }).frame(width: 130)
            } else if let only = session.audioSources.first {
                Label(only.title, systemImage: only == .system ? "speaker.wave.2" : "mic").font(.system(size: 12)).foregroundStyle(.secondary)
            }
            Slider(value: Binding(get: { player.position }, set: { player.seek($0) }), in: 0...max(player.duration, 1)).accessibilityLabel(Text("播放进度"))
            Text(Self.clock(player.position) + " / " + Self.clock(player.duration)).font(.system(size: 11)).monospacedDigit().foregroundStyle(.secondary)
            if player.failure { Text("音频读取失败。").font(.system(size: 11)).foregroundStyle(.orange) }
        }
        .padding(.horizontal, 12).padding(.vertical, 8)
        .background(ShengStyle.field, in: RoundedRectangle(cornerRadius: 10))
        .onAppear { source = session.audioSources.first?.rawValue ?? ""; load() }
        .onChange(of: source) { _ in load() }.onDisappear { player.stop() }
        .onReceive(NotificationCenter.default.publisher(for: NSApplication.willTerminateNotification)) { _ in player.stop() }
    }
    static func clock(_ seconds: Double) -> String {
        let total = Int(max(0, seconds).rounded(.down))
        return total >= 3600 ? String(format: "%d:%02d:%02d", total / 3600, total / 60 % 60, total % 60) : String(format: "%d:%02d", total / 60, total % 60)
    }
    private func load() { if !source.isEmpty { player.open(session.directory.appendingPathComponent(source + ".wav")) } }
}
