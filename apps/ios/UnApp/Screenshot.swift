#if DEBUG
import CoreGraphics
import Foundation
import ImageIO
#if os(macOS)
import AppKit
#else
import UIKit
#endif

/// Screenshot mode, for the store screenshots (tools/screenshots.sh): the app opens one screen
/// with example children, and in a lesson a photo of toys on a table stands in for the camera
/// (its frames still go through the real detector, so the boxes and badges are genuine). Debug
/// builds only; set by the environment:
///
///   UN_SCREENSHOT=children|trail|lesson   the screen
///   UN_LANGUAGE=en|ru                     the language (and the example children's names)
///   UN_SCREENSHOT_SIZE=1440x900           the Mac window's content size, in points
///   UN_SCREENSHOT_DELAY=20                save the screen after this many seconds, then quit
///
/// In a lesson the camera's picture is shown as a still (the app's own capture cannot see a
/// live preview), or, when `screenshot-table.jpg` is in `folder`, that photo stands in for the
/// camera (the simulator has none). The Mac reads and writes /tmp/un-screenshots through a
/// debug-only sandbox exception (UnMac-Debug.entitlements). The example children
/// live in their own file, so the real children are untouched, and nothing the mode picks
/// (language, child, lesson) is saved.
enum Screenshot {
    private static let env = ProcessInfo.processInfo.environment

    static let screen: String? = env["UN_SCREENSHOT"].flatMap { $0.isEmpty ? nil : $0 }
    static var active: Bool { screen != nil }

    static var language: UnLanguage? {
        switch env["UN_LANGUAGE"] {
        case "ru": return .russian
        case "en": return .english
        default: return nil
        }
    }

    static var windowSize: CGSize? {
        let parts = (env["UN_SCREENSHOT_SIZE"] ?? "").split(separator: "x").compactMap { Double($0) }
        return parts.count == 2 ? CGSize(width: parts[0], height: parts[1]) : nil
    }

    private static var caches: URL {
        FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask).first!
    }

    /// The example children's file, emptied at every launch.
    static func profilesPath() -> String {
        let url = caches.appendingPathComponent("screenshot-profiles.json")
        try? FileManager.default.removeItem(at: url)
        return url.path
    }

    /// The folder shared with tools/screenshots.sh: /tmp/un-screenshots on the Mac and in the
    /// simulator (both read the host's files), Documents/screenshots on an iPhone or iPad (the
    /// script copies it with devicectl).
    static var folder: URL {
        if let dir = env["UN_SCREENSHOT_DIR"] { return URL(fileURLWithPath: dir) }
        #if os(macOS) || targetEnvironment(simulator)
        return URL(fileURLWithPath: "/tmp/un-screenshots")
        #else
        return FileManager.default.urls(for: .documentDirectory, in: .userDomainMask).first!.appendingPathComponent("screenshots")
        #endif
    }

    static var photoURL: URL { folder.appendingPathComponent("screenshot-table.jpg") }

    /// Three children at different points: one well along the trail, one who paused in a lesson,
    /// one just added. Returns the first, whose trail and lesson the screenshots show.
    static func seed(_ profiles: UnProfiles, lessons: [UnStrategyInfo], ru: Bool) -> String {
        let names = ru ? ["Маша", "Лёва", "Ева"] : ["Mia", "Leo", "Ava"]
        let ids = lessons.map(\.id)
        func summary(paused: Bool) -> UnSummary {
            UnSummary(tasksCompleted: 3, correct: 3, finalLevel: 1, records: [], paused: paused, remember: [:])
        }
        let first = profiles.add(name: names[0])
        for id in ids.prefix(4) { profiles.record(id: first.id, lessonId: id, summary: summary(paused: false)) }
        let second = profiles.add(name: names[1])
        for id in ids.prefix(1) { profiles.record(id: second.id, lessonId: id, summary: summary(paused: false)) }
        if ids.count > 1 { profiles.record(id: second.id, lessonId: ids[1], summary: summary(paused: true)) }
        _ = profiles.add(name: names[2])
        return first.id
    }
}

/// The table photo fed to perception as if from the camera, a few frames a second.
final class PhotoSource {
    let image: CGImage
    private let rgb: Data
    let width: UInt32
    let height: UInt32
    private var timer: DispatchSourceTimer?
    private let queue = DispatchQueue(label: "un.photo")

    init?(url: URL) {
        guard let source = CGImageSourceCreateWithURL(url as CFURL, nil),
              let full = CGImageSourceCreateImageAtIndex(source, 0, [kCGImageSourceShouldCacheImmediately: true] as CFDictionary)
        else { return nil }
        // The camera's size: the detector letterboxes it to its input anyway.
        let scale = min(1, 1280 / Double(max(full.width, full.height)))
        let w = Int(Double(full.width) * scale), h = Int(Double(full.height) * scale)
        var rgba = [UInt8](repeating: 0, count: w * h * 4)
        guard let context = CGContext(data: &rgba, width: w, height: h, bitsPerComponent: 8, bytesPerRow: w * 4,
                                      space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue)
        else { return nil }
        context.draw(full, in: CGRect(x: 0, y: 0, width: w, height: h))
        guard let scaled = context.makeImage() else { return nil }
        var packed = Data(count: w * h * 3)
        packed.withUnsafeMutableBytes { out in
            let p = out.bindMemory(to: UInt8.self)
            for i in 0..<(w * h) {
                p[i * 3] = rgba[i * 4]; p[i * 3 + 1] = rgba[i * 4 + 1]; p[i * 3 + 2] = rgba[i * 4 + 2]
            }
        }
        image = scaled
        rgb = packed
        width = UInt32(w)
        height = UInt32(h)
    }

    func start(_ handler: @escaping (_ rgb: Data, _ width: UInt32, _ height: UInt32, _ timestamp: Double) -> Void) {
        let started = Date()
        let timer = DispatchSource.makeTimerSource(queue: queue)
        timer.schedule(deadline: .now(), repeating: 0.2)
        timer.setEventHandler { [rgb, width, height] in handler(rgb, width, height, Date().timeIntervalSince(started)) }
        timer.resume()
        self.timer = timer
    }

    func stop() {
        timer?.cancel()
        timer = nil
    }
}

extension Screenshot {
    /// Seconds after launch at which the app saves its screen and quits (UN_SCREENSHOT_DELAY);
    /// unset, it stays open.
    static var captureDelay: Double? { env["UN_SCREENSHOT_DELAY"].flatMap(Double.init) }

    /// Where the app saves `screenshot.png` (and `camera.jpg`, the camera's still).
    static var outputURL: URL { folder.appendingPathComponent("screenshot.png") }

    /// After `captureDelay`, saves the screen (iPhone, iPad) or the window's content (Mac, sized
    /// to UN_SCREENSHOT_SIZE first) at the screen's scale, and the camera's latest still, then
    /// quits. The app draws itself, so no screen-recording permission is needed and there is no
    /// status bar.
    @MainActor static func scheduleCapture(still: @escaping @MainActor () -> CGImage?) {
        setvbuf(stdout, nil, _IOLBF, 0)  // the log line by line, also into a file
        try? FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        try? FileManager.default.removeItem(at: outputURL)
        #if os(macOS)
        func sizeWindow(_ tries: Int) {
            guard let window = NSApp.windows.first(where: { $0.isVisible }) else {
                if tries > 0 { DispatchQueue.main.asyncAfter(deadline: .now() + 0.2) { sizeWindow(tries - 1) } }
                return
            }
            if let size = windowSize {
                window.setContentSize(size)
                window.center()
            }
        }
        sizeWindow(50)
        #endif
        guard let delay = captureDelay else { return }
        DispatchQueue.main.asyncAfter(deadline: .now() + delay) {
            if let still = still(), let destination = CGImageDestinationCreateWithURL(
                folder.appendingPathComponent("camera.jpg") as CFURL, "public.jpeg" as CFString, 1, nil) {
                CGImageDestinationAddImage(destination, still, nil)
                CGImageDestinationFinalize(destination)
            }
            #if os(macOS)
            if let view = NSApp.windows.first(where: { $0.isVisible })?.contentView,
               let rep = view.bitmapImageRepForCachingDisplay(in: view.bounds) {
                view.cacheDisplay(in: view.bounds, to: rep)
                try? rep.representation(using: .png, properties: [:])?.write(to: outputURL)
            }
            #else
            if let window = UIApplication.shared.connectedScenes.compactMap({ ($0 as? UIWindowScene)?.keyWindow }).first {
                let image = UIGraphicsImageRenderer(bounds: window.bounds).image { _ in
                    window.drawHierarchy(in: window.bounds, afterScreenUpdates: true)
                }
                try? image.pngData()?.write(to: outputURL)
            }
            #endif
            print("screenshot: \(outputURL.path)")
            exit(0)
        }
    }
}
#endif
