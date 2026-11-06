import Foundation
import CoreAudio
import AudioToolbox
import ScreenCaptureKit

public struct MicrophoneDevice: Identifiable {
    public let id: String
    public let deviceID: AudioDeviceID
    public let name: String
}
public enum AudioDevices {
    public static func microphones() -> [MicrophoneDevice] {
        var address = AudioObjectPropertyAddress(mSelector: kAudioHardwarePropertyDevices, mScope: kAudioObjectPropertyScopeGlobal, mElement: kAudioObjectPropertyElementMain)
        var size: UInt32 = 0
        guard AudioObjectGetPropertyDataSize(AudioObjectID(kAudioObjectSystemObject), &address, 0, nil, &size) == noErr else { return [] }
        var devices = [AudioDeviceID](repeating: 0, count: Int(size) / MemoryLayout<AudioDeviceID>.size)
        let status = devices.withUnsafeMutableBytes { AudioObjectGetPropertyData(AudioObjectID(kAudioObjectSystemObject), &address, 0, nil, &size, $0.baseAddress!) }
        guard status == noErr else { return [] }
        return devices.compactMap { id in
            var streams = AudioObjectPropertyAddress(mSelector: kAudioDevicePropertyStreams, mScope: kAudioDevicePropertyScopeInput, mElement: kAudioObjectPropertyElementMain)
            var streamSize: UInt32 = 0
            guard AudioObjectGetPropertyDataSize(id, &streams, 0, nil, &streamSize) == noErr, streamSize > 0 else { return nil }
            var property = AudioObjectPropertyAddress(mSelector: kAudioObjectPropertyName, mScope: kAudioObjectPropertyScopeGlobal, mElement: kAudioObjectPropertyElementMain)
            var name: CFString = "" as CFString; var nameSize = UInt32(MemoryLayout<CFString>.size)
            guard withUnsafeMutablePointer(to: &name, { AudioObjectGetPropertyData(id, &property, 0, nil, &nameSize, $0) }) == noErr else { return nil }
            property.mSelector = kAudioDevicePropertyDeviceUID
            var uid: CFString = "" as CFString
            guard withUnsafeMutablePointer(to: &uid, { AudioObjectGetPropertyData(id, &property, 0, nil, &nameSize, $0) }) == noErr else { return nil }
            return MicrophoneDevice(id: uid as String, deviceID: id, name: name as String)
        }
    }
    static func selectMicrophone(_ uid: String, engine: AVAudioEngine) throws {
        guard !uid.isEmpty else { return }
        guard let device = microphones().first(where: { $0.id == uid }), let unit = engine.inputNode.audioUnit else { throw EngineError(code: "microphone_unavailable") }
        var id = device.deviceID
        guard AudioUnitSetProperty(unit, kAudioOutputUnitProperty_CurrentDevice, kAudioUnitScope_Global, 0, &id, UInt32(MemoryLayout<AudioDeviceID>.size)) == noErr else { throw EngineError(code: "microphone_selection_failed") }
    }
}
import AVFoundation
