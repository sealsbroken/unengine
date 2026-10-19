import AVFoundation
import SwiftUI
#if os(macOS)
import AppKit
import IOKit.pwr_mgt
#else
import UIKit
#endif

// The few places iOS and macOS differ. The screens are the same SwiftUI code on both; the iOS
// screen is the reference, and the Mac app is that screen in a window.

#if os(macOS)
typealias PlatformColor = NSColor

extension NSColor {
    /// A colour that follows the window's light or dark appearance.
    static func dynamic(light: NSColor, dark: NSColor) -> NSColor {
        NSColor(name: nil) { $0.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua ? dark : light }
    }
}
#else
typealias PlatformColor = UIColor

extension UIColor {
    /// A colour that follows the light or dark appearance.
    static func dynamic(light: UIColor, dark: UIColor) -> UIColor {
        UIColor { $0.userInterfaceStyle == .dark ? dark : light }
    }
}
#endif

/// Keeps the screen awake while a lesson runs: the device stands by the table for minutes with
/// nobody touching it.
enum ScreenAwake {
    #if os(macOS)
    private static var assertion: IOPMAssertionID = 0
    #endif

    static func set(_ on: Bool) {
        #if os(macOS)
        if on, assertion == 0 {
            IOPMAssertionCreateWithName(kIOPMAssertionTypePreventUserIdleDisplaySleep as CFString,
                                        IOPMAssertionLevel(kIOPMAssertionLevelOn), "A lesson is running" as CFString, &assertion)
        } else if !on, assertion != 0 {
            IOPMAssertionRelease(assertion)
            assertion = 0
        }
        #else
        UIApplication.shared.isIdleTimerDisabled = on
        #endif
    }
}

extension View {
    /// The lesson over everything: a full-screen cover on iOS (no swipe-back mid-lesson), the
    /// whole window on the Mac.
    @ViewBuilder
    func lessonCover<Content: View>(isPresented: Binding<Bool>, @ViewBuilder content: @escaping () -> Content) -> some View {
        #if os(macOS)
        overlay {
            if isPresented.wrappedValue {
                content().transition(.opacity)
            }
        }
        .animation(.easeInOut(duration: 0.25), value: isPresented.wrappedValue)
        #else
        fullScreenCover(isPresented: isPresented, content: content)
        #endif
    }

    /// An inline navigation title where iOS has one.
    @ViewBuilder
    func inlineNavigationTitle() -> some View {
        #if os(macOS)
        self
        #else
        navigationBarTitleDisplayMode(.inline)
        #endif
    }
}

/// The camera that looks at the table. On iPhone and iPad the back camera. A Mac has no back
/// camera, so it takes an iPhone used as a Continuity Camera, else an external camera, else the
/// built-in one (which faces the user; tilt the screen or put the toys in front of it).
func tableCamera() -> AVCaptureDevice? {
    #if os(macOS)
    let session = AVCaptureDevice.DiscoverySession(deviceTypes: [.continuityCamera, .external, .builtInWideAngleCamera],
                                                   mediaType: .video, position: .unspecified)
    for type in [AVCaptureDevice.DeviceType.continuityCamera, .external, .builtInWideAngleCamera] {
        if let device = session.devices.first(where: { $0.deviceType == type }) { return device }
    }
    return AVCaptureDevice.default(for: .video)
    #else
    return AVCaptureDevice.default(.builtInWideAngleCamera, for: .video, position: .back)
    #endif
}

/// How the camera's frames are turned before the detector sees them: portrait on iPhone and iPad
/// (the app is portrait-only), as they come on the Mac (a landscape window).
let cameraRotationAngle: CGFloat = {
    #if os(macOS)
    return 0
    #else
    return 90
    #endif
}()
