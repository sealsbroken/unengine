import SwiftUI

/// What the tutor sees, drawn over the camera picture as on the desktop: a box around every
/// recognised object (amber while a hand is over the table, green once the table is settled,
/// white while it is still changing), the objects' stable numbers, lit red up to the number
/// the tutor is counting out, the fingers the detector saw in blue (never counted), and the
/// hand skeletons with the raised-finger count.
struct TableOverlay: View {
    let table: UnTableState?
    /// Size of the frames the detector saw, in pixels, to place the normalized boxes over the
    /// aspect-filled preview.
    let frameSize: CGSize
    let highlight: UInt32?

    private static let amber = Color(red: 240 / 255, green: 190 / 255, blue: 60 / 255)
    private static let green = Color(red: 90 / 255, green: 200 / 255, blue: 90 / 255)
    private static let red = Color(red: 230 / 255, green: 80 / 255, blue: 80 / 255)
    private static let blue = Color(red: 90 / 255, green: 160 / 255, blue: 255 / 255)

    /// MediaPipe's hand skeleton over the 21 joints in `UnHand.joints` order.
    private static let bones: [(Int, Int)] = [
        (0, 1), (1, 2), (2, 3), (3, 4),
        (0, 5), (5, 6), (6, 7), (7, 8),
        (5, 9), (9, 10), (10, 11), (11, 12),
        (9, 13), (13, 14), (14, 15), (15, 16),
        (13, 17), (17, 18), (18, 19), (19, 20),
        (0, 17),
    ]

    var body: some View {
        Canvas { context, size in
            guard let table, frameSize.width > 0, frameSize.height > 0 else { return }
            // The preview fills the view keeping the frame's aspect (`resizeAspectFill`), so the
            // frame is scaled by the larger ratio and centred, overflowing on one axis.
            let scale = max(size.width / frameSize.width, size.height / frameSize.height)
            let drawn = CGSize(width: frameSize.width * scale, height: frameSize.height * scale)
            let origin = CGPoint(x: (size.width - drawn.width) / 2, y: (size.height - drawn.height) / 2)
            func point(_ x: Float, _ y: Float) -> CGPoint {
                CGPoint(x: origin.x + CGFloat(x) * drawn.width, y: origin.y + CGFloat(y) * drawn.height)
            }
            func rect(_ b: UnBox) -> CGRect {
                let a = point(b.x1, b.y1), z = point(b.x2, b.y2)
                return CGRect(x: a.x, y: a.y, width: z.x - a.x, height: z.y - a.y)
            }

            let color = table.handsPresent ? Self.amber : table.isStable ? Self.green : Color.white
            for d in table.detections {
                let r = rect(d.bbox)
                context.stroke(Path(roundedRect: r, cornerRadius: 3), with: .color(color), lineWidth: 2)
                if let label = d.label {
                    context.draw(Text(label).font(.system(size: 13)).foregroundStyle(color), at: CGPoint(x: r.minX + 4, y: r.minY - 9), anchor: .leading)
                }
                if d.number > 0 {
                    if table.pointedAt == d.number {
                        // The object a finger is on.
                        let radius = max(r.width, r.height) * 0.7
                        let ring = CGRect(x: r.midX - radius, y: r.midY - radius, width: 2 * radius, height: 2 * radius)
                        context.stroke(Path(ellipseIn: ring), with: .color(Self.blue), lineWidth: 3)
                    }
                    let lit = highlight.map { d.number <= $0 } ?? false
                    let radius: CGFloat = lit ? 22 : 16
                    let center = CGPoint(x: r.midX, y: r.midY)
                    let disc = CGRect(x: center.x - radius, y: center.y - radius, width: 2 * radius, height: 2 * radius)
                    context.fill(Path(ellipseIn: disc), with: .color(lit ? Self.red : Color.black.opacity(0.6)))
                    context.draw(Text("\(d.number)").font(.system(size: lit ? 26 : 18, weight: .semibold)).foregroundStyle(.white), at: center)
                }
            }

            for d in table.fingers {
                let r = rect(d.bbox)
                context.stroke(Path(roundedRect: r, cornerRadius: 3), with: .color(Self.blue), lineWidth: 2)
                let text = "\(d.label ?? "finger") \(Int((d.confidence * 100).rounded()))%"
                context.draw(Text(text).font(.system(size: 13)).foregroundStyle(Self.blue), at: CGPoint(x: r.minX + 4, y: r.minY - 9), anchor: .leading)
            }

            for hand in table.hands {
                let joints = stride(from: 0, to: hand.joints.count - 1, by: 2).map { i -> CGPoint? in
                    let x = hand.joints[i], y = hand.joints[i + 1]
                    return x.isNaN || y.isNaN ? nil : point(x, y)
                }
                for (a, b) in Self.bones where a < joints.count && b < joints.count {
                    if let pa = joints[a], let pb = joints[b] {
                        var path = Path()
                        path.move(to: pa)
                        path.addLine(to: pb)
                        context.stroke(path, with: .color(Self.blue), lineWidth: 2)
                    }
                }
                for case let p? in joints {
                    context.fill(Path(ellipseIn: CGRect(x: p.x - 3, y: p.y - 3, width: 6, height: 6)), with: .color(.white))
                }
                if let wrist = joints.first ?? nil {
                    let side = hand.chirality == .left ? "Left" : hand.chirality == .right ? "Right" : "Hand"
                    context.draw(Text("\(side) \(hand.raisedFingers)").font(.system(size: 15)).foregroundStyle(Self.blue), at: CGPoint(x: wrist.x, y: wrist.y + 14))
                }
            }
        }
        .allowsHitTesting(false)
    }
}
