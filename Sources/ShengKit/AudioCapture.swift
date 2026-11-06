import Foundation
import AVFoundation
import ScreenCaptureKit
import CoreMedia
import OSLog

final class PCMConverter {
    private let rate: Double
    private var converter: AVAudioConverter?
    private var inputFormat: AVAudioFormat?
    init(rate: Double) { self.rate = rate }
    func convert(_ input: AVAudioPCMBuffer) -> Data? {
        guard let target = AVAudioFormat(commonFormat: .pcmFormatInt16, sampleRate: rate, channels: 1, interleaved: true) else { return nil }
        if inputFormat != input.format { inputFormat = input.format; converter = AVAudioConverter(from: input.format, to: target) }
        guard let converter, let output = AVAudioPCMBuffer(pcmFormat: target, frameCapacity: AVAudioFrameCount(ceil(Double(input.frameLength) * rate / input.format.sampleRate)) + 32) else { return nil }
        var delivered = false
        var error: NSError?
        let result = converter.convert(to: output, error: &error) { _, status in
            if delivered { status.pointee = .noDataNow; return nil }
            delivered = true; status.pointee = .haveData; return input
        }
        guard result != .error, error == nil else { return nil }
        let buffer = output.audioBufferList.pointee.mBuffers
        guard let bytes = buffer.mData, buffer.mDataByteSize > 0 else { return nil }
        return Data(bytes: bytes, count: Int(buffer.mDataByteSize))
    }
}
public final class AudioCapture: NSObject, SCStreamOutput, SCStreamDelegate, @unchecked Sendable {
    public typealias Handler = @Sendable (AudioSource, Data) -> Void
    private let deliveryLock = NSLock()
    private var accepting = false
    private var stream: SCStream?
    private var audioTap: AnyObject?
    private var microphone: AVAudioEngine?
    private var microphoneTapped = false
    private let systemQueue = DispatchQueue(label: "app.xlle.audio.system")
    private var systemConverter: PCMConverter?
    private var micConverter: PCMConverter?
    private let handler: Handler
    private let failed: @Sendable () -> Void
    public init(handler: @escaping Handler, failed: @escaping @Sendable () -> Void) { self.handler = handler; self.failed = failed }
    public func start(input: AudioInput, sampleRate: UInt32, microphoneUID: String = "", systemProcessID: Int32? = nil) async throws {
        deliveryLock.withLock { accepting = false }
        do {
            if input == .microphone || input == .both {
                let allowed = await AVCaptureDevice.requestAccess(for: .audio)
                guard allowed else { throw EngineError(code: "microphone_permission_denied") }
                let engine = AVAudioEngine()
                microphone = engine
                try AudioDevices.selectMicrophone(microphoneUID, engine: engine)
                let node = engine.inputNode
                let format = node.outputFormat(forBus: 0)
                guard format.sampleRate > 0, format.channelCount > 0 else { throw EngineError(code: "microphone_unavailable") }
                let converter = PCMConverter(rate: Double(sampleRate)); micConverter = converter
                node.installTap(onBus: 0, bufferSize: 2048, format: format) { [weak self] buffer, _ in
                    if let data = converter.convert(buffer) { self?.deliver(data, source: .microphone) }
                }
                microphoneTapped = true
                try engine.start()
            }
            if input == .system || input == .both {
                if #available(macOS 14.2, *) {
                    Logger(subsystem: L10n.currentProduct.bundleID, category: "AudioCapture").notice("capture_backend=core_audio_tap")
                    let tap = SystemAudioTap(handler: { [weak self] in self?.deliver($0, source: .system) }, failed: failed)
                    audioTap = tap
                    try tap.start(rate: sampleRate, processID: systemProcessID)
                    deliveryLock.withLock { accepting = true }
                    return
                }
                let content = try await SCShareableContent.excludingDesktopWindows(false, onScreenWindowsOnly: true)
                guard let display = content.displays.first else { throw EngineError(code: "display_unavailable") }
                let filter: SCContentFilter
                if let systemProcessID {
                    guard let application = content.applications.first(where: { $0.processID == systemProcessID }) else { throw EngineError(code: "audio_application_unavailable") }
                    filter = SCContentFilter(display: display, including: [application], exceptingWindows: [])
                } else { filter = SCContentFilter(display: display, excludingWindows: []) }
                let configuration = SCStreamConfiguration()
                configuration.width = 64; configuration.height = 64
                configuration.minimumFrameInterval = CMTime(value: 1, timescale: 1)
                configuration.capturesAudio = true
                configuration.excludesCurrentProcessAudio = true
                configuration.sampleRate = 48_000
                configuration.channelCount = 1
                systemConverter = PCMConverter(rate: Double(sampleRate))
                let capture = SCStream(filter: filter, configuration: configuration, delegate: self)
                try capture.addStreamOutput(self, type: .audio, sampleHandlerQueue: systemQueue)
                stream = capture
                try await capture.startCapture()
            }
            deliveryLock.withLock { accepting = true }
        } catch {
            let native = error as NSError
            let domain = native.domain == SCStreamErrorDomain ? "ScreenCaptureKit" : error is EngineError ? "ShengCapture" : "NativeAudio"
            Logger(subsystem: L10n.currentProduct.bundleID, category: "AudioCapture").error("capture_start_failed domain=\(domain, privacy: .public) code=\(native.code, privacy: .public)")
            await stop(); throw CaptureFailure.classify(error)
        }
    }
    public func stop() async {
        deliveryLock.withLock { accepting = false }
        if #available(macOS 14.2, *), let tap = audioTap as? SystemAudioTap { tap.stop() }
        audioTap = nil
        if let microphone {
            microphone.stop()
            if microphoneTapped { microphone.inputNode.removeTap(onBus: 0) }
        }
        microphoneTapped = false; microphone = nil
        if let stream { try? await stream.stopCapture() }
        stream = nil
        // Converters are released with the capture object, after callbacks stop.
    }
    private func deliver(_ data: Data, source: AudioSource) {
        let active = deliveryLock.withLock { accepting }
        guard active else { return }
        // Keep each admission within the Rust queue's frame budget.
        for offset in stride(from: 0, to: data.count, by: 6400) {
            handler(source, data.subdata(in: offset..<min(offset + 6400, data.count)))
        }
    }
    public func stream(_ stream: SCStream, didStopWithError error: Error) { failed() }
    public func stream(_ stream: SCStream, didOutputSampleBuffer sampleBuffer: CMSampleBuffer, of type: SCStreamOutputType) {
        guard type == .audio, sampleBuffer.isValid,
              let description = CMSampleBufferGetFormatDescription(sampleBuffer),
              let streamDescription = CMAudioFormatDescriptionGetStreamBasicDescription(description),
              let format = AVAudioFormat(streamDescription: streamDescription) else { return }
        let count = CMSampleBufferGetNumSamples(sampleBuffer)
        guard count > 0, let pcm = AVAudioPCMBuffer(pcmFormat: format, frameCapacity: AVAudioFrameCount(count)) else { return }
        pcm.frameLength = AVAudioFrameCount(count)
        let status = CMSampleBufferCopyPCMDataIntoAudioBufferList(sampleBuffer, at: 0, frameCount: Int32(count), into: pcm.mutableAudioBufferList)
        guard status == noErr, let data = systemConverter?.convert(pcm) else { failed(); return }
        deliver(data, source: .system)
    }
}
