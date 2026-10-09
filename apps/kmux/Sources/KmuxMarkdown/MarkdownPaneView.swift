import AppKit
import Markdown

/// A markdown pane: a file rendered natively (TextKit, no web view), read
/// only, reloaded when it changes on disk. Mermaid blocks are drawn by
/// KmuxDiagram. Zoom is plain magnification (NSScrollView's): a pinch or
/// ⌘= ⌘− ⌘0 enlarge the page to inspect details without reflowing it.
@MainActor
public final class MarkdownPaneView: NSView, NSTextViewDelegate {
    /// Safari's steps for Zoom In / Zoom Out; a pinch goes from 0.5 to 4.
    public static let zoomSteps: [CGFloat] = [0.5, 0.67, 0.75, 0.8, 0.9, 1, 1.1, 1.25, 1.5, 1.75, 2, 2.5, 3]

    public var onFocus: (() -> Void)? { didSet { textView.onFocus = onFocus } }
    /// A link to another markdown file (absolute path): show it in this pane.
    public var onOpenMarkdown: ((String) -> Void)?
    /// Back (true) or forward (false): the mouse's buttons 4 and 5, or a
    /// two-finger swipe sideways.
    public var onHistory: ((Bool) -> Void)?
    /// The magnification changed (a zoom key, or the end of a pinch).
    public var onZoom: (() -> Void)?
    /// Where to scroll once the pane is first laid out (returning to a file).
    public var startScrollY: CGFloat?
    /// kmux's pane menu, added below the text view's own items on a right-click.
    public var contextMenu: (() -> NSMenu?)? { didSet { textView.contextMenu = contextMenu } }
    public let path: String
    /// The view that takes keyboard focus.
    public var keyView: NSView { textView }
    /// The magnification (1 is actual size).
    public var zoom: CGFloat { scrollView.magnification }
    /// How long the last render took (parse excluded), in milliseconds.
    public private(set) var renderMs: Double = 0
    public private(set) var parseMs: Double = 0

    private let scrollView = MarkdownScrollView()
    private let textView: MarkdownTextView
    private let layoutManager = MarkdownLayoutManager()
    private let diagrams = DiagramCache()
    private var document = Document(parsing: "")
    private var stamp: (Date, Int)?
    private var poll: Timer?
    private var magnifyObserver: NSObjectProtocol?

    public init(path: String, zoom: CGFloat = 1) {
        self.path = path
        let storage = NSTextStorage()
        storage.addLayoutManager(layoutManager)
        let container = NSTextContainer(size: NSSize(width: 600, height: CGFloat.greatestFiniteMagnitude))
        container.widthTracksTextView = true
        layoutManager.addTextContainer(container)
        textView = MarkdownTextView(frame: NSRect(x: 0, y: 0, width: 600, height: 400), textContainer: container)
        super.init(frame: .zero)
        configureTextView()
        scrollView.contentView = CenteringClipView()
        scrollView.documentView = textView
        scrollView.hasVerticalScroller = true
        scrollView.autohidesScrollers = true
        scrollView.drawsBackground = true
        scrollView.allowsMagnification = true
        scrollView.minMagnification = Self.zoomSteps.first!
        scrollView.maxMagnification = 4
        scrollView.magnification = zoom
        scrollView.backgroundColor = MarkdownTheme.background
        addSubview(scrollView)
        textView.delegate = self
        scrollView.onSwipe = { [weak self] back in self?.onHistory?(back) }
        magnifyObserver = NotificationCenter.default.addObserver(forName: NSScrollView.didEndLiveMagnifyNotification, object: scrollView, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated { self?.onZoom?() }
        }
        load()
        poll = Timer.scheduledTimer(withTimeInterval: 0.5, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated { self?.reloadIfChanged() }
        }
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    private func configureTextView() {
        textView.isEditable = false
        textView.isSelectable = true
        textView.isRichText = true
        textView.importsGraphics = false
        textView.drawsBackground = true
        textView.backgroundColor = MarkdownTheme.background
        textView.isVerticallyResizable = true
        textView.isHorizontallyResizable = false
        // A text view made in code keeps its first frame as maxSize, and then never scrolls.
        textView.minSize = .zero
        textView.maxSize = NSSize(width: CGFloat.greatestFiniteMagnitude, height: CGFloat.greatestFiniteMagnitude)
        textView.autoresizingMask = [.width]
        textView.linkTextAttributes = [.foregroundColor: MarkdownTheme.accent, .cursor: NSCursor.pointingHand]
        textView.selectedTextAttributes = [.backgroundColor: NSColor.selectedTextBackgroundColor]
        textView.usesFindBar = true
        textView.isIncrementalSearchingEnabled = true
    }

    public func stop() {
        poll?.invalidate()
        poll = nil
        magnifyObserver.map(NotificationCenter.default.removeObserver)
        magnifyObserver = nil
    }

    public override func layout() {
        super.layout()
        scrollView.frame = bounds
        updateInset()
        if let y = startScrollY, bounds.height > 0 {
            startScrollY = nil
            scroll(toY: y)
        }
    }

    /// Mouse buttons 4 and 5 (back, forward), from the text, or from the
    /// space around a page zoomed out.
    public override func otherMouseDown(with event: NSEvent) {
        switch event.buttonNumber {
        case 3: onHistory?(true)
        case 4: onHistory?(false)
        default: super.otherMouseDown(with: event)
        }
    }

    /// Wide panes centre the text at its maximum width. The text is laid out
    /// for the pane's unmagnified width, so magnifying never re-wraps it.
    private func updateInset() {
        let width = scrollView.contentSize.width
        let inset = max(28, (width - (MarkdownTheme.maxTextWidth + 10)) / 2)
        let size = NSSize(width: inset, height: 22)
        if textView.textContainerInset != size { textView.textContainerInset = size }
        textView.layoutWidth = width
        if textView.frame.width != width { textView.frame.size.width = width }
    }

    // MARK: Loading

    private func fileStamp() -> (Date, Int)? {
        guard let attributes = try? FileManager.default.attributesOfItem(atPath: path) else { return nil }
        return ((attributes[.modificationDate] as? Date) ?? .distantPast, (attributes[.size] as? Int) ?? 0)
    }

    private func reloadIfChanged() {
        guard let now = fileStamp() else { return }
        if let stamp, stamp == now { return }
        load()
    }

    /// Reads and parses the file, then renders it where the reader was.
    private func load() {
        stamp = fileStamp()
        let text = FileManager.default.contents(atPath: path).map { String(decoding: $0, as: UTF8.self) } ?? ""
        let start = Date()
        document = Document(parsing: text, options: [.disableSmartOpts])
        parseMs = Date().timeIntervalSince(start) * 1000
        render()
    }

    private func render() {
        let start = Date()
        let position = readingPosition()
        var renderer = MarkdownRenderer(folder: URL(fileURLWithPath: path).deletingLastPathComponent(), diagrams: diagrams)
        let text = renderer.render(document)
        diagrams.keep(only: Set(renderer.diagramSources))
        updateInset()
        textView.textStorage?.setAttributedString(text)
        if let position { restoreReadingPosition(position) }
        renderMs = Date().timeIntervalSince(start) * 1000
    }

    // MARK: Zoom

    /// One step in (1) or out (-1), or back to actual size (0), about the middle of the view.
    public func zoom(by step: Int) {
        let steps = Self.zoomSteps, now = zoom
        let target = step == 0 ? 1 : step > 0 ? (steps.first { $0 > now + 0.001 } ?? steps.last!) : (steps.last { $0 < now - 0.001 } ?? steps.first!)
        let visible = scrollView.contentView.bounds
        scrollView.setMagnification(target, centeredAt: NSPoint(x: visible.midX, y: visible.midY))
        onZoom?()
    }

    // MARK: Reading position (as in kanna-v3's doc view)

    private struct ReadingPosition {
        let index: Int
        let offset: CGFloat
    }

    private func lineTop(at index: Int) -> CGFloat {
        let length = textView.textStorage?.length ?? 0
        guard length > 0 else { return 0 }
        let glyph = layoutManager.glyphIndexForCharacter(at: min(index, length - 1))
        return layoutManager.lineFragmentRect(forGlyphAt: glyph, effectiveRange: nil).minY + textView.textContainerOrigin.y
    }

    /// The character at the top of the view, and how far its line sits from the top.
    private func readingPosition() -> ReadingPosition? {
        guard let container = textView.textContainer, (textView.textStorage?.length ?? 0) > 0 else { return nil }
        let visible = scrollView.contentView.bounds
        guard visible.minY > 1 else { return nil }
        layoutManager.ensureLayout(for: container)
        let point = NSPoint(x: 0, y: max(0, visible.minY - textView.textContainerOrigin.y))
        let index = layoutManager.characterIndexForGlyph(at: layoutManager.glyphIndex(for: point, in: container))
        return ReadingPosition(index: index, offset: lineTop(at: index) - visible.minY)
    }

    private func restoreReadingPosition(_ position: ReadingPosition) {
        guard let container = textView.textContainer else { return }
        layoutManager.ensureLayout(for: container)
        textView.scroll(NSPoint(x: 0, y: max(0, lineTop(at: position.index) - position.offset)))
    }

    /// Scrolls to the heading with this anchor; false if there is none.
    @discardableResult
    public func scroll(toAnchor fragment: String) -> Bool {
        guard let storage = textView.textStorage else { return false }
        let wanted = fragment.removingPercentEncoding ?? fragment
        var found: Int?
        storage.enumerateAttribute(.kmuxHeading, in: NSRange(location: 0, length: storage.length)) { value, range, stop in
            if value as? String == wanted { found = range.location; stop.pointee = true }
        }
        guard let found, let container = textView.textContainer else { return false }
        layoutManager.ensureLayout(for: container)
        textView.scroll(NSPoint(x: 0, y: max(0, lineTop(at: found) - 8)))
        return true
    }

    // MARK: Links

    /// What a click on a link does. A document must never be able to launch
    /// anything: local files other than markdown are only revealed in Finder.
    public enum LinkAction: Equatable {
        /// Scroll to the heading with this anchor.
        case scroll(String)
        /// http, https or mailto: open in the browser or mail app.
        case openWeb(URL)
        /// A markdown file (absolute path, a regular file): show it in this pane.
        case showMarkdown(String)
        /// Any other local file or folder: select it in Finder, never open it.
        case reveal(URL)
        case ignore
    }

    /// Decides what a link in the document at `documentPath` does.
    public nonisolated static func action(for target: String, from documentPath: String) -> LinkAction {
        if target.hasPrefix("#") { return .scroll(String(target.dropFirst())) }
        if let url = URL(string: target), let scheme = url.scheme?.lowercased(), scheme != "file" {
            return ["http", "https", "mailto"].contains(scheme) ? .openWeb(url) : .ignore
        }
        // A local file, relative to this one; a #fragment after it is dropped.
        var file = target.hasPrefix("file://") ? (URL(string: target)?.path ?? "") : target
        if let hash = file.firstIndex(of: "#") { file = String(file[..<hash]) }
        file = file.removingPercentEncoding ?? file
        guard !file.isEmpty else { return .ignore }
        let url = (file.hasPrefix("/") ? URL(fileURLWithPath: file)
            : URL(fileURLWithPath: documentPath).deletingLastPathComponent().appendingPathComponent(file)).standardizedFileURL
        let real = url.resolvingSymlinksInPath()
        guard let type = (try? FileManager.default.attributesOfItem(atPath: real.path))?[.type] as? FileAttributeType else { return .ignore }
        if type == .typeRegular, ["md", "markdown", "mdown"].contains(real.pathExtension.lowercased()) {
            return .showMarkdown(real.path)
        }
        return .reveal(url)
    }

    /// Opens web links; replaceable for tests.
    var openWeb: (URL) -> Void = { NSWorkspace.shared.open($0) }
    /// Selects a file in Finder; replaceable for tests.
    var reveal: (URL) -> Void = { NSWorkspace.shared.activateFileViewerSelecting([$0]) }

    public func textView(_ textView: NSTextView, clickedOnLink link: Any, at charIndex: Int) -> Bool {
        follow((link as? URL)?.absoluteString ?? (link as? String) ?? "")
        return true
    }

    /// Follows a link as a click would.
    func follow(_ target: String) {
        switch Self.action(for: target, from: path) {
        case .scroll(let fragment): scroll(toAnchor: fragment)
        case .openWeb(let url): openWeb(url)
        case .showMarkdown(let file): onOpenMarkdown?(file)
        case .reveal(let url): reveal(url)
        case .ignore: break
        }
    }

    // MARK: For tests

    /// The rendered text, with diagrams and images shown as [diagram] and [image].
    public var text: String {
        guard let storage = textView.textStorage else { return "" }
        var out = ""
        storage.enumerateAttributes(in: NSRange(location: 0, length: storage.length)) { attributes, range, _ in
            if let attachment = attributes[.attachment] as? NSTextAttachment {
                out += attachment.attachmentCell is DiagramCell ? "[diagram]" : "[image]"
            } else {
                out += (storage.string as NSString).substring(with: range)
            }
        }
        return out
    }

    /// Each drawn diagram's type and labels, in order.
    public var drawnDiagrams: [(type: String, labels: [String])] {
        guard let storage = textView.textStorage else { return [] }
        var out: [(String, [String])] = []
        storage.enumerateAttribute(.attachment, in: NSRange(location: 0, length: storage.length)) { value, _, _ in
            if let cell = (value as? NSTextAttachment)?.attachmentCell as? DiagramCell { out.append((cell.diagramType, cell.scene.texts)) }
        }
        return out
    }

    /// The width the text is laid out for (unchanged by magnification).
    public var layoutWidth: CGFloat { textView.textContainer?.size.width ?? 0 }

    /// The scroll position's top, in points (for tests).
    public var scrollTop: CGFloat { scrollView.contentView.bounds.minY }

    /// Scrolls so `y` (points from the top of the document) is at the top.
    public func scroll(toY y: CGFloat) { textView.scroll(NSPoint(x: 0, y: max(0, y))) }
}

/// The scroll view: a two-finger swipe sideways goes back (fingers moving
/// right) or forward, as in Safari, but only once the page can't scroll any
/// further that way, so a magnified page still pans with two fingers.
final class MarkdownScrollView: NSScrollView {
    var onSwipe: ((Bool) -> Void)?

    /// Whether a sideways swipe should navigate rather than scroll: the
    /// visible part of the page is already at that edge.
    static func swipes(back: Bool, visible: NSRect, page: NSRect) -> Bool {
        back ? visible.minX <= page.minX + 1 : visible.maxX >= page.maxX - 1
    }

    override func scrollWheel(with event: NSEvent) {
        guard event.phase == .began, NSEvent.isSwipeTrackingFromScrollEventsEnabled, onSwipe != nil,
              abs(event.scrollingDeltaX) > abs(event.scrollingDeltaY), let page = documentView?.frame else {
            return super.scrollWheel(with: event)
        }
        // With natural scrolling the content follows the fingers; otherwise it's reversed.
        let back = (event.isDirectionInvertedFromDevice ? event.scrollingDeltaX : -event.scrollingDeltaX) > 0
        guard Self.swipes(back: back, visible: contentView.bounds, page: page) else { return super.scrollWheel(with: event) }
        event.trackSwipeEvent(options: [.lockDirection, .clampGestureAmount], dampenAmountThresholdMin: -1, max: 1) { [weak self] _, phase, _, _ in
            guard phase == .ended else { return }
            MainActor.assumeIsolated { self?.onSwipe?(back) }
        }
    }
}

/// The scroll view's clip view: centres the page when it is magnified to less
/// than the pane's width, and a click beside or below it focuses the pane.
private final class CenteringClipView: NSClipView {
    override func constrainBoundsRect(_ proposedBounds: NSRect) -> NSRect {
        var rect = super.constrainBoundsRect(proposedBounds)
        if let page = documentView?.frame, rect.width > page.width { rect.origin.x = (page.width - rect.width) / 2 }
        return rect
    }

    // A click there selects nothing, so it can focus the pane in an inactive window too.
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }

    override func mouseDown(with event: NSEvent) {
        window?.makeFirstResponder(documentView)
        super.mouseDown(with: event)
    }
}

/// The text view inside a markdown pane: reports focus and adds kmux's pane
/// menu to its own.
public final class MarkdownTextView: NSTextView {
    var onFocus: (() -> Void)?
    var contextMenu: (() -> NSMenu?)?
    /// The pane's unmagnified width. Magnifying shrinks the scroll view's
    /// visible width, and AppKit would narrow the text view to match and
    /// re-wrap the text; the width stays this instead.
    var layoutWidth: CGFloat = 0

    public override func setFrameSize(_ newSize: NSSize) {
        super.setFrameSize(NSSize(width: layoutWidth > 0 ? layoutWidth : newSize.width, height: newSize.height))
    }

    public override func becomeFirstResponder() -> Bool {
        let result = super.becomeFirstResponder()
        if result { onFocus?() }
        return result
    }

    public override func menu(for event: NSEvent) -> NSMenu? {
        window?.makeFirstResponder(self)
        let menu = super.menu(for: event) ?? NSMenu()
        if let extra = contextMenu?() {
            menu.addItem(.separator())
            for item in extra.items {
                extra.removeItem(item)
                menu.addItem(item)
            }
        }
        return menu
    }
}
