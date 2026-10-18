import SwiftUI

/// What the tutor sees, over the camera picture: a box around every toy and its stable number
/// badge. Overlays use a white core with a dark halo, so they stay readable on wood, a white
/// table or a patterned cloth. A badge is white until counted, yellow (and larger) while the
/// tutor counts it, blue once counted, green when the count was right.
struct TableOverlay: View {
    let table: UnTableState?
    /// Size of the frames the detector saw, in pixels, to place the normalized boxes over the
    /// aspect-filled preview.
    let frameSize: CGSize
    let highlight: UInt32?
    /// The count was right: every badge turns green.
    var correct = false
    var copy = Copy(ru: false)

    enum BadgeState { case idle, lit, counted, correct }

    var body: some View {
        GeometryReader { geo in
            let size = geo.size
            if let table, frameSize.width > 0, frameSize.height > 0 {
                // The preview fills the view keeping the frame's aspect (`resizeAspectFill`), so the
                // frame is scaled by the larger ratio and centred, overflowing on one axis.
                let scale = max(size.width / frameSize.width, size.height / frameSize.height)
                let drawn = CGSize(width: frameSize.width * scale, height: frameSize.height * scale)
                let origin = CGPoint(x: (size.width - drawn.width) / 2, y: (size.height - drawn.height) / 2)
                let badge = min(72, max(44, size.width * 0.095))
                ForEach(Array(table.detections.enumerated()), id: \.offset) { _, d in
                    let rect = CGRect(x: origin.x + CGFloat(d.bbox.x1) * drawn.width, y: origin.y + CGFloat(d.bbox.y1) * drawn.height,
                                      width: CGFloat(d.bbox.x2 - d.bbox.x1) * drawn.width,
                                      height: CGFloat(d.bbox.y2 - d.bbox.y1) * drawn.height)
                    let state = badgeState(d.number)
                    ToyBox(rect: rect, state: state)
                    if d.number > 0 {
                        NumberBadge(number: d.number, state: state, size: badge)
                            .position(x: rect.minX + 6, y: rect.minY + 6)
                            .accessibilityLabel(copy("lesson.toy", ["n": "\(d.number)", "state": stateName(state)]))
                    }
                }
                .animation(.spring(response: 0.35, dampingFraction: 0.6), value: highlight)
            }
        }
        .allowsHitTesting(false)
    }

    private func badgeState(_ number: UInt32) -> BadgeState {
        if correct { return .correct }
        guard let highlight, number > 0 else { return .idle }
        if number < highlight { return .counted }
        if number == highlight { return .lit }
        return .idle
    }

    private func stateName(_ state: BadgeState) -> String {
        switch state {
        case .idle: return copy("lesson.idle")
        case .lit: return copy("lesson.lit")
        case .counted: return copy("lesson.counted")
        case .correct: return copy("lesson.done")
        }
    }
}

/// A toy's box: a white (or lit / correct) stroke with a dark halo inside and out.
struct ToyBox: View {
    let rect: CGRect
    let state: TableOverlay.BadgeState

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: min(22, rect.width / 5), style: .continuous)
        let stroke: Color = state == .lit ? Un.badgeLit : state == .correct ? Un.correctStroke : .white
        let width: CGFloat = state == .lit || state == .correct ? 5 : 3
        ZStack {
            shape.stroke(Un.halo, lineWidth: width + 3)
            shape.stroke(stroke, lineWidth: width)
        }
        .frame(width: rect.width, height: rect.height)
        .position(x: rect.midX, y: rect.midY)
    }
}

/// The stable number of a toy, in heavy rounded numerals sized to the screen.
struct NumberBadge: View {
    let number: UInt32
    let state: TableOverlay.BadgeState
    let size: CGFloat

    var body: some View {
        let (bg, fg): (Color, Color) = {
            switch state {
            case .idle: return (Un.badgeIdle, Color(hex: 0x0A0A0C))
            case .lit: return (Un.badgeLit, .black)
            case .counted: return (Un.badgeCounted, .white)
            case .correct: return (Un.badgeCorrect, .white)
            }
        }()
        Text("\(number)")
            .font(Un.rounded(size * 0.56))
            .foregroundStyle(fg)
            .frame(width: size, height: size)
            .background(bg, in: Circle())
            .overlay(Circle().stroke(state == .idle || state == .counted ? Color.white : bg, lineWidth: 4))
            .overlay(Circle().stroke(Un.halo, lineWidth: 1.5).padding(-2))
            .background(Circle().fill(state == .lit ? Un.badgeLit.opacity(0.45) : state == .correct
                ? Un.correctStroke.opacity(0.35) : .clear).padding(state == .lit ? -12 : -10))
            .shadow(color: .black.opacity(0.3), radius: 8, y: 4)
            .scaleEffect(state == .lit ? 1.25 : state == .correct ? 1.1 : 1)
    }
}
