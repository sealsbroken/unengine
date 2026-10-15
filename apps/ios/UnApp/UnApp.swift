import AVFoundation
import Speech
import SwiftUI

@main
struct UnApp: App {
    init() {
        unmethod_mobile_init()
        Self.requestPermissions()
    }

    var body: some Scene {
        WindowGroup { LessonScreen() }
    }

    /// Camera, microphone and speech recognition are asked for at launch, one after the other,
    /// so the first lesson starts without prompts in the middle of it.
    static func requestPermissions() {
        AVCaptureDevice.requestAccess(for: .video) { _ in
            AVAudioApplication.requestRecordPermission { _ in
                SFSpeechRecognizer.requestAuthorization { _ in }
            }
        }
    }
}
