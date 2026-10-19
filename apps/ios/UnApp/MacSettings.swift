#if os(macOS)
import AVFoundation
import CoreAudio
import SwiftUI

/// The Mac app's settings (Un ▸ Settings…, ⌘,): which camera looks at the table and which
/// microphone hears the child. Both follow the system's choice until one is picked here (the
/// camera chosen in macOS, which may be an iPhone as a Continuity Camera; the default input). A
/// picked device that is unplugged falls back to the system's choice.
enum MediaDevices {
    static let cameraKey = "camera"
    static let microphoneKey = "microphone"

    struct Device: Identifiable, Hashable {
        let id: String  // AVCaptureDevice.uniqueID; for a microphone also its Core Audio UID
        let name: String
    }

    static func cameras() -> [Device] {
        AVCaptureDevice.DiscoverySession(deviceTypes: [.builtInWideAngleCamera, .continuityCamera, .external],
                                         mediaType: .video, position: .unspecified)
            .devices.map { Device(id: $0.uniqueID, name: $0.localizedName) }
    }

    static func microphones() -> [Device] {
        AVCaptureDevice.DiscoverySession(deviceTypes: [.microphone, .external], mediaType: .audio, position: .unspecified)
            .devices.map { Device(id: $0.uniqueID, name: $0.localizedName) }
    }

    /// The picked camera if it is connected, else the system's.
    static func camera() -> AVCaptureDevice? {
        if let id = UserDefaults.standard.string(forKey: cameraKey), !id.isEmpty, let device = AVCaptureDevice(uniqueID: id) {
            return device
        }
        return AVCaptureDevice.systemPreferredCamera ?? AVCaptureDevice.default(for: .video)
    }

    static var systemCameraName: String? {
        (AVCaptureDevice.systemPreferredCamera ?? AVCaptureDevice.default(for: .video))?.localizedName
    }

    static var systemMicrophoneName: String? { AVCaptureDevice.default(for: .audio)?.localizedName }

    /// The Core Audio device picked for the microphone, if one is picked and connected; nil means
    /// the system's default input, which the audio engine follows by itself.
    static func microphoneDeviceID() -> AudioDeviceID? {
        guard let uid = UserDefaults.standard.string(forKey: microphoneKey), !uid.isEmpty else { return nil }
        return deviceID(uid: uid)
    }

    private static func deviceID(uid: String) -> AudioDeviceID? {
        var id = AudioDeviceID(0)
        var size = UInt32(MemoryLayout<AudioDeviceID>.size)
        var address = AudioObjectPropertyAddress(mSelector: kAudioHardwarePropertyTranslateUIDToDevice,
                                                 mScope: kAudioObjectPropertyScopeGlobal, mElement: kAudioObjectPropertyElementMain)
        var cfUID = uid as CFString
        let status = withUnsafeMutablePointer(to: &cfUID) { pointer in
            AudioObjectGetPropertyData(AudioObjectID(kAudioObjectSystemObject), &address,
                                       UInt32(MemoryLayout<CFString>.size), pointer, &size, &id)
        }
        return status == noErr && id != kAudioObjectUnknown ? id : nil
    }
}

struct MacSettings: View {
    /// The app's language, as the EN/RU switch saved it; unset, the Mac's own first language.
    @AppStorage("language") private var language = (Locale.preferredLanguages.first ?? "").hasPrefix("ru") ? "ru" : "en"
    @AppStorage(MediaDevices.cameraKey) private var camera = ""
    @AppStorage(MediaDevices.microphoneKey) private var microphone = ""
    @State private var cameras: [MediaDevices.Device] = []
    @State private var microphones: [MediaDevices.Device] = []

    private var t: Copy { Copy(ru: language == "ru") }

    var body: some View {
        Form {
            Picker(t("settings.camera"), selection: $camera) {
                Text(systemChoice(MediaDevices.systemCameraName)).tag("")
                Divider()
                ForEach(cameras) { Text($0.name).tag($0.id) }
                if !camera.isEmpty, !cameras.contains(where: { $0.id == camera }) {
                    Text(t("settings.missing")).tag(camera)
                }
            }
            Text(t("settings.cameraHint")).font(.callout).foregroundStyle(.secondary)
            Picker(t("settings.microphone"), selection: $microphone) {
                Text(systemChoice(MediaDevices.systemMicrophoneName)).tag("")
                Divider()
                ForEach(microphones) { Text($0.name).tag($0.id) }
                if !microphone.isEmpty, !microphones.contains(where: { $0.id == microphone }) {
                    Text(t("settings.missing")).tag(microphone)
                }
            }
        }
        .formStyle(.grouped)
        .frame(width: 460)
        .fixedSize()
        .onAppear(perform: reload)
        // Devices come and go (a USB camera, an iPhone nearby): the lists follow.
        .onReceive(NotificationCenter.default.publisher(for: AVCaptureDevice.wasConnectedNotification)) { _ in reload() }
        .onReceive(NotificationCenter.default.publisher(for: AVCaptureDevice.wasDisconnectedNotification)) { _ in reload() }
        .onChange(of: microphone) { AudioIO.shared.microphoneChanged() }
    }

    private func systemChoice(_ name: String?) -> String {
        t("settings.system", ["name": name ?? "—"])
    }

    private func reload() {
        cameras = MediaDevices.cameras()
        microphones = MediaDevices.microphones()
    }
}
#endif
