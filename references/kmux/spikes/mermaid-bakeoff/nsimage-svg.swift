// Can macOS draw merman's SVG natively? Loads each SVG with NSImage (no web
// view) and writes it as PNG at 2x: swift nsimage-svg.swift IN_DIR OUT_DIR
import AppKit

let input = URL(fileURLWithPath: CommandLine.arguments[1])
let output = URL(fileURLWithPath: CommandLine.arguments[2])
try FileManager.default.createDirectory(at: output, withIntermediateDirectories: true)
var ok = 0, failed: [String] = [], times: [Double] = []
for file in try FileManager.default.contentsOfDirectory(at: input, includingPropertiesForKeys: nil) where file.pathExtension == "svg" {
    let start = Date()
    guard let data = try? Data(contentsOf: file), let image = NSImage(data: data), image.size.width > 0 else {
        failed.append(file.lastPathComponent); continue
    }
    let size = NSSize(width: image.size.width * 2, height: image.size.height * 2)
    let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: Int(size.width), pixelsHigh: Int(size.height), bitsPerSample: 8,
                               samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
    NSColor.white.setFill(); NSRect(origin: .zero, size: size).fill()
    image.draw(in: NSRect(origin: .zero, size: size))
    NSGraphicsContext.restoreGraphicsState()
    times.append(Date().timeIntervalSince(start) * 1000)
    try rep.representation(using: .png, properties: [:])!.write(to: output.appendingPathComponent(file.deletingPathExtension().lastPathComponent + ".png"))
    ok += 1
}
times.sort()
print("NSImage SVG: \(ok) drawn, \(failed.count) failed \(failed.prefix(5)); median \(times.isEmpty ? 0 : times[times.count / 2]) ms")
