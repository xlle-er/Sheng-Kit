import Foundation
import CoreAudio
import AVFoundation
import AppKit

/// Audio-only capture. Owns a private, nonpersistent tap and aggregate device.
@available(macOS 14.2, *)
final class SystemAudioTap: @unchecked Sendable {
    private var tap: AudioObjectID = 0
    private var device: AudioObjectID = 0
    private var ioProc: AudioDeviceIOProcID?
    private var started = false
    private let lock = NSLock()
    private var accepting = false
    private let slots = DispatchSemaphore(value: 32)
    private let callbackQueue = DispatchQueue(label: "app.xlle.capture.tap.callback", qos: .userInteractive)
    private let conversionKey = DispatchSpecificKey<Bool>()
    private var healthTimer: DispatchSourceTimer?
    private let conversionQueue = DispatchQueue(label: "app.xlle.capture.tap.convert", qos: .userInitiated)
    private let handler: @Sendable (Data) -> Void
    private let failed: @Sendable () -> Void
    init(handler: @escaping @Sendable (Data) -> Void, failed: @escaping @Sendable () -> Void) {
        self.handler = handler; self.failed = failed
        conversionQueue.setSpecific(key: conversionKey, value: true)
    }
    func start(rate: UInt32, processID: Int32?) throws {
        do {
            let description: CATapDescription
            if let processID {
                if #available(macOS 26.0, *), let bundleID = NSRunningApplication(processIdentifier: processID)?.bundleIdentifier {
                    description = CATapDescription(monoMixdownOfProcesses: [])
                    description.bundleIDs = [bundleID]
                    description.isProcessRestoreEnabled = true
                } else {
                    description = CATapDescription(monoMixdownOfProcesses: [try Self.processObject(processID)])
                }
            } else {
                let ownProcess = try? Self.processObject(ProcessInfo.processInfo.processIdentifier)
                description = CATapDescription(monoGlobalTapButExcludeProcesses: ownProcess.map { [$0] } ?? [])
            }
            description.name = L10n.named("%@系统音频"); description.isPrivate = true; description.muteBehavior = .unmuted
            try Self.check(AudioHardwareCreateProcessTap(description, &tap))
            var address = AudioObjectPropertyAddress(mSelector: kAudioTapPropertyFormat, mScope: kAudioObjectPropertyScopeGlobal, mElement: kAudioObjectPropertyElementMain)
            var asbd = AudioStreamBasicDescription(), size = UInt32(MemoryLayout<AudioStreamBasicDescription>.size)
            try Self.check(AudioObjectGetPropertyData(tap, &address, 0, nil, &size, &asbd))
            guard asbd.mFormatID == kAudioFormatLinearPCM, asbd.mBytesPerFrame > 0,
                  let format = AVAudioFormat(streamDescription: &asbd) else { throw EngineError(code: "system_audio_format_unavailable") }
            let properties: [String: Any] = [
                kAudioAggregateDeviceNameKey: L10n.named("%@私有音频输入"),
                kAudioAggregateDeviceUIDKey: UUID().uuidString,
                kAudioAggregateDeviceIsPrivateKey: true,
                kAudioAggregateDeviceIsStackedKey: false,
                kAudioAggregateDeviceTapAutoStartKey: false,
                kAudioAggregateDeviceTapListKey: [[kAudioSubTapUIDKey: description.uuid.uuidString, kAudioSubTapDriftCompensationKey: true]]
            ]
            try Self.check(AudioHardwareCreateAggregateDevice(properties as CFDictionary, &device))
            let converter = PCMConverter(rate: Double(rate))
            let bytesPerFrame = asbd.mBytesPerFrame
            lock.withLock { accepting = true }
            try Self.check(AudioDeviceCreateIOProcIDWithBlock(&ioProc, device, callbackQueue) { [weak self] _, input, _, _, _ in
                guard let self, self.lock.withLock({ self.accepting }) else { return }
                let buffers = UnsafeMutableAudioBufferListPointer(UnsafeMutablePointer(mutating: input))
                guard !buffers.isEmpty else { return } // A quiet tap may not have an input stream yet.
                guard buffers.count <= 8 else { self.failed(); return }
                let total = buffers.reduce(0) { $0 + Int($1.mDataByteSize) }
                guard total > 0 else { return }
                guard total <= 64 * 1024, self.slots.wait(timeout: .now()) == .success else { self.failed(); return }
                let packets = buffers.map { buffer -> Data in
                    guard let data = buffer.mData else { return Data() }
                    return Data(bytes: data, count: Int(buffer.mDataByteSize))
                }
                self.conversionQueue.async { [weak self] in
                    guard let self else { return }
                    defer { self.slots.signal() }
                    guard self.lock.withLock({ self.accepting }), let first = packets.first, !first.isEmpty,
                          let pcm = AVAudioPCMBuffer(pcmFormat: format, frameCapacity: AVAudioFrameCount(first.count / Int(bytesPerFrame))) else { return }
                    pcm.frameLength = pcm.frameCapacity
                    let destination = UnsafeMutableAudioBufferListPointer(pcm.mutableAudioBufferList)
                    guard destination.count == packets.count else { self.failed(); return }
                    for index in packets.indices {
                        guard let pointer = destination[index].mData, packets[index].count <= Int(destination[index].mDataByteSize) else { self.failed(); return }
                        packets[index].copyBytes(to: pointer.assumingMemoryBound(to: UInt8.self), count: packets[index].count)
                    }
                    if let converted = converter.convert(pcm) { self.handler(converted) }
                    else { self.failed() }
                }
            })
            try Self.check(AudioDeviceStart(device, ioProc)); started = true
            let expected = asbd
            let health = DispatchSource.makeTimerSource(queue: conversionQueue)
            health.schedule(deadline: .now() + .milliseconds(500), repeating: .milliseconds(500))
            health.setEventHandler { [weak self] in
                guard let self, self.lock.withLock({ self.accepting }) else { return }
                var address = AudioObjectPropertyAddress(mSelector: kAudioTapPropertyFormat, mScope: kAudioObjectPropertyScopeGlobal, mElement: kAudioObjectPropertyElementMain)
                var current = AudioStreamBasicDescription(), size = UInt32(MemoryLayout<AudioStreamBasicDescription>.size)
                let status = AudioObjectGetPropertyData(self.tap, &address, 0, nil, &size, &current)
                if status != noErr || current.mSampleRate != expected.mSampleRate || current.mChannelsPerFrame != expected.mChannelsPerFrame || current.mFormatFlags != expected.mFormatFlags { self.failed() }
            }
            health.resume(); healthTimer = health
        } catch { stop(); throw error }
    }
    func stop() {
        lock.withLock { accepting = false }
        healthTimer?.cancel(); healthTimer = nil
        if started { AudioDeviceStop(device, ioProc); started = false }
        if let ioProc { AudioDeviceDestroyIOProcID(device, ioProc); self.ioProc = nil }
        // Drain copied packets before destroying the input format/device ownership.
        if DispatchQueue.getSpecific(key: conversionKey) != true { conversionQueue.sync {} }
        if device != 0 { AudioHardwareDestroyAggregateDevice(device); device = 0 }
        if tap != 0 { AudioHardwareDestroyProcessTap(tap); tap = 0 }
    }
    deinit { stop() }
    private static func check(_ status: OSStatus) throws {
        guard status == noErr else {
            if status == kAudioDevicePermissionsError { throw EngineError(code: "system_audio_permission_denied") }
            throw EngineError(code: "core_audio_tap_\(status)")
        }
    }
    private static func processObject(_ pid: Int32) throws -> AudioObjectID {
        var address = AudioObjectPropertyAddress(mSelector: kAudioHardwarePropertyTranslatePIDToProcessObject, mScope: kAudioObjectPropertyScopeGlobal, mElement: kAudioObjectPropertyElementMain)
        var pid = pid, result: AudioObjectID = 0, size = UInt32(MemoryLayout<AudioObjectID>.size)
        try check(AudioObjectGetPropertyData(AudioObjectID(kAudioObjectSystemObject), &address, UInt32(MemoryLayout<Int32>.size), &pid, &size, &result))
        guard result != 0 else { throw EngineError(code: "audio_application_unavailable") }
        return result
    }
}
