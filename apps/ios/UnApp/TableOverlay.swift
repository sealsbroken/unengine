import SwiftUI

/// What the tutor sees, drawn over the camera picture as on the desktop: a box around every
/// recognised object (green once the table is settled, white while it is still changing) and
/// the objects' stable numbers, lit red up to the number the tutor is counting out.
struct TableOverlay: View {
    let table: UnTableState?
    /// Size of the frames the detector saw, in pixels, to place the normalized boxes over the
    /// aspect-filled preview.
    let frameSize: CGSize
    let highlight: UInt32?

    private static let green = Color(red: 90 / 255, green: 200 / 255, blue: 90 / 255)
    private static let red = Color(red: 230 / 255, green: 80 / 255, blue: 80 / 255)

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

            let color = table.isStable ? Self.green : Color.white
            for d in table.detections {
                let r = rect(d.bbox)
                context.stroke(Path(roundedRect: r, cornerRadius: 3), with: .color(color), lineWidth: 2)
                if let label = d.label {
                    context.draw(Text(label).font(.system(size: 13)).foregroundStyle(color), at: CGPoint(x: r.minX + 4, y: r.minY - 9), anchor: .leading)
                }
                if d.number > 0 {
                    let lit = highlight.map { d.number <= $0 } ?? false
                    let radius: CGFloat = lit ? 22 : 16
                    let center = CGPoint(x: r.midX, y: r.midY)
                    let disc = CGRect(x: center.x - radius, y: center.y - radius, width: 2 * radius, height: 2 * radius)
                    context.fill(Path(ellipseIn: disc), with: .color(lit ? Self.red : Color.black.opacity(0.6)))
                    context.draw(Text("\(d.number)").font(.system(size: lit ? 26 : 18, weight: .semibold)).foregroundStyle(.white), at: center)
                }
            }
        }
        .allowsHitTesting(false)
    }
}
