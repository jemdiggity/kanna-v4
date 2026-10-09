import CoreGraphics

/// Flowcharts: merman's node boxes and edge points, with shapes, arrows and
/// labels from its semantic model.
enum Flowchart {
    static func scene(semantic: [String: Any], layout: [String: Any]) -> DiagramScene {
        let frame = Frame(layout: layout)
        var scene = DiagramScene(size: frame.size)
        let nodes = Dictionary(semantic.list("nodes").compactMap { n in n.string("id").map { ($0, n) } }, uniquingKeysWith: { a, _ in a })
        let edges = Dictionary(semantic.list("edges").compactMap { e in e.string("id").map { ($0, e) } }, uniquingKeysWith: { a, _ in a })
        let classDefs = semantic.dict("classDefs") ?? [:]

        for cluster in layout.list("clusters") {
            guard let box = cluster.box.map(frame.rect) else { continue }
            scene.shape(Paths.rect(box, radius: 3), fill: .role(.clusterFill), stroke: .role(.clusterStroke))
            let title = cluster.string("title") ?? ""
            let titleBox = (cluster.dict("title_label")?.box).map(frame.rect)
                ?? CGRect(x: box.minX, y: box.minY + 4, width: box.width, height: 24)
            scene.text(title, in: CGRect(x: box.minX + 4, y: titleBox.minY, width: box.width - 8, height: max(titleBox.height, 20)))
        }

        for edge in layout.list("edges") {
            let info = edge.string("id").flatMap { edges[$0] } ?? [:]
            Self.edge(edge, info: info, frame: frame, into: &scene)
        }

        for node in layout.list("nodes") where node["is_cluster"] as? Bool != true {
            guard let id = node.string("id"), let box = node.box.map(frame.rect) else { continue }
            let info = nodes[id] ?? [:]
            var css = (info["styles"] as? [String]) ?? []
            for name in (info["classes"] as? [String]) ?? [] {
                // merman gives a classDef as its CSS declarations ("fill:#f96", ...).
                let def = classDefs[name] as? [String] ?? (classDefs[name] as? [String: Any])?["styles"] as? [String] ?? []
                css = def + css
            }
            let style = Paths.css(css)
            let fill: Paint = style.fill.map(Paint.color) ?? .role(.nodeFill)
            let stroke: Paint = style.stroke.map(Paint.color) ?? .role(.nodeStroke)
            let shape = info.string("shape") ?? "square"
            for path in paths(shape: shape, box: box) {
                scene.shape(path, fill: fill, stroke: stroke, lineWidth: 1.5, dash: style.dash ? [3, 3] : [], shadow: true)
            }
            scene.text(info.string("label") ?? id, in: labelRect(shape: shape, box: box),
                       color: style.text.map(Paint.color) ?? .role(.text))
        }
        return scene
    }

    private static func edge(_ edge: [String: Any], info: [String: Any], frame: Frame, into scene: inout DiagramScene) {
        let points = edge.points.map(frame.point)
        guard points.count >= 2 else { return }
        let type = info.string("type") ?? "arrow_point"
        let stroke = info.string("stroke") ?? "normal"
        guard stroke != "invisible" else { return }
        func head(_ name: String) -> Paths.Head {
            name.contains("point") ? .filled : name.contains("circle") ? .circle : name.contains("cross") ? .cross : .none
        }
        let end = head(type)
        let start: Paths.Head = type.hasPrefix("double_") ? end : .none
        var line = Paths.shorten(points, end: end == .filled ? 8 : 0)
        if start == .filled { line = Array(Paths.shorten(line.reversed(), end: 8).reversed()) }
        let width: CGFloat = stroke == "thick" ? 3 : 1.25
        scene.shape(Paths.rounded(line), stroke: .role(.line), lineWidth: width, dash: stroke == "dotted" ? [3, 3] : [])
        for (kind, tip, from) in [(end, points[points.count - 1], points[points.count - 2]), (start, points[0], points[1])] {
            if let (path, filled) = Paths.head(kind, tip: tip, from: from) {
                scene.shape(path, fill: filled ? .role(.line) : nil, stroke: .role(.line), lineWidth: filled ? 1 : width)
            }
        }
        if let label = info.string("label"), !label.isEmpty, let box = edge.dict("label")?.box.map(frame.rect) {
            scene.shape(Paths.rect(box.insetBy(dx: -2, dy: 0)), fill: .role(.labelBackground))
            scene.text(label, in: box)
        }
    }

    /// The outline (and any inner lines) of a node shape, in the shape's box.
    static func paths(shape: String, box b: CGRect) -> [CGPath] {
        let (x, y, w, h) = (b.minX, b.minY, b.width, b.height)
        switch shape {
        case "round": return [Paths.rect(b, radius: 5)]
        case "stadium": return [Paths.rect(b, radius: h / 2)]
        case "circle": return [CGPath(ellipseIn: b, transform: nil)]
        case "doublecircle":
            return [CGPath(ellipseIn: b, transform: nil), CGPath(ellipseIn: b.insetBy(dx: 5, dy: 5), transform: nil)]
        case "diamond", "question":
            return [Paths.polygon([CGPoint(x: b.midX, y: y), CGPoint(x: b.maxX, y: b.midY), CGPoint(x: b.midX, y: b.maxY), CGPoint(x: x, y: b.midY)])]
        case "hexagon":
            let f = h / 4
            return [Paths.polygon([CGPoint(x: x + f, y: y), CGPoint(x: x + w - f, y: y), CGPoint(x: x + w, y: b.midY),
                                   CGPoint(x: x + w - f, y: y + h), CGPoint(x: x + f, y: y + h), CGPoint(x: x, y: b.midY)])]
        case "subroutine":
            return [Paths.rect(b), Paths.line([CGPoint(x: x + 8, y: y), CGPoint(x: x + 8, y: y + h)]),
                    Paths.line([CGPoint(x: x + w - 8, y: y), CGPoint(x: x + w - 8, y: y + h)])]
        case "cylinder":
            // A side wall and bottom arc, closed along the top ellipse's lower half.
            let ry = min(h / 4, w / 2 / (2.5 + w / 50)), rx = w / 2, k: CGFloat = 0.5523
            let body = CGMutablePath()
            body.move(to: CGPoint(x: x, y: y + ry))
            body.addLine(to: CGPoint(x: x, y: y + h - ry))
            body.addCurve(to: CGPoint(x: b.midX, y: y + h), control1: CGPoint(x: x, y: y + h - ry + k * ry), control2: CGPoint(x: b.midX - k * rx, y: y + h))
            body.addCurve(to: CGPoint(x: x + w, y: y + h - ry), control1: CGPoint(x: b.midX + k * rx, y: y + h), control2: CGPoint(x: x + w, y: y + h - ry + k * ry))
            body.addLine(to: CGPoint(x: x + w, y: y + ry))
            body.addCurve(to: CGPoint(x: b.midX, y: y + 2 * ry), control1: CGPoint(x: x + w, y: y + ry + k * ry), control2: CGPoint(x: b.midX + k * rx, y: y + 2 * ry))
            body.addCurve(to: CGPoint(x: x, y: y + ry), control1: CGPoint(x: b.midX - k * rx, y: y + 2 * ry), control2: CGPoint(x: x, y: y + ry + k * ry))
            body.closeSubpath()
            // The top as its own shape: in one path with the body, the overlap would cancel out when filled.
            return [body, CGPath(ellipseIn: CGRect(x: x, y: y, width: w, height: 2 * ry), transform: nil)]
        case "lean_right":
            let o = h / 3
            return [Paths.polygon([CGPoint(x: x + o, y: y), CGPoint(x: x + w, y: y), CGPoint(x: x + w - o, y: y + h), CGPoint(x: x, y: y + h)])]
        case "lean_left":
            let o = h / 3
            return [Paths.polygon([CGPoint(x: x, y: y), CGPoint(x: x + w - o, y: y), CGPoint(x: x + w, y: y + h), CGPoint(x: x + o, y: y + h)])]
        case "trapezoid":
            let o = h / 3
            return [Paths.polygon([CGPoint(x: x + o, y: y), CGPoint(x: x + w - o, y: y), CGPoint(x: x + w, y: y + h), CGPoint(x: x, y: y + h)])]
        case "inv_trapezoid":
            let o = h / 3
            return [Paths.polygon([CGPoint(x: x, y: y), CGPoint(x: x + w, y: y), CGPoint(x: x + w - o, y: y + h), CGPoint(x: x + o, y: y + h)])]
        case "odd":
            return [Paths.polygon([CGPoint(x: x, y: y), CGPoint(x: x + w, y: y), CGPoint(x: x + w, y: y + h), CGPoint(x: x, y: y + h), CGPoint(x: x + h / 2, y: b.midY)])]
        default: return [Paths.rect(b)]
        }
    }

    /// Where a shape's label goes: inside the outline, clear of its edges.
    static func labelRect(shape: String, box b: CGRect) -> CGRect {
        switch shape {
        // Mermaid makes a diamond's side the label's width plus its height, so
        // a one-line label gets about two thirds of the width across the middle.
        case "diamond", "question": return b.insetBy(dx: b.width * 0.175, dy: b.height / 4)
        case "circle", "doublecircle": return b.insetBy(dx: b.width * 0.15, dy: b.height * 0.15)
        case "lean_right", "lean_left", "trapezoid", "inv_trapezoid", "hexagon": return b.insetBy(dx: b.height / 3, dy: 2)
        case "subroutine": return b.insetBy(dx: 10, dy: 2)
        case "odd": return CGRect(x: b.minX + b.height / 2, y: b.minY + 2, width: b.width - b.height / 2 - 4, height: b.height - 4)
        default: return b.insetBy(dx: 4, dy: 2)
        }
    }
}
