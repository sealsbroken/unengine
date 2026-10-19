import AVFoundation
import Speech
import SwiftUI

@main
struct UnApp: App {
    init() { un_mobile_init() }

    var body: some Scene {
        #if os(macOS)
        // The lesson's camera picture is landscape on a Mac; the window opens at a size that fits
        // it and the HUD, and never gets so small that the HUD overlaps.
        WindowGroup { LessonScreen().frame(minWidth: 760, minHeight: 600) }
            .defaultSize(width: 1100, height: 800)
            .windowResizability(.contentMinSize)
        #else
        WindowGroup { LessonScreen() }
        #endif
    }
}

enum Permissions {
    /// Camera, microphone and speech recognition, asked for one after the other at launch so the
    /// lesson can start right away and is not interrupted by prompts; `then` runs on the main
    /// thread once all three are answered (granted or not).
    static func request(then: @escaping @MainActor () -> Void) {
        AVCaptureDevice.requestAccess(for: .video) { _ in
            AVAudioApplication.requestRecordPermission { _ in
                SFSpeechRecognizer.requestAuthorization { _ in
                    Task { @MainActor in then() }
                }
            }
        }
    }
}
