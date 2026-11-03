import AppKit
import NetAuthCore

/// Vector artwork tuned for an 18pt menu-bar slot. The shield keeps product identity;
/// distinct centre marks remain readable without colour or animation.
public enum StatusIcon {
    public static func image(_ state: StatusMark, colored: Bool, highlighted: Bool = false) -> NSImage {
        let image = NSImage(size: NSSize(width: 18, height: 18), flipped: false) { _ in
            let outline: NSColor = !colored ? .black : highlighted ? .selectedMenuItemTextColor : .labelColor
            let accent: NSColor
            if !colored || highlighted { accent = outline }
            else {
                switch state {
                case .ready: accent = .systemBlue
                case .authenticating: accent = .systemOrange
                case .authenticated: accent = .systemGreen
                case .failed: accent = .systemRed
                case .disconnected, .unavailable: accent = .secondaryLabelColor
                }
            }
            func stroke(_ points: [NSPoint], color: NSColor, width: CGFloat = 1.5) {
                let path = NSBezierPath()
                path.lineWidth = width
                path.lineCapStyle = .round
                path.lineJoinStyle = .round
                path.move(to: points[0])
                for point in points.dropFirst() { path.line(to: point) }
                color.setStroke(); path.stroke()
            }
            let shield = NSBezierPath()
            shield.move(to: NSPoint(x: 9, y: 16.5))
            shield.curve(to: NSPoint(x: 16, y: 14), controlPoint1: NSPoint(x: 11.5, y: 15), controlPoint2: NSPoint(x: 13, y: 14.2))
            shield.line(to: NSPoint(x: 16, y: 9))
            shield.curve(to: NSPoint(x: 9, y: 1.5), controlPoint1: NSPoint(x: 16, y: 5.8), controlPoint2: NSPoint(x: 13.5, y: 3))
            shield.curve(to: NSPoint(x: 2, y: 9), controlPoint1: NSPoint(x: 4.5, y: 3), controlPoint2: NSPoint(x: 2, y: 5.8))
            shield.line(to: NSPoint(x: 2, y: 14))
            shield.curve(to: NSPoint(x: 9, y: 16.5), controlPoint1: NSPoint(x: 5, y: 14.2), controlPoint2: NSPoint(x: 6.5, y: 15))
            shield.close(); shield.lineWidth = 1.3
            shield.lineJoinStyle = .round
            outline.setStroke(); shield.stroke()
            func p(_ x: CGFloat, _ y: CGFloat) -> NSPoint { NSPoint(x: x, y: y) }
            switch state {
            case .ready:
                stroke([p(5.4,11.9),p(12.6,11.9),p(12.6,7.8),p(11,7.8),p(10,6.6),p(8,6.6),p(7,7.8),p(5.4,7.8),p(5.4,11.9)], color: accent, width: 1.2)
                for x: CGFloat in [7,9,11] { stroke([p(x,11.7),p(x,10.2)], color: accent, width: 1) }
            case .authenticating:
                stroke([p(5.5,10.8),p(12.2,10.8),p(10.5,12.5)], color: accent)
                stroke([p(12.5,7.5),p(5.8,7.5),p(7.5,5.8)], color: accent)
            case .authenticated:
                stroke([p(5.5,9.1),p(8,6.6),p(12.6,11.4)], color: accent, width: 1.9)
            case .failed:
                stroke([p(6.2,6.8),p(11.8,12.1)], color: accent, width: 1.8)
                stroke([p(6.2,12.1),p(11.8,6.8)], color: accent, width: 1.8)
            case .disconnected:
                stroke([p(5.5,9.5),p(12.5,9.5)], color: accent, width: 1.9)
            case .unavailable:
                stroke([p(9,12.5),p(9,9)], color: accent, width: 1.8)
                accent.setFill(); NSBezierPath(ovalIn: NSRect(x: 8.1,y: 5.8,width: 1.8,height: 1.8)).fill()
            }
            return true
        }
        image.isTemplate = !colored
        image.accessibilityDescription = "SYSU NetAuth · \(state.label)"
        return image
    }
}
