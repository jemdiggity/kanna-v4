import AppKit

struct GitPlace: Equatable {
    let root: String
    let branch: String?
    let main: String

    static func of(_ folder: String) -> GitPlace? {
        var url = URL(fileURLWithPath: folder).standardizedFileURL
        while true {
            let git = url.appendingPathComponent(".git")
            var isFolder: ObjCBool = false
            if FileManager.default.fileExists(atPath: git.path, isDirectory: &isFolder) {
                let dir = isFolder.boolValue ? git : gitdir(from: git, base: url)
                let main = isFolder.boolValue ? nil : dir.flatMap(mainRepo)
                return GitPlace(root: url.path, branch: dir.flatMap(branch), main: main ?? url.path)
            }
            let parent = url.deletingLastPathComponent()
            if parent.path == url.path { return nil }
            url = parent
        }
    }

    private static func mainRepo(_ gitdir: URL) -> String? {
        let text = gitdir.path
        return text.range(of: "/.git/worktrees/").map { String(text[..<$0.lowerBound]) }
    }

    private static func gitdir(from file: URL, base: URL) -> URL? {
        guard let text = small(file), let line = text.split(separator: "\n").first, line.hasPrefix("gitdir: ") else { return nil }
        let path = String(line.dropFirst("gitdir: ".count)).trimmingCharacters(in: .whitespaces)
        return path.hasPrefix("/") ? URL(fileURLWithPath: path) : base.appendingPathComponent(path).standardizedFileURL
    }

    private static func branch(_ dir: URL) -> String? {
        guard let text = small(dir.appendingPathComponent("HEAD"))?.trimmingCharacters(in: .whitespacesAndNewlines) else { return nil }
        if text.hasPrefix("ref: refs/heads/") {
            let name = text.dropFirst("ref: refs/heads/".count)
            return !name.isEmpty && name.unicodeScalars.allSatisfy({ $0.value > 0x20 && $0.value != 0x7f && $0 != "\\" }) ? String(name) : nil
        }
        return text.count >= 40 && text.allSatisfy(\.isHexDigit) ? String(text.prefix(7)) : nil
    }

    private static func small(_ file: URL) -> String? {
        guard let attributes = try? FileManager.default.attributesOfItem(atPath: file.path),
              attributes[.type] as? FileAttributeType == .typeRegular,
              let size = attributes[.size] as? Int, size <= 4096,
              let data = FileManager.default.contents(atPath: file.path) else { return nil }
        return String(data: data, encoding: .utf8)
    }
}

func canonical(_ path: String) -> String {
    URL(fileURLWithPath: path).resolvingSymlinksInPath().path
}

func tilde(_ path: String) -> String {
    let home = NSHomeDirectory()
    return path == home ? "~" : path.hasPrefix(home + "/") ? "~" + path.dropFirst(home.count) : path
}

@MainActor
final class AgentHeader: NSView {
    static let height: CGFloat = 34
    var onPark: (() -> Void)?
    private let label = NSTextField(labelWithString: "")
    private let park = ParkButton()
    var parkable = true {
        didSet { park.isHidden = !parkable }
    }

    override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
        wantsLayer = true
        label.font = .systemFont(ofSize: 12)
        label.textColor = .secondaryLabelColor
        label.lineBreakMode = .byTruncatingMiddle
        addSubview(label)
        park.toolTip = "Park"
        park.onPress = { [weak self] in self?.onPark?() }
        addSubview(park)
        paint()
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    override func viewDidChangeEffectiveAppearance() {
        super.viewDidChangeEffectiveAppearance()
        paint()
    }

    private func paint() { layer?.backgroundColor = resolved(.windowBackgroundColor) }

    var parkCenter: NSPoint { park.convert(NSPoint(x: park.bounds.midX, y: park.bounds.midY), to: nil) }
    var text: String { label.stringValue }

    func show(_ text: String) {
        if label.stringValue != text { label.stringValue = text }
        needsLayout = true
    }

    override func layout() {
        super.layout()
        park.frame = NSRect(x: bounds.width - 30, y: (bounds.height - 22) / 2, width: 22, height: 22)
        label.sizeToFit()
        label.frame = NSRect(x: 14, y: (bounds.height - label.frame.height) / 2, width: max(0, park.frame.minX - 22), height: label.frame.height)
    }

    override func draw(_ dirtyRect: NSRect) {
        NSColor.separatorColor.setFill()
        NSRect(x: 0, y: 0, width: bounds.width, height: 1).fill()
    }
}

@MainActor
private final class ParkButton: NSView {
    var onPress: (() -> Void)?
    private var hover = false

    override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
        addTrackingArea(NSTrackingArea(rect: .zero, options: [.mouseEnteredAndExited, .activeAlways, .inVisibleRect], owner: self))
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    override var isFlipped: Bool { true }

    override func draw(_ dirtyRect: NSRect) {
        if hover {
            NSColor.quaternaryLabelColor.setFill()
            NSBezierPath(roundedRect: bounds, xRadius: 5, yRadius: 5).fill()
        }
        let scale = bounds.width / 22
        let transform = AffineTransform(translationByX: 3 * scale, byY: 3 * scale)
        (hover ? NSColor.labelColor : NSColor.secondaryLabelColor).setStroke()
        let frame = NSBezierPath(roundedRect: NSRect(x: 2, y: 2, width: 12, height: 12), xRadius: 2, yRadius: 2)
        let chevron = NSBezierPath()
        chevron.move(to: NSPoint(x: 5.5, y: 5.5))
        chevron.line(to: NSPoint(x: 8, y: 8))
        chevron.line(to: NSPoint(x: 10.5, y: 5.5))
        let shelf = NSBezierPath()
        shelf.move(to: NSPoint(x: 5, y: 11))
        shelf.line(to: NSPoint(x: 11, y: 11))
        for (path, width) in [(frame, 1.4), (chevron, 1.4), (shelf, 2.0)] {
            path.transform(using: transform)
            path.lineWidth = width
            path.lineCapStyle = .round
            path.lineJoinStyle = .round
            path.stroke()
        }
    }

    override func mouseEntered(with event: NSEvent) { hover = true; needsDisplay = true }
    override func mouseExited(with event: NSEvent) { hover = false; needsDisplay = true }
    override func mouseDown(with event: NSEvent) { onPress?() }
}

extension AppController {
    func isTask(_ pane: String) -> Bool {
        windows.contains { $0["dock"]?["pane"]?.string == pane } || shelfEntries.contains { $0["task"]?.string == pane }
    }

    func place(of pane: String) -> (folder: String, git: GitPlace?)? {
        guard let folder = folders[pane] else { return nil }
        return (folder, GitPlace.of(folder))
    }

    func headerText(_ pane: String) -> String {
        guard let place = place(of: pane) else { return "" }
        return [tilde(place.folder), place.git?.branch].compactMap { $0 }.joined(separator: " · ")
    }

    func projectRoot(_ pane: String) -> String? {
        place(of: pane).map { canonical($0.git?.main ?? $0.folder) }
    }

    func projectName(_ tasks: [String]) -> String {
        tasks.lazy.compactMap(projectRoot).first.map(tilde) ?? "Shelf"
    }

    func taskDetail(_ pane: String) -> String {
        place(of: pane)?.git?.branch ?? ""
    }
}

extension AppController {
    func backlogChanged(_ tasks: [JSON]) {
        backlog = tasks
        refresh()
    }

    private func taskName(_ pane: String) -> String? {
        if let window = windows.first(where: { $0["dock"]?["pane"]?.string == pane }) { return taskTitle(pane, name: window["dock"]?["name"]?.string) }
        if let entry = shelfEntries.first(where: { $0["task"]?.string == pane }) { return taskTitle(pane, name: entry["name"]?.string) }
        return nil
    }

    func backlogTasks(in root: String?) -> [BacklogTask] {
        backlog.filter { root == nil || $0["repo"]?.string.map(canonical) == root }.compactMap { entry in
            guard let id = entry["id"]?.string, let name = entry["name"]?.string else { return nil }
            return BacklogTask(id: id, name: name, message: entry["message"]?.string, from: entry["by"]?.string.flatMap(taskName), madeAt: entry["madeAt"]?.number)
        }
    }

    func runningTasks(in root: String?) -> Int {
        let docked = windows.compactMap { $0["dock"]?["pane"]?.string } + shelfEntries.compactMap { $0["task"]?.string }
        return docked.filter { root == nil || projectRoot($0) == root }.count
    }

    private func noted(_ window: String, _ reply: JSON) {
        guard reply["ok"]?.bool != true, let controller = controllers[window] else { return }
        controller.note = reply["error"]?["message"]?.string ?? "it didn't start"
        refresh()
    }

    func startTask(_ task: String, from window: String) {
        controllers[window]?.note = ""
        request("backlog", ["action": "start", "task": .string(task)]) { [weak self] reply in self?.noted(window, reply) }
    }

    func startBlankTask(in window: String) {
        guard let controller = controllers[window] else { return }
        controller.note = ""
        let root = controller.projectRoot ?? NSHomeDirectory()
        let args: [String: JSON] = ["action": "blank", "repo": .string(root), "window": .string(window)]
        request("backlog", args) { [weak self] reply in self?.noted(window, reply) }
    }

    func openPlanner(in window: String) {
        guard !starting.contains(window), let controller = controllers[window] else { return }
        starting.insert(window)
        let root = controller.projectRoot
        var args: [String: JSON] = ["action": "plan", "window": .string(window)]
        if let root {
            args["repo"] = .string(root)
            if self.window(window)?["project"]?.string == nil {
                DispatchQueue.main.async { [weak self] in self?.layoutCommand("task", ["action": "desk", "window": .string(window), "project": .string(root)]) }
            }
        }
        if runningTasks(in: root) == 0, backlogTasks(in: root).isEmpty, let guide = Bundle.main.url(forResource: "getting-started", withExtension: "md") {
            args["guide"] = .string(guide.path)
        }
        request("backlog", args) { [weak self] reply in
            guard let self else { return }
            if reply["ok"]?.bool == true { starting.remove(window) }
            noted(window, reply)
        }
    }

    func wantsPlanner(_ window: JSON) -> Bool {
        guard let id = window["id"]?.string, window["desk"]?.bool == true, window["dock"]?["pane"]?.string == nil else { return false }
        return window["planner"]?["pane"]?.string == nil && !starting.contains(id)
    }

    func plannerHeader(_ root: String?) -> String {
        "Planning · " + (root.map(tilde) ?? "no project yet")
    }

    var latestProject: String? {
        let parked = shelfEntries.sorted { ($0["parkedAt"]?.number ?? 0) > ($1["parkedAt"]?.number ?? 0) }.lazy.compactMap { $0["task"]?.string }.compactMap(projectRoot).first
        let waiting = backlog.sorted { ($0["madeAt"]?.number ?? 0) > ($1["madeAt"]?.number ?? 0) }.lazy.compactMap { $0["repo"]?.string }.first.map(canonical)
        return parked ?? waiting
    }
}
