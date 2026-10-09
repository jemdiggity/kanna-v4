import AppKit

@MainActor
final class LayoutView: NSView {
    private(set) var panes: [String: NSView] = [:]
    private(set) var order: [String] = []
    private var root: JSON = .null
    var make: (_ pane: String, _ type: String) -> NSView = { _, _ in NSView() }
    var added: (_ pane: String, _ view: NSView) -> Void = { _, _ in }
    var removed: (_ pane: String, _ view: NSView) -> Void = { _, _ in }

    private let gap: CGFloat = 1

    override init(frame: NSRect) {
        super.init(frame: frame)
        wantsLayer = true
        layer?.backgroundColor = NSColor.separatorColor.cgColor
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    func show(_ tree: JSON) {
        var types: [(String, String)] = []
        collect(tree, into: &types)
        let ids = Set(types.map(\.0))
        for (id, view) in panes where !ids.contains(id) {
            removed(id, view)
            view.removeFromSuperview()
            panes.removeValue(forKey: id)
        }
        var new: [String] = []
        for (id, type) in types where panes[id] == nil {
            let view = make(id, type)
            view.autoresizingMask = []
            addSubview(view)
            panes[id] = view
            new.append(id)
        }
        root = tree
        order = types.map(\.0)
        place(root, in: bounds)
        for id in new { added(id, panes[id]!) }
    }

    override func layout() {
        super.layout()
        place(root, in: bounds)
    }

    private func collect(_ node: JSON, into types: inout [(String, String)]) {
        if let id = node["pane"]?.string { return types.append((id, node["type"]?.string ?? "term")) }
        if case .array(let children)? = node["children"] { children.forEach { collect($0, into: &types) } }
    }

    private func place(_ node: JSON, in rect: NSRect) {
        if let id = node["pane"]?.string {
            panes[id]?.frame = rect.integral
            return
        }
        guard case .array(let children)? = node["children"], !children.isEmpty else { return }
        let row = node["split"]?.string == "row"
        let total = (row ? rect.width : rect.height) - gap * CGFloat(children.count - 1)
        var offset: CGFloat = 0
        for child in children {
            let length = total * CGFloat(child["size"]?.number ?? 1 / Double(children.count))
            let frame = row
                ? NSRect(x: rect.minX + offset, y: rect.minY, width: length, height: rect.height)
                : NSRect(x: rect.minX, y: rect.maxY - offset - length, width: rect.width, height: length)
            place(child, in: frame)
            offset += length + gap
        }
    }
}

@MainActor
final class PlaceholderView: NSView {
    init(pane: String, type: String) {
        super.init(frame: .zero)
        wantsLayer = true
        layer?.backgroundColor = NSColor.windowBackgroundColor.cgColor
        let label = NSTextField(labelWithString: "\(type) pane \(pane): not shown yet")
        label.textColor = .secondaryLabelColor
        label.translatesAutoresizingMaskIntoConstraints = false
        addSubview(label)
        NSLayoutConstraint.activate([label.centerXAnchor.constraint(equalTo: centerXAnchor), label.centerYAnchor.constraint(equalTo: centerYAnchor)])
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }
}
