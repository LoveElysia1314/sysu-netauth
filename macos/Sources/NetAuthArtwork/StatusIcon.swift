import AppKit
import NetAuthCore

/// Both platforms render the same generated geometry and status palette.
/// Source of truth: assets/icons/source.json; never hand-edit generated Swift/SVG.
public enum StatusIcon {
    public static let sourceSHA256 = GeneratedIconSource.sourceSHA256
    private struct Path: Decodable {
        let width: Double
        let commands: [[Double]]
        let fill: Bool?
    }
    private struct Mark: Decodable { let color: String; let paths: [Path] }
    private struct Document: Decodable {
        let size: Double
        let palette: [String: String]
        let windows_outline: String
        let shield: Path
        let states: [String: Mark]
    }
    private static let source = try! JSONDecoder().decode(Document.self, from: Data(GeneratedIconSource.json.utf8))
    private static func color(_ hex: String) -> NSColor {
        let value = UInt32(hex, radix: 16)!
        return NSColor(srgbRed: CGFloat((value >> 16) & 255)/255,
                       green: CGFloat((value >> 8) & 255)/255, blue: CGFloat(value & 255)/255, alpha: 1)
    }
    private static func draw(_ definition: Path, color: NSColor) {
        let path = NSBezierPath()
        for command in definition.commands {
            switch Int(command[0]) {
            case 0: path.move(to: NSPoint(x: command[1],y: command[2]))
            case 1: path.line(to: NSPoint(x: command[1],y: command[2]))
            case 2: path.curve(to: NSPoint(x: command[5],y: command[6]),
                               controlPoint1: NSPoint(x: command[1],y: command[2]),
                               controlPoint2: NSPoint(x: command[3],y: command[4]))
            case 3: path.close()
            case 4: path.appendOval(in: NSRect(x: command[1],y: command[2],width: command[3],height: command[4]))
            default: preconditionFailure("Invalid generated path command")
            }
        }
        if definition.fill == true { color.setFill(); path.fill() }
        else {
            path.lineWidth = definition.width
            path.lineCapStyle = .round; path.lineJoinStyle = .round
            color.setStroke(); path.stroke()
        }
    }
    public static func image(_ state: StatusMark, colored: Bool, highlighted: Bool = false,
                             windows: Bool = false, appIcon: Bool = false, monochrome: NSColor? = nil) -> NSImage {
        let image = NSImage(size: NSSize(width: source.size,height: source.size), flipped: false) { _ in
            let mark = source.states[state.rawValue]!
            let outline: NSColor = monochrome ?? (windows ? color(source.windows_outline)
                : !colored ? .black : highlighted ? .selectedMenuItemTextColor : .labelColor)
            let accent = monochrome != nil || !colored || highlighted ? outline : color(source.palette[appIcon ? "green" : mark.color]!)
            draw(source.shield, color: outline)
            for path in mark.paths { draw(path, color: accent) }
            return true
        }
        image.isTemplate = !colored
        image.accessibilityDescription = "SYSU NetAuth · \(state.label)"
        return image
    }
}
