import AppKit

/// Class diagrams: merman's class boxes, namespaces (its clusters), notes and
/// relations. The rows inside a box (annotation, name, members, methods) are
/// placed here as Mermaid's classBox does, inside the box merman sized for them.
enum Class {
    /// Mermaid's class padding: around the rows, and between the compartments.
    static let padding: CGFloat = 12
    static let rowHeight: CGFloat = 21
    // Every label is drawn at 14 pt, as merman lays them out: it also measures
    // class names at other sizes (wrap probes), so the measure log can't be used.

    static func scene(semantic: [String: Any], layout: [String: Any]) -> DiagramScene {
        let frame = Frame(layout: layout)
        var scene = DiagramScene(size: frame.size)
        let classes = semantic.dict("classes") ?? [:]
        let relations = Dictionary(semantic.list("relations").compactMap { r in r.string("id").map { ($0, r) } }, uniquingKeysWith: { a, _ in a })
        let notes = Dictionary(semantic.list("notes").compactMap { n in n.string("id").map { ($0, n) } }, uniquingKeysWith: { a, _ in a })
        let interfaces = Dictionary(semantic.list("interfaces").compactMap { i in i.string("id").map { ($0, i) } }, uniquingKeysWith: { a, _ in a })
        let styleClasses = semantic.dict("styleClasses") ?? [:]
        let boxes = Dictionary(layout.list("nodes").compactMap { n in n.string("id").flatMap { id in n.box.map { (id, frame.rect($0)) } } },
                               uniquingKeysWith: { a, _ in a })

        for cluster in layout.list("clusters") {
            guard let box = cluster.box.map(frame.rect) else { continue }
            scene.shape(Paths.rect(box), fill: .role(.clusterFill), stroke: .role(.nodeStroke), shadow: true)
            let titleBox = (cluster.dict("title_label")?.box).map(frame.rect) ?? CGRect(x: box.minX, y: box.minY + 4, width: box.width, height: rowHeight)
            scene.text(cluster.string("title") ?? "", in: CGRect(x: box.minX + 4, y: titleBox.minY, width: box.width - 8, height: max(titleBox.height, 20)), fixedSize: true)
        }

        for edge in layout.list("edges") {
            let id = edge.string("id") ?? ""
            Self.edge(edge, relation: relations[id], from: edge.string("from").flatMap { boxes[$0] }, frame: frame, into: &scene)
        }

        for node in layout.list("nodes") where node["is_cluster"] as? Bool != true {
            guard let id = node.string("id"), let box = boxes[id] else { continue }
            if let note = notes[id] {
                scene.shape(Paths.rect(box), fill: .role(.noteFill), stroke: .role(.noteStroke), lineWidth: 1.3, shadow: true)
                scene.text(escaped(note.string("text") ?? ""), in: box.insetBy(dx: 4, dy: 2), fixedSize: true)
            } else if let interface = interfaces[id] {
                scene.text(escaped(interface.string("label") ?? id), in: box.insetBy(dx: 2, dy: 2), fixedSize: true)
            } else if let info = classes[id] as? [String: Any] {
                drawClass(info, id: id, box: box, styleClasses: styleClasses, into: &scene)
            }
        }
        return scene
    }

    private static func drawClass(_ info: [String: Any], id: String, box: CGRect, styleClasses: [String: Any], into scene: inout DiagramScene) {
        var css = (info["styles"] as? [String]) ?? []
        for name in (info.string("cssClasses") ?? "").split(separator: " ").map(String.init) {
            css = ((styleClasses[name] as? [String: Any])?["styles"] as? [String] ?? []) + css
        }
        let style = Paths.css(css)
        // The class's colour comes from the counter in its DOM id ("classId-Shape-0"), as in Mermaid.
        let index = Int(info.string("domId")?.split(separator: "-").last ?? "") ?? 0
        let fill: Paint = style.fill.map(Paint.color) ?? .role(.classFill(index))
        let stroke: Paint = style.stroke.map(Paint.color) ?? .role(.classStroke(index))
        let text: Paint = style.text.map(Paint.color) ?? .role(.text)
        scene.shape(Paths.rect(box), fill: fill, stroke: stroke, lineWidth: 2, dash: style.dash ? [3, 3] : [], shadow: true)

        let members = info.list("members"), methods = info.list("methods")
        let annotation = (info["annotations"] as? [String])?.first.map { "«\($0.trimmingCharacters(in: .whitespaces))»" }
        let annotationHeight: CGFloat = annotation == nil ? 0 : rowHeight
        let (left, top) = (box.minX + padding, box.minY)
        let width = box.width - 2 * padding
        let empty = members.isEmpty && methods.isEmpty
        if let annotation {
            scene.text(escaped(annotation), in: CGRect(x: left, y: top + padding + 1.5, width: width, height: rowHeight), color: text, fixedSize: true)
        }
        let title = info.string("text").flatMap { $0.isEmpty ? nil : $0 } ?? info.string("label") ?? id
        scene.text(title, in: CGRect(x: left, y: top + padding + annotationHeight, width: width, height: rowHeight), color: text, bold: true, fixedSize: true)

        // Two dividers: under the name, and between members and methods. An
        // empty class still shows both compartments, as Mermaid draws it.
        let membersHeight = CGFloat(members.count) * rowHeight
        let nameBottom = top + 2 * padding + annotationHeight + rowHeight
        let dividers = [nameBottom, nameBottom + (empty ? 1.5 * padding : membersHeight + 2 * padding)]
        for y in dividers {
            scene.shape(Paths.line([CGPoint(x: box.minX, y: y), CGPoint(x: box.maxX, y: y)]), stroke: stroke, lineWidth: 2)
        }
        let membersTop = nameBottom + 2 * padding - rowHeight / 2
        let methodsTop = nameBottom + (members.isEmpty ? padding / 2 : membersHeight) + 4 * padding - rowHeight / 2
        for (rows, start) in [(members, membersTop), (methods, methodsTop)] {
            for (i, row) in rows.enumerated() {
                let rect = CGRect(x: left, y: start + CGFloat(i) * rowHeight, width: width, height: rowHeight)
                let label = row.string("displayText") ?? ""
                // Mermaid's classifiers: `*` (abstract) is italic, `$` (static) underlined.
                let italic = row.string("classifier") == "*"
                scene.text(escaped(label), in: rect, color: text, italic: italic, alignment: .left, fixedSize: true)
                if row.string("classifier") == "$" {
                    let font = LabelText.font(size: 14, italic: italic)
                    let lineWidth = LabelText.width(label, font: font)
                    let y = rect.midY + CTFontGetAscent(font) / 2 + 2
                    scene.shape(Paths.line([CGPoint(x: left, y: y), CGPoint(x: left + lineWidth, y: y)]), stroke: text)
                }
            }
        }
    }

    /// Text to draw as is: label markup (`<…>`, entities) would otherwise be read in it.
    private static func escaped(_ text: String) -> String {
        text.replacingOccurrences(of: "&", with: "&amp;").replacingOccurrences(of: "<", with: "&lt;").replacingOccurrences(of: ">", with: "&gt;")
    }

    /// Mermaid's relation ends: type1 is drawn at the first class, type2 at the second.
    enum End: Int { case aggregation = 0, inheritance = 1, composition = 2, dependency = 3, lollipop = 4 }

    private static func edge(_ edge: [String: Any], relation: [String: Any]?, from: CGRect?, frame: Frame, into scene: inout DiagramScene) {
        var points = edge.points.map(frame.point)
        guard points.count >= 2 else { return }
        let kinds = relation?.dict("relation") ?? [:]
        // A note's link is dotted, as is a relation drawn `..`.
        let dotted = relation == nil || (kinds["lineType"] as? NSNumber)?.intValue == 1
        var start = (kinds["type1"] as? NSNumber).flatMap { End(rawValue: $0.intValue) }
        var end = (kinds["type2"] as? NSNumber).flatMap { End(rawValue: $0.intValue) }
        // ELK may route an edge backwards; the first class's end is the one nearer its box.
        if let from, distance(points[points.count - 1], from) < distance(points[0], from) { swap(&start, &end) }
        if let start { points = Array(Paths.shorten(points.reversed(), end: length(start)).reversed()) }
        if let end { points = Paths.shorten(points, end: length(end)) }
        scene.shape(Paths.rounded(points), stroke: .role(.line), lineWidth: 2, dash: dotted ? [2, 2] : [])
        let original = edge.points.map(frame.point)
        if let start { marker(start, tip: original[0], from: original[1], into: &scene) }
        if let end { marker(end, tip: original[original.count - 1], from: original[original.count - 2], into: &scene) }

        if let title = relation?.string("title"), !title.isEmpty, let box = edge.dict("label")?.box.map(frame.rect) {
            scene.shape(Paths.rect(box.insetBy(dx: -2, dy: 0)), fill: .role(.frameLabelFill))
            scene.text(escaped(title), in: box, fixedSize: true)
        }
        // Cardinalities: relationTitle1 is placed at the start, relationTitle2 at the end.
        for (key, sides) in [("relationTitle1", ["start_label_left", "start_label_right"]), ("relationTitle2", ["end_label_left", "end_label_right"])] {
            guard let text = relation?.string(key), text != "none", !text.isEmpty else { continue }
            for side in sides {
                if let box = edge.dict(side)?.box.map(frame.rect) {
                    scene.text(escaped(text), in: box.insetBy(dx: -2, dy: 0), size: 11, fixedSize: true)
                }
            }
        }
    }

    private static func distance(_ p: CGPoint, _ r: CGRect) -> CGFloat {
        hypot(max(r.minX - p.x, 0, p.x - r.maxX), max(r.minY - p.y, 0, p.y - r.maxY))
    }

    /// How far a line stops short of the class for each end's marker.
    private static func length(_ end: End) -> CGFloat {
        switch end {
        case .aggregation, .composition: 17
        case .inheritance, .dependency: 9
        case .lollipop: 12
        }
    }

    /// Mermaid's class markers: hollow triangle (inheritance), diamonds
    /// (aggregation hollow, composition filled), a notched arrow (dependency)
    /// and a circle (lollipop), with the tip on the class.
    private static func marker(_ end: End, tip: CGPoint, from: CGPoint, into scene: inout DiagramScene) {
        let angle = atan2(tip.y - from.y, tip.x - from.x)
        func at(_ back: CGFloat, _ side: CGFloat) -> CGPoint {
            CGPoint(x: tip.x - back * cos(angle) - side * sin(angle), y: tip.y - back * sin(angle) + side * cos(angle))
        }
        switch end {
        case .inheritance:
            scene.shape(Paths.polygon([tip, at(9, 6), at(9, -6)]), fill: .role(.frameLabelFill), stroke: .role(.line), lineWidth: 1.5)
        case .aggregation, .composition:
            let filled = end == .composition
            scene.shape(Paths.polygon([tip, at(8.5, 6), at(17, 0), at(8.5, -6)]), fill: filled ? .role(.line) : .role(.frameLabelFill),
                        stroke: .role(.line), lineWidth: filled ? 1 : 1.5)
        case .dependency:
            scene.shape(Paths.polygon([tip, at(9, 6), at(4, 0), at(9, -6)]), fill: .role(.line), stroke: .role(.line))
        case .lollipop:
            let c = at(6, 0)
            scene.shape(CGPath(ellipseIn: CGRect(x: c.x - 6, y: c.y - 6, width: 12, height: 12), transform: nil),
                        fill: .role(.frameLabelFill), stroke: .role(.line), lineWidth: 1.5)
        }
    }
}
