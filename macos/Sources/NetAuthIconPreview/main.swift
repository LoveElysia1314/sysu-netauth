import AppKit
import NetAuthCore
import NetAuthArtwork

// Windows Vista+ supports PNG-compressed entries in ICO resources. Each size
// is independently rendered from vector geometry, never resized from a bitmap.
if CommandLine.arguments.contains("--export-windows") {
    let directory = URL(fileURLWithPath: "assets/icons/generated/windows", isDirectory: true)
    try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
    let sizes = [16,20,24,32,40,48,64,128,256]
    func little<T: FixedWidthInteger>(_ value: T) -> Data {
        var v = value.littleEndian
        return withUnsafeBytes(of: &v) { Data($0) }
    }
    for name in ["app"] + StatusMark.allCases.map(\.rawValue) {
        let state = name == "app" ? StatusMark.ready : StatusMark(rawValue: name)!
        var pngs: [Data] = []
        for size in sizes {
            let bitmap = NSBitmapImageRep(bitmapDataPlanes: nil,pixelsWide: size,pixelsHigh: size,
                bitsPerSample: 8,samplesPerPixel: 4,hasAlpha: true,isPlanar: false,
                colorSpaceName: .deviceRGB,bytesPerRow: 0,bitsPerPixel: 0)!
            NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: bitmap)
            NSColor.clear.setFill(); NSRect(x: 0,y: 0,width: size,height: size).fill(using: .copy)
            StatusIcon.image(state,colored: true,windows: true,appIcon: name == "app")
                .draw(in: NSRect(x: 0,y: 0,width: size,height: size))
            NSGraphicsContext.current = nil
            pngs.append(bitmap.representation(using: .png,properties: [:])!)
        }
        var ico = little(UInt16(0)) + little(UInt16(1)) + little(UInt16(sizes.count))
        var offset = 6 + sizes.count * 16
        for (size,png) in zip(sizes,pngs) {
            ico.append(contentsOf: [UInt8(size % 256),UInt8(size % 256),0,0])
            ico.append(little(UInt16(1))); ico.append(little(UInt16(32)))
            ico.append(little(UInt32(png.count))); ico.append(little(UInt32(offset)))
            offset += png.count
        }
        for png in pngs { ico.append(png) }
        try ico.write(to: directory.appendingPathComponent(name + ".ico"))
    }
    try Data((StatusIcon.sourceSHA256 + "\n").utf8).write(to: directory.appendingPathComponent("source.sha256"))
    print("Exported 7 Windows ICO files with 9 vector-rendered sizes each")
    exit(0)
}

let width = 1100, height = 800
let bitmap = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: width, pixelsHigh: height,
                             bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true,
                             isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: bitmap)
NSColor.white.setFill(); NSRect(x: 0, y: 0, width: width, height: height).fill()
func text(_ string: String, x: CGFloat, y: CGFloat, size: CGFloat, color: NSColor) {
    string.draw(at: NSPoint(x: x,y: y), withAttributes: [.font: NSFont.systemFont(ofSize: size), .foregroundColor: color])
}
text("SYSU NetAuth · macOS / Windows 共享图标", x: 24,y: 760,size: 22,color: .black)
text("实际尺寸 18 pt + 放大预览 · 单色默认，彩色沿用 Windows 语义", x: 24,y: 732,size: 13,color: .darkGray)
let rows = [("Mac 单色 · 浅色",false,false,false,false), ("Mac 单色 · 深色",false,true,false,false),
            ("Mac 彩色 · 浅色",true,false,false,false), ("Mac 彩色 · 深色",true,true,false,false),
            ("Mac 菜单选中",true,true,true,false), ("Windows · 浅色",true,false,false,true),
            ("Windows · 深色",true,true,false,true)]
for (index, row) in rows.enumerated() {
    let y = CGFloat(625 - index * 96)
    let appearance = NSAppearance(named: row.2 ? .darkAqua : .aqua)!
    let background = row.3 ? NSColor(calibratedRed: 0.12,green: 0.38,blue: 0.78,alpha: 1) : row.2 ? NSColor(calibratedWhite: 0.12,alpha: 1) : NSColor(calibratedWhite: 0.96,alpha: 1)
    background.setFill(); NSRect(x: 12,y: y,width: 1076,height: 90).fill()
    let foreground: NSColor = row.2 ? .white : .black
    text(row.0,x: 24,y: y + 34,size: 14,color: foreground)
    for (column, mark) in StatusMark.allCases.enumerated() {
        let x = CGFloat(170 + column * 150)
        appearance.performAsCurrentDrawingAppearance {
            var icon = StatusIcon.image(mark,colored: row.1,highlighted: row.3,windows: row.4)
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
