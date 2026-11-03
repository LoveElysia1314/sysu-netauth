import AppKit
import NetAuthCore
import NetAuthArtwork

let width = 1100, height = 600
let bitmap = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: width, pixelsHigh: height,
                             bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true,
                             isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: bitmap)
NSColor.white.setFill(); NSRect(x: 0, y: 0, width: width, height: height).fill()
func text(_ string: String, x: CGFloat, y: CGFloat, size: CGFloat, color: NSColor) {
    string.draw(at: NSPoint(x: x,y: y), withAttributes: [.font: NSFont.systemFont(ofSize: size), .foregroundColor: color])
}
text("SYSU NetAuth · 菜单栏状态图标", x: 24,y: 560,size: 22,color: .black)
text("实际尺寸 18 pt + 放大预览 · 单色默认，彩色沿用 Windows 语义", x: 24,y: 532,size: 13,color: .darkGray)
let rows = [("单色 · 浅色",false,false,false), ("单色 · 深色",false,true,false),
            ("彩色 · 浅色",true,false,false), ("彩色 · 深色",true,true,false), ("菜单选中",true,true,true)]
for (index, row) in rows.enumerated() {
    let y = CGFloat(425 - index * 96)
    let appearance = NSAppearance(named: row.2 ? .darkAqua : .aqua)!
    let background = row.3 ? NSColor(calibratedRed: 0.12,green: 0.38,blue: 0.78,alpha: 1) : row.2 ? NSColor(calibratedWhite: 0.12,alpha: 1) : NSColor(calibratedWhite: 0.96,alpha: 1)
    background.setFill(); NSRect(x: 12,y: y,width: 1076,height: 90).fill()
    let foreground: NSColor = row.2 ? .white : .black
    text(row.0,x: 24,y: y + 34,size: 14,color: foreground)
    for (column, mark) in StatusMark.allCases.enumerated() {
        let x = CGFloat(170 + column * 150)
        appearance.performAsCurrentDrawingAppearance {
            var icon = StatusIcon.image(mark,colored: row.1,highlighted: row.3)
            if !row.1 && row.2 {
                let source = icon
                icon = NSImage(size: icon.size, flipped: false) { rect in
                    source.draw(in: rect)
                    NSColor.white.setFill(); rect.fill(using: .sourceAtop)
                    return true
                }
            }
            icon.draw(in: NSRect(x: x,y: y + 40,width: 18,height: 18))
            icon.draw(in: NSRect(x: x + 30,y: y + 26,width: 54,height: 54))
        }
        text(mark.label,x: x,y: y + 6,size: 12,color: foreground)
    }
}
NSGraphicsContext.current = nil
let destination = URL(fileURLWithPath: CommandLine.arguments.dropFirst().first ?? "dist/menu-bar-icons.png")
try FileManager.default.createDirectory(at: destination.deletingLastPathComponent(),withIntermediateDirectories: true)
try bitmap.representation(using: .png,properties: [:])!.write(to: destination)
print(destination.path)
