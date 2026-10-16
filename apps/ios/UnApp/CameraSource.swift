import AVFoundation
import CoreImage
import SwiftUI

/// Front or back camera frames as NV12 planes, handed to the shared perception (`UnPerception.processNv12`).
final class CameraSource: NSObject, AVCaptureVideoDataOutputSampleBufferDelegate {
    typealias FrameHandler = (_ y: Data, _ uv: Data, _ width: UInt32, _ height: UInt32, _ yStride: UInt32, _ uvStride: UInt32, _ timestamp: Double) -> Void

    let session = AVCaptureSession()
    private let queue = DispatchQueue(label: "un.camera")
    private let handler: FrameHandler
    private let started = Date()
    #if DEBUG
    /// Debug builds keep the last few camera frames in Documents/frames (frame-0.jpg ... frame-4.jpg,
    /// one every two seconds) to see what the detector sees: `xcrun devicectl device copy from
    /// --domain-type appDataContainer --domain-identifier app.onym.undigit --source Documents/frames`.
    private var lastDump = Date.distantPast
    private var dumpIndex = 0
    private let ciContext = CIContext()
    #endif

    init(position: AVCaptureDevice.Position = .back, handler: @escaping FrameHandler) {
        self.handler = handler
        super.init()
        session.sessionPreset = .hd1280x720
        guard let device = AVCaptureDevice.default(.builtInWideAngleCamera, for: .video, position: position),
              let input = try? AVCaptureDeviceInput(device: device), session.canAddInput(input) else { return }
        session.addInput(input)
        let output = AVCaptureVideoDataOutput()
        output.videoSettings = [kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange]
        output.alwaysDiscardsLateVideoFrames = true
        output.setSampleBufferDelegate(self, queue: queue)
        if session.canAddOutput(output) { session.addOutput(output) }
        output.connection(with: .video)?.videoRotationAngle = 90
    }

    #if DEBUG
    static let framesDirectory: URL? = {
        guard let docs = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask).first else { return nil }
        let dir = docs.appendingPathComponent("frames")
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        return dir
    }()
    #endif

    func start() { queue.async { self.session.startRunning() } }
    func stop() { queue.async { self.session.stopRunning() } }

    func captureOutput(_ output: AVCaptureOutput, didOutput sampleBuffer: CMSampleBuffer, from connection: AVCaptureConnection) {
        guard let pixelBuffer = CMSampleBufferGetImageBuffer(sampleBuffer) else { return }
        #if DEBUG
        if Date().timeIntervalSince(lastDump) > 2, let dir = Self.framesDirectory {
            lastDump = Date()
            let image = CIImage(cvPixelBuffer: pixelBuffer)
            if let data = ciContext.jpegRepresentation(of: image, colorSpace: CGColorSpaceCreateDeviceRGB()) {
                try? data.write(to: dir.appendingPathComponent("frame-\(dumpIndex).jpg"))
                dumpIndex = (dumpIndex + 1) % 5
            }
        }
        #endif
        CVPixelBufferLockBaseAddress(pixelBuffer, .readOnly)
        defer { CVPixelBufferUnlockBaseAddress(pixelBuffer, .readOnly) }
        let width = UInt32(CVPixelBufferGetWidth(pixelBuffer))
        let height = UInt32(CVPixelBufferGetHeight(pixelBuffer))
        guard let yBase = CVPixelBufferGetBaseAddressOfPlane(pixelBuffer, 0),
              let uvBase = CVPixelBufferGetBaseAddressOfPlane(pixelBuffer, 1) else { return }
        let yStride = CVPixelBufferGetBytesPerRowOfPlane(pixelBuffer, 0)
        let uvStride = CVPixelBufferGetBytesPerRowOfPlane(pixelBuffer, 1)
        let y = Data(bytes: yBase, count: yStride * Int(height))
        let uv = Data(bytes: uvBase, count: uvStride * Int(height) / 2)
        handler(y, uv, width, height, UInt32(yStride), UInt32(uvStride), Date().timeIntervalSince(started))
    }
}

/// The camera's own picture behind the lesson, so the child and the grown-up see what the tutor sees.
struct CameraPreview: UIViewRepresentable {
    let session: AVCaptureSession

    final class PreviewView: UIView {
        override class var layerClass: AnyClass { AVCaptureVideoPreviewLayer.self }
        var previewLayer: AVCaptureVideoPreviewLayer { layer as! AVCaptureVideoPreviewLayer }
    }

    func makeUIView(context: Context) -> PreviewView {
        let view = PreviewView()
        view.previewLayer.session = session
        view.previewLayer.videoGravity = .resizeAspectFill
        return view
    }

    /// A restarted lesson brings a new capture session; SwiftUI keeps this view, so it must follow
    /// (left on the old, stopped session it showed a frozen picture).
    func updateUIView(_ uiView: PreviewView, context: Context) {
        if uiView.previewLayer.session !== session {
            uiView.previewLayer.session = session
        }
    }
}
