// Renders every corpus diagram with BeautifulMermaid to NAME.png (or NAME.error),
// timing a warm render of each: bm-cli CORPUS_DIR OUT_DIR
import BeautifulMermaid
import Foundation

let corpus = URL(fileURLWithPath: CommandLine.arguments[1])
let out = URL(fileURLWithPath: CommandLine.arguments[2])
try FileManager.default.createDirectory(at: out, withIntermediateDirectories: true)
let renderer = MermaidImageRenderer()
renderer.scale = 2
var times: [String: Double] = [:]
var drawTimes: [String: Double] = [:]
let files = try FileManager.default.contentsOfDirectory(at: corpus, includingPropertiesForKeys: nil)
    .filter { $0.pathExtension == "mmd" }.sorted { $0.path < $1.path }
for file in files {
    let name = file.deletingPathExtension().lastPathComponent
    let source = try String(contentsOf: file, encoding: .utf8)
    do {
        _ = try renderer.renderPNG(from: source)
        let start = Date()
        guard let png = try renderer.renderPNG(from: source) else { throw NSError(domain: "bm-cli", code: 1, userInfo: [NSLocalizedDescriptionKey: "renderPNG returned nil"]) }
        times[name] = (Date().timeIntervalSince(start) * 100_000).rounded() / 100
        try png.write(to: out.appendingPathComponent(name + ".png"))
        // Layout + Core Graphics drawing only (no PNG encoding): what a native view would pay.
        let drawStart = Date()
        _ = try renderer.renderImage(from: source)
        drawTimes[name] = (Date().timeIntervalSince(drawStart) * 100_000).rounded() / 100
        try renderer.renderSVG(from: source).write(to: out.appendingPathComponent(name + ".svg"), atomically: true, encoding: .utf8)
    } catch {
        try "\(error)".write(to: out.appendingPathComponent(name + ".error"), atomically: true, encoding: .utf8)
    }
}
try JSONSerialization.data(withJSONObject: drawTimes, options: [.prettyPrinted, .sortedKeys]).write(to: out.appendingPathComponent("draw-timings.json"))
try JSONSerialization.data(withJSONObject: times, options: [.prettyPrinted, .sortedKeys]).write(to: out.appendingPathComponent("timings.json"))
print("BeautifulMermaid: \(times.count)/\(files.count) rendered")
