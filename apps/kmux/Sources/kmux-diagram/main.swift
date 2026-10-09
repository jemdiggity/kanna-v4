// kmux-diagram FILE.mmd... --out DIR [--dark] [--scale N] [--json]
// Lays out and draws each Mermaid file natively, writing DIR/NAME.png (or
// NAME.error), and prints per-file timings: warm layout and draw, in ms.
import CoreGraphics
import Foundation
import KmuxDiagram

var files: [String] = []
var out = "."
var dark = false
var scale: CGFloat = 2
var arguments = CommandLine.arguments.dropFirst()
while let argument = arguments.popFirst() {
    switch argument {
    case "--out": out = arguments.popFirst() ?? out
    case "--dark": dark = true
    case "--scale": scale = CGFloat(Double(arguments.popFirst() ?? "") ?? 2)
    default: files.append(argument)
    }
}
try FileManager.default.createDirectory(atPath: out, withIntermediateDirectories: true)
var report: [String: Any] = [:]
for file in files {
    let name = ((file as NSString).lastPathComponent as NSString).deletingPathExtension
    let source = try String(contentsOfFile: file, encoding: .utf8)
    do {
        _ = try Diagram.layout(source: source) // warm-up
        let start = DispatchTime.now()
        let layout = try Diagram.layout(source: source)
        let laidOut = DispatchTime.now()
        guard let scene = layout.scene else { throw DiagramError(message: "Unsupported diagram type: \(layout.type)") }
        guard let png = scene.png(scale: scale, dark: dark) else { throw DiagramError(message: "could not draw") }
        let drawn = DispatchTime.now()
        try png.write(to: URL(fileURLWithPath: out).appendingPathComponent(name + ".png"))
        let ms = { (a: DispatchTime, b: DispatchTime) in Double(b.uptimeNanoseconds - a.uptimeNanoseconds) / 1e6 }
        // Labels that had to shrink to fit their box (should be none: Core Text measured them).
        let probe = CGContext(data: nil, width: 1, height: 1, bitsPerComponent: 8, bytesPerRow: 0, space: CGColorSpaceCreateDeviceRGB(),
                              bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)
        let shrunk = probe.map { scene.draw(in: $0, dark: false) } ?? 0
        // Drawing alone (what a pane pays per frame), into a 2x bitmap, no PNG encoding.
        let bitmap = CGContext(data: nil, width: Int(scene.size.width * scale), height: Int(scene.size.height * scale), bitsPerComponent: 8,
                               bytesPerRow: 0, space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)
        bitmap?.scaleBy(x: scale, y: scale)
        let drawStart = DispatchTime.now()
        if let bitmap { scene.draw(in: bitmap, dark: false) }
        let drawOnly = Double(DispatchTime.now().uptimeNanoseconds - drawStart.uptimeNanoseconds) / 1e6
        report[name] = ["shrunk": shrunk, "draw_only_ms": drawOnly, "layout_ms": ms(start, laidOut), "draw_ms": ms(laidOut, drawn), "width": scene.size.width, "height": scene.size.height,
                        "labels": scene.texts]
    } catch {
        try "\(error)".write(toFile: (out as NSString).appendingPathComponent(name + ".error"), atomically: true, encoding: .utf8)
        report[name] = ["error": "\(error)"]
    }
}
let data = try JSONSerialization.data(withJSONObject: report, options: [.prettyPrinted, .sortedKeys])
try data.write(to: URL(fileURLWithPath: out).appendingPathComponent("report.json"))
print("kmux-diagram: \(report.values.filter { ($0 as? [String: Any])?["error"] == nil }.count)/\(files.count) drawn into \(out)")
