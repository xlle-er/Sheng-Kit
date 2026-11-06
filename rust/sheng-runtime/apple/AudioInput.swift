import AVFoundation
import Speech

/// Turns 16 kHz mono PCM16 chunks into analyzer input, converting when the analyzer
/// prefers another format.
@available(macOS 26, *)
final class AudioInput: @unchecked Sendable {
    static let sampleRate = 16_000.0
    static let sourceFormat = AVAudioFormat(
        commonFormat: .pcmFormatInt16, sampleRate: sampleRate, channels: 1, interleaved: true
    )!
    /// Extra output frames allowed for resampler rounding.
    private static let conversionSlack: AVAudioFrameCount = 64

    private let targetFormat: AVAudioFormat
    private let converter: AVAudioConverter?

    init(analyzerFormat: AVAudioFormat?) {
        let target = analyzerFormat ?? Self.sourceFormat
        targetFormat = target
        converter = target == Self.sourceFormat ? nil : AVAudioConverter(from: Self.sourceFormat, to: target)
    }

    func input(_ data: Data) -> AnalyzerInput? {
        guard let buffer = Self.buffer(data) else { return nil }
        guard let converter else { return AnalyzerInput(buffer: buffer) }
        return convert(buffer, with: converter).map { AnalyzerInput(buffer: $0) }
    }

    private static func buffer(_ data: Data) -> AVAudioPCMBuffer? {
        let frames = data.count / MemoryLayout<Int16>.size
        guard frames > 0,
              let buffer = AVAudioPCMBuffer(pcmFormat: sourceFormat, frameCapacity: AVAudioFrameCount(frames)),
              let channel = buffer.int16ChannelData?[0]
        else { return nil }
        buffer.frameLength = AVAudioFrameCount(frames)
        data.withUnsafeBytes { raw in
            guard let base = raw.baseAddress else { return }
            memcpy(channel, base, frames * MemoryLayout<Int16>.size)
        }
        return buffer
    }

    private func convert(_ buffer: AVAudioPCMBuffer, with converter: AVAudioConverter) -> AVAudioPCMBuffer? {
        let ratio = targetFormat.sampleRate / Self.sourceFormat.sampleRate
        let capacity = AVAudioFrameCount(Double(buffer.frameLength) * ratio) + Self.conversionSlack
        guard let output = AVAudioPCMBuffer(pcmFormat: targetFormat, frameCapacity: capacity) else { return nil }
        var supplied = false
        var failure: NSError?
        converter.convert(to: output, error: &failure) { _, status in
            if supplied {
                status.pointee = .noDataNow
                return nil
            }
            supplied = true
            status.pointee = .haveData
            return buffer
        }
        guard failure == nil, output.frameLength > 0 else { return nil }
        return output
    }
}
