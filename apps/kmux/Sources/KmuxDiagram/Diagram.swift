import AppKit
import KmuxMerman

/// Mermaid diagrams laid out by merman (no web view, no JavaScript) and drawn
/// natively by `DiagramView`.
public enum Diagram {
    /// Lays out `source` (flowchart, sequence or state diagrams; other types
    /// come back `.unsupported`). Throws `DiagramError` for invalid source.
    public static func layout(source: String) throws -> DiagramLayout {
        let log = MeasureLog()
        let json = try layoutJSON(source: source, log: log)
        guard let meta = json["meta"] as? [String: Any], let semantic = json["semantic"] as? [String: Any],
              let layout = json["layout"] as? [String: Any] else { throw DiagramError(message: "merman returned no layout") }
        let type = meta["diagram_type"] as? String ?? "unknown"
        let scene: DiagramScene?
        switch type {
        case "flowchart-v2", "flowchart": scene = Flowchart.scene(semantic: semantic, layout: layout["FlowchartV2"] as? [String: Any] ?? [:])
        case "sequence": scene = Sequence.scene(semantic: semantic, layout: layout["SequenceDiagram"] as? [String: Any] ?? [:])
        case "stateDiagram", "state": scene = State.scene(semantic: semantic, layout: layout["StateDiagramV2"] as? [String: Any] ?? [:])
        default: scene = nil
        }
        return DiagramLayout(type: type, scene: scene.map { scene in
            var scene = log.apply(to: scene)
            scene.fit()
            return scene
        })
    }

    /// merman's raw layout JSON ({meta, semantic, layout}), text measured with Core Text.
    static func layoutJSON(source: String, log: MeasureLog = MeasureLog()) throws -> [String: Any] {
        let bytes = Array(source.utf8)
        let context = Unmanaged.passUnretained(log).toOpaque()
        guard let raw = withExtendedLifetime(log, {
            bytes.withUnsafeBufferPointer { buffer in
                buffer.withMemoryRebound(to: CChar.self) { kmux_merman_layout($0.baseAddress, $0.count, measure, context) }
            }
        }) else { throw DiagramError(message: "merman returned nothing") }
        defer { kmux_merman_free(raw) }
        guard let object = try? JSONSerialization.jsonObject(with: Data(bytes: raw, count: strlen(raw))) as? [String: Any] else {
            throw DiagramError(message: "merman returned invalid JSON")
        }
        if let error = object["error"] as? String {
            // merman is built with only the types kmux draws; others aren't errors in the source.
            if let match = error.firstMatch(of: /Unsupported diagram type: ([^;]+)/) {
                return ["meta": ["diagram_type": String(match.1)], "semantic": [String: Any](), "layout": [String: Any]()]
            }
            throw DiagramError(message: error)
        }
        return object
    }
}

/// The font size and kind (HTML or SVG text) merman measured each text at,
/// so every label is drawn exactly as it was measured.
final class MeasureLog {
    private var styles: [String: (size: CGFloat, htmlLike: Bool)] = [:]

    func record(_ text: String, size: CGFloat, htmlLike: Bool) {
        let key = text.trimmingCharacters(in: .whitespaces)
        if styles[key] == nil { styles[key] = (size, htmlLike) }
    }

    func apply(to scene: DiagramScene) -> DiagramScene {
        var scene = scene
        scene.items = scene.items.map { item in
            guard case .text(var label) = item, !label.fixedSize else { return item }
            let lines = LabelText.plainLines(label.text)
            let key = [label.text.trimmingCharacters(in: .whitespaces), lines.joined(separator: " ")] + lines
            if let style = key.lazy.compactMap({ self.styles[$0] }).first {
                label.size = style.size
                label.htmlLike = style.htmlLike
            }
            return .text(label)
        }
        return scene
    }
}

public struct DiagramLayout {
    /// merman's name for the diagram type, e.g. "flowchart-v2".
    public let type: String
    /// What to draw; nil for diagram types kmux doesn't draw yet.
    public let scene: DiagramScene?
}

public struct DiagramError: Error, CustomStringConvertible {
    public let message: String
    public var description: String { message }
    public init(message: String) { self.message = message }
}

/// Core Text measurements for merman (see kmux_merman.h). Fonts come from the
/// request's size, weight and style; family only picks monospaced or not,
/// since kmux draws every label in the system font.
private let measure: kmux_merman_measure_fn = { request, out, context in
    guard let request = request?.pointee, let out else { return 0 }
    func string(_ pointer: UnsafePointer<CChar>?, _ length: Int) -> String {
        guard let pointer, length > 0 else { return "" }
        return pointer.withMemoryRebound(to: UInt8.self, capacity: length) { String(decoding: UnsafeBufferPointer(start: $0, count: length), as: UTF8.self) }
    }
    let text = string(request.text, request.text_len)
    let family = string(request.font_family, request.font_family_len).lowercased()
    let size = CGFloat(request.font_size > 0 ? request.font_size : 16)
    let htmlLike = request.html_like != 0
    let monospaced = family.contains("mono") || family.contains("courier")
    let maxWidth = request.max_width > 0 ? CGFloat(request.max_width) : nil
    if let context, Int(request.kind) == Int(KMUX_MEASURE_METRICS) {
        Unmanaged<MeasureLog>.fromOpaque(context).takeUnretainedValue().record(text, size: size, htmlLike: htmlLike)
    }
    let measured = LabelText.measure(text, size: size, bold: request.bold != 0, italic: request.italic != 0, monospaced: monospaced,
                                     maxWidth: maxWidth, htmlLike: htmlLike)
    switch Int(request.kind) {
    case Int(KMUX_MEASURE_METRICS), Int(KMUX_MEASURE_WRAPPED_RAW):
        out.pointee.width = Double(measured.width)
        out.pointee.height = Double(measured.height)
        out.pointee.line_count = UInt32(measured.lines)
        if Int(request.kind) == Int(KMUX_MEASURE_WRAPPED_RAW) {
            out.pointee.raw_width = Double(LabelText.measure(text, size: size, bold: request.bold != 0, italic: request.italic != 0,
                                                             monospaced: monospaced, maxWidth: nil, htmlLike: htmlLike).width)
        }
    case Int(KMUX_MEASURE_WIDTH):
        out.pointee.length = Double(LabelText.measure(text, size: size, bold: request.bold != 0, italic: request.italic != 0,
                                                      monospaced: monospaced, maxWidth: nil, htmlLike: htmlLike).width)
    case Int(KMUX_MEASURE_HEIGHT):
        out.pointee.length = Double(measured.height)
    case Int(KMUX_MEASURE_EXTENTS):
        let width = LabelText.measure(text, size: size, bold: request.bold != 0, italic: request.italic != 0,
                                      monospaced: monospaced, maxWidth: nil, htmlLike: htmlLike).width
        out.pointee.left = Double(width) / 2 // distances from the anchor, as merman's own measurer
        out.pointee.right = Double(width) / 2
    default:
        return 0
    }
    return 1
}
