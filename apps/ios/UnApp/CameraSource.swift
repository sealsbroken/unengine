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

    func start() { queue.async { self.session.startRunning() } }
    func stop() { queue.async { self.session.stopRunning() } }

    func captureOutput(_ output: AVCaptureOutput, didOutput sampleBuffer: CMSampleBuffer, from connection: AVCaptureConnection) {
        guard let pixelBuffer = CMSampleBufferGetImageBuffer(sampleBuffer) else { return }
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

    func updateUIView(_ uiView: PreviewView, context: Context) {}
}
