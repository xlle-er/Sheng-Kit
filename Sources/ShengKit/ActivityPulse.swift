import SwiftUI

/// Connection activity, never presented as an audio level meter.
struct ActivityPulse: View {
    let active: Bool
    let style: String
    let animation: String
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    var body: some View {
        Group {
            if active && animation != "off" && !reduceMotion {
                MovingPulse(ribbon: style == "ribbon")
            } else {
                Circle().fill(active ? ShengStyle.accent : Color.secondary).frame(width: 6, height: 6)
            }
        }.frame(width: 16, height: 16).accessibilityHidden(true)
    }
}
private struct MovingPulse: View {
    let ribbon: Bool
    @State private var expanded = false
    var body: some View {
        Group {
            if ribbon {
                HStack(spacing: 2) {
                    ForEach(0..<3) { index in
                        Capsule().fill(ShengStyle.accent).frame(width: 2, height: expanded ? 5 + CGFloat(index) * 3 : 4)
                            .animation(.easeInOut(duration: 0.9).repeatForever(autoreverses: true).delay(Double(index) * 0.12), value: expanded)
                    }
                }
            } else {
                ZStack {
                    Circle().strokeBorder(ShengStyle.accent.opacity(expanded ? 0.1 : 0.6)).frame(width: 13, height: 13).scaleEffect(expanded ? 1 : 0.55)
                    Circle().fill(ShengStyle.accent).frame(width: 5, height: 5)
                }.animation(.easeInOut(duration: 1.1).repeatForever(autoreverses: true), value: expanded)
            }
        }.onAppear { expanded = true }
    }
}
