import CoreGraphics

/// State diagrams: states, start and end points, choices, forks, notes and
/// composite states (merman's clusters), joined by transitions.
enum State {
    static func scene(semantic: [String: Any], layout: [String: Any]) -> DiagramScene {
        let frame = Frame(layout: layout)
        var scene = DiagramScene(size: frame.size)
        let nodes = Dictionary(semantic.list("nodes").compactMap { n in n.string("id").map { ($0, n) } }, uniquingKeysWith: { a, _ in a })
        let edges = Dictionary(semantic.list("edges").compactMap { e in e.string("id").map { ($0, e) } }, uniquingKeysWith: { a, _ in a })

        for cluster in layout.list("clusters") {
            guard let box = cluster.box.map(frame.rect) else { continue }
            let title = cluster.string("title").flatMap { $0.isEmpty ? nil : $0 } ?? cluster.string("id").flatMap { nodes[$0]?.string("label") } ?? ""
            scene.shape(Paths.rect(box, radius: 5), fill: .role(.clusterFill), stroke: .role(.clusterStroke), shadow: true)
            let titleHeight: CGFloat = 26
            scene.shape(Paths.rect(CGRect(x: box.minX + 1, y: box.minY + titleHeight, width: box.width - 2, height: box.height - titleHeight - 1), radius: 4),
                        fill: .role(.activationFill))
            scene.text(title, in: CGRect(x: box.minX + 4, y: box.minY + 2, width: box.width - 8, height: titleHeight - 2))
        }

        for edge in layout.list("edges") {
            let points = edge.points.map(frame.point)
            guard points.count >= 2 else { continue }
            let info = edge.string("id").flatMap { edges[$0] } ?? [:]
            scene.shape(Paths.rounded(Paths.shorten(points, end: 8)), stroke: .role(.line), lineWidth: 1.25)
            if let (arrow, _) = Paths.head(.filled, tip: points[points.count - 1], from: points[points.count - 2]) {
                scene.shape(arrow, fill: .role(.line), stroke: .role(.line))
            }
            if let label = info.string("label"), !label.isEmpty, let box = edge.dict("label")?.box.map(frame.rect) {
                scene.shape(Paths.rect(box.insetBy(dx: -2, dy: 0)), fill: .role(.labelBackground))
                scene.text(label, in: box)
            }
        }

        for node in layout.list("nodes") where node["is_cluster"] as? Bool != true {
            guard let id = node.string("id"), let box = node.box.map(frame.rect) else { continue }
            let info = nodes[id] ?? [:]
            let label = info.string("label") ?? id
            switch info.string("shape") ?? "rect" {
            case "stateStart":
                scene.shape(CGPath(ellipseIn: box, transform: nil), fill: .role(.startFill))
            case "stateEnd":
                scene.shape(CGPath(ellipseIn: box, transform: nil), stroke: .role(.startFill), lineWidth: 1.5)
                scene.shape(CGPath(ellipseIn: box.insetBy(dx: 3, dy: 3), transform: nil), fill: .role(.startFill))
            case "choice":
                scene.shape(Paths.polygon([CGPoint(x: box.midX, y: box.minY), CGPoint(x: box.maxX, y: box.midY),
                                           CGPoint(x: box.midX, y: box.maxY), CGPoint(x: box.minX, y: box.midY)]),
                            fill: .role(.nodeFill), stroke: .role(.nodeStroke))
            case "fork", "join":
                scene.shape(Paths.rect(box, radius: 2), fill: .role(.startFill))
            case "note":
                scene.shape(Paths.rect(box), fill: .role(.noteFill), stroke: .role(.noteStroke))
                scene.text(label, in: box.insetBy(dx: 4, dy: 2))
            case "noteGroup", "divider":
                break
            default:
                scene.shape(Paths.rect(box, radius: 5), fill: .role(.nodeFill), stroke: .role(.nodeStroke), lineWidth: 1.5, shadow: true)
                scene.text(label, in: box.insetBy(dx: 4, dy: 2))
            }
        }
        return scene
    }
}
