import AppKit

@MainActor
public final class CodePaneView: NSView {
    public var onFocus: (() -> Void)? { didSet { textView.onFocus = onFocus } }
    public var contextMenu: (() -> NSMenu?)? { didSet { textView.contextMenu = contextMenu } }
    public let path: String
    public var keyView: NSView { textView }
    public private(set) var file = CodeFile(text: "")
    public private(set) var failure: CodeFile.Failure?
    public private(set) var language: String?
    public private(set) var highlights = CodeHighlights()
    public var onHighlighted: (() -> Void)?
    public var currentLine: Int { textView.currentLine }

    let scrollView = NSScrollView()
    let textView: CodeTextView
    let gutter: CodeGutter
    let sticky = CodeStickyView()
    private let layoutManager = NSLayoutManager()
    private var stamp: (Date, Int)?
    private var poll: Timer?
    private var fontSize = CodeTheme.fontSize
    private var scrollObserver: NSObjectProtocol?
    private var pendingLine: Int?

    public init(path: String, line: Int?) {
        self.path = path
        let storage = NSTextStorage()
        storage.addLayoutManager(layoutManager)
        let container = NSTextContainer(size: NSSize(width: CGFloat.greatestFiniteMagnitude, height: CGFloat.greatestFiniteMagnitude))
        container.widthTracksTextView = false
        container.lineFragmentPadding = 0
        layoutManager.addTextContainer(container)
        layoutManager.allowsNonContiguousLayout = true
        textView = CodeTextView(frame: NSRect(x: 0, y: 0, width: 600, height: 400), textContainer: container)
        gutter = CodeGutter(scrollView: scrollView, orientation: .verticalRuler)
        super.init(frame: .zero)
        configure()
        gutter.textView = textView
        load()
        pendingLine = line
        if let line { textView.currentLine = min(max(line, 1), file.lineCount) }
        poll = Timer.scheduledTimer(withTimeInterval: 0.5, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated { self?.reloadIfChanged() }
        }
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    private func configure() {
        textView.isEditable = false
        textView.isSelectable = true
        textView.isRichText = false
        textView.importsGraphics = false
        textView.drawsBackground = true
        textView.backgroundColor = CodeTheme.background
        textView.textColor = CodeTheme.text
        textView.font = CodeTheme.font(size: fontSize)
        textView.isVerticallyResizable = true
        textView.isHorizontallyResizable = true
        textView.minSize = .zero
        textView.maxSize = NSSize(width: CGFloat.greatestFiniteMagnitude, height: CGFloat.greatestFiniteMagnitude)
        textView.autoresizingMask = []
        textView.textContainerInset = NSSize(width: 0, height: 8)
        textView.usesFindBar = true
        textView.isIncrementalSearchingEnabled = true
        textView.selectedTextAttributes = [.backgroundColor: NSColor.selectedTextBackgroundColor]
        textView.onCurrentLine = { [weak self] in self?.gutter.needsDisplay = true }
        scrollView.documentView = textView
        scrollView.hasVerticalScroller = true
        scrollView.hasHorizontalScroller = true
        scrollView.autohidesScrollers = true
        scrollView.drawsBackground = true
        scrollView.backgroundColor = CodeTheme.background
        scrollView.verticalRulerView = gutter
        scrollView.hasVerticalRuler = true
        scrollView.rulersVisible = true
        scrollView.contentView.postsBoundsChangedNotifications = true
        scrollObserver = NotificationCenter.default.addObserver(forName: NSView.boundsDidChangeNotification, object: scrollView.contentView, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated {
                self?.gutter.needsDisplay = true
                self?.updateSticky()
            }
        }
        addSubview(scrollView)
        sticky.onPick = { [weak self] line in self?.reveal(line: line, toTop: true) }
        addSubview(sticky)
    }

    public func stop() {
        poll?.invalidate()
        poll = nil
        scrollObserver.map(NotificationCenter.default.removeObserver)
        scrollObserver = nil
    }

    public override func layout() {
        super.layout()
        scrollView.frame = bounds
        sizeText()
        updateSticky()
        if let line = pendingLine, bounds.height > 0 {
            pendingLine = nil
            reveal(line: line)
        }
    }

    private func fileStamp() -> (Date, Int)? {
        guard let attributes = try? FileManager.default.attributesOfItem(atPath: path) else { return nil }
        return ((attributes[.modificationDate] as? Date) ?? .distantPast, (attributes[.size] as? Int) ?? 0)
    }

    private func reloadIfChanged() {
        guard let now = fileStamp() else { return }
        if let stamp, stamp == now { return }
        let top = scrollView.contentView.bounds.origin
        load()
        scrollView.contentView.scroll(to: top)
        scrollView.reflectScrolledClipView(scrollView.contentView)
    }

    private func load() {
        stamp = fileStamp()
        switch CodeFile.read(path) {
        case .success(let read):
            failure = nil
            file = read
        case .failure(let error):
            failure = error
            file = CodeFile(text: "")
        }
        textView.file = file
        textView.indents = CodeScopes.indents(file)
        textView.indentUnit = CodeScopes.unit(textView.indents)
        let attributes: [NSAttributedString.Key: Any] = [.font: CodeTheme.font(size: fontSize), .foregroundColor: CodeTheme.text]
        textView.textStorage?.setAttributedString(NSAttributedString(string: file.text, attributes: attributes))
        gutter.file = file
        sizeText()
        highlight()
    }

    private func highlight() {
        let text = file.text
        let firstLine = String(text.prefix { $0 != "\n" })
        language = CodeSyntax.language(for: path, firstLine: firstLine)
        highlights = CodeHighlights()
        guard let language else { return }
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            let found = CodeSyntax.highlight(text, language: language)
            DispatchQueue.main.async {
                guard let self, self.file.text == text else { return }
                self.apply(found)
            }
        }
    }

    private func apply(_ found: CodeHighlights) {
        highlights = found
        guard let storage = textView.textStorage else { return }
        let length = storage.length
        let italic = CodeTheme.italic(size: fontSize)
        storage.beginEditing()
        for span in found.spans where NSMaxRange(span.range) <= length {
            guard let color = CodeTheme.color(capture: span.capture) else { continue }
            storage.addAttribute(.foregroundColor, value: color, range: span.range)
            if span.capture.hasPrefix("comment") { storage.addAttribute(.font, value: italic, range: span.range) }
        }
        storage.endEditing()
        updateSticky()
        onHighlighted?()
    }

    private func sizeText() {
        guard let container = textView.textContainer else { return }
        layoutManager.ensureLayout(for: container)
        let used = layoutManager.usedRect(for: container)
        let visible = scrollView.contentSize
        let lineHeight = layoutManager.defaultLineHeight(for: CodeTheme.font(size: fontSize))
        let height = max(visible.height, used.height + textView.textContainerInset.height * 2 + max(0, visible.height - lineHeight * 3))
        textView.setFrameSize(NSSize(width: max(visible.width, used.width + 24), height: height))
        let thickness = gutter.thickness(font: CodeTheme.font(size: fontSize))
        if gutter.ruleThickness != thickness {
            gutter.ruleThickness = thickness
            scrollView.tile()
        }
    }

    var lineHeight: CGFloat { layoutManager.defaultLineHeight(for: CodeTheme.font(size: fontSize)) }

    public var pinned: [Int] { sticky.rows.map(\.line) }

    func updateSticky() {
        let rows = CodeScopes.pinned(top: topLine, in: highlights.scopes)
        let clip = scrollView.contentView
        let height = CGFloat(rows.count) * lineHeight
        sticky.frame = NSRect(x: 0, y: bounds.height - height, width: max(0, bounds.width - (scrollView.verticalScroller?.frame.width ?? 0)), height: height)
        sticky.isHidden = rows.isEmpty
        sticky.gutterWidth = gutter.ruleThickness
        sticky.textX = clip.frame.minX - clip.bounds.minX + textView.textContainerInset.width
        sticky.lineHeight = lineHeight
        sticky.font = CodeTheme.font(size: fontSize)
        guard let storage = textView.textStorage else { return }
        sticky.rows = rows.compactMap { line in
            let start = file.start(ofLine: line)
            let end = line < file.lineCount ? file.start(ofLine: line + 1) : storage.length
            guard start <= end, end <= storage.length else { return nil }
            var range = NSRange(location: start, length: end - start)
            if range.length > 0, (storage.string as NSString).character(at: NSMaxRange(range) - 1) == 0x0a { range.length -= 1 }
            return CodeStickyView.Row(line: line, text: storage.attributedSubstring(from: range))
        }
    }

    public func reveal(line: Int, toTop: Bool) {
        guard let rect = textView.rect(forLine: CodeScopes.top(showing: line, in: highlights.scopes)) else { return }
        scrollView.contentView.scroll(to: NSPoint(x: -scrollView.contentView.contentInsets.left, y: max(0, rect.minY - textView.textContainerInset.height)))
        scrollView.reflectScrolledClipView(scrollView.contentView)
        textView.currentLine = min(max(line, 1), file.lineCount)
    }

    public func reveal(line: Int) {
        guard let rect = textView.rect(forLine: line) else { return }
        let target = max(0, rect.minY - scrollView.contentSize.height / 3)
        scrollView.contentView.scroll(to: NSPoint(x: -scrollView.contentView.contentInsets.left, y: target))
        scrollView.reflectScrolledClipView(scrollView.contentView)
        textView.currentLine = min(max(line, 1), file.lineCount)
        textView.flash(line: textView.currentLine)
    }

    public var debugNumbers: [String: Double] {
        let clip = scrollView.contentView
        return ["spans": Double(highlights.spans.count), "scopes": Double(highlights.scopes.count), "lines": Double(file.lineCount), "top": Double(topLine), "current": Double(currentLine), "gutter": Double(gutter.ruleThickness),
                "clip_x": Double(clip.frame.minX), "clip_width": Double(clip.frame.width), "scroll_x": Double(clip.bounds.minX),
                "scroll_y": Double(clip.bounds.minY), "pinned": Double(pinned.count), "indent_unit": Double(textView.indentUnit), "text_x": Double(textView.frame.minX), "text_width": Double(textView.frame.width)]
    }

    public var topLine: Int {
        let y = scrollView.contentView.bounds.minY + textView.textContainerInset.height
        guard textView.textStorage?.length ?? 0 > 0 else { return 1 }
        let index = layoutManager.characterIndexForGlyph(at: layoutManager.glyphIndex(for: NSPoint(x: 0, y: y), in: textView.textContainer!))
        return file.line(at: index)
    }
}

@MainActor
final class CodeTextView: NSTextView {
    var onFocus: (() -> Void)?
    var onCurrentLine: (() -> Void)?
    var contextMenu: (() -> NSMenu?)?
    var file = CodeFile(text: "")
    var indents: [Int] = []
    var indentUnit = 4
    var currentLine = 1 { didSet { if currentLine != oldValue { needsDisplay = true; onCurrentLine?() } } }
    private var flashing: Int?
    private var flashTask: DispatchWorkItem?

    func rect(forLine line: Int) -> NSRect? {
        guard let manager = layoutManager else { return nil }
        let length = textStorage?.length ?? 0
        guard length > 0 else { return NSRect(x: 0, y: textContainerInset.height, width: bounds.width, height: 0) }
        let start = min(file.start(ofLine: line), length - 1)
        let glyph = manager.glyphIndexForCharacter(at: start)
        var rect = manager.lineFragmentRect(forGlyphAt: glyph, effectiveRange: nil)
        rect.origin.y += textContainerInset.height
        rect.origin.x = 0
        rect.size.width = max(bounds.width, visibleRect.maxX)
        return rect
    }

    func flash(line: Int) {
        flashing = line
        needsDisplay = true
        flashTask?.cancel()
        let task = DispatchWorkItem { [weak self] in
            self?.flashing = nil
            self?.needsDisplay = true
        }
        flashTask = task
        DispatchQueue.main.asyncAfter(deadline: .now() + 1.1, execute: task)
    }

    override func drawBackground(in rect: NSRect) {
        super.drawBackground(in: rect)
        if let line = self.rect(forLine: currentLine), line.intersects(rect) {
            CodeTheme.currentLine.setFill()
            line.fill()
        }
        if let flashing, let line = self.rect(forLine: flashing), line.intersects(rect) {
            CodeTheme.flash.setFill()
            line.fill()
        }
        drawGuides(in: rect)
    }

    private func drawGuides(in rect: NSRect) {
        guard let manager = layoutManager, let container = textContainer, !indents.isEmpty, (textStorage?.length ?? 0) > 0 else { return }
        let width = ("0" as NSString).size(withAttributes: [.font: font ?? CodeTheme.font(size: CodeTheme.fontSize)]).width
        let glyphs = manager.glyphRange(forBoundingRect: rect, in: container)
        let characters = manager.characterRange(forGlyphRange: glyphs, actualGlyphRange: nil)
        let first = file.line(at: characters.location)
        let last = file.line(at: max(characters.location, NSMaxRange(characters) - 1))
        CodeTheme.guide.setFill()
        for line in first...max(first, last) where line <= indents.count {
            guard let fragment = self.rect(forLine: line) else { continue }
            var column = 0
            while column < indents[line - 1] {
                NSRect(x: textContainerInset.width + CGFloat(column) * width, y: fragment.minY, width: 1, height: fragment.height).fill()
                column += indentUnit
            }
        }
    }

    override func mouseDown(with event: NSEvent) {
        super.mouseDown(with: event)
        if selectedRange().length == 0 { currentLine = file.line(at: selectedRange().location) }
    }

    override func becomeFirstResponder() -> Bool {
        let result = super.becomeFirstResponder()
        if result { onFocus?() }
        return result
    }

    override func menu(for event: NSEvent) -> NSMenu? {
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

@MainActor
final class CodeGutter: NSRulerView {
    weak var textView: CodeTextView?
    var file = CodeFile(text: "") { didSet { needsDisplay = true } }

    override init(scrollView: NSScrollView?, orientation: NSRulerView.Orientation) {
        super.init(scrollView: scrollView, orientation: orientation)
        clientView = nil
    }

    required init(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    override var isFlipped: Bool { true }
    override var requiredThickness: CGFloat { ruleThickness }

    func thickness(font: NSFont) -> CGFloat {
        let digits = max(3, String(file.lineCount).count)
        let width = ("0" as NSString).size(withAttributes: [.font: font]).width
        return ceil(width * CGFloat(digits) + CodeTheme.gutterPadding * 2)
    }

    override func draw(_ dirtyRect: NSRect) {
        CodeTheme.background.setFill()
        bounds.fill()
        drawHashMarksAndLabels(in: dirtyRect)
    }

    override func drawHashMarksAndLabels(in rect: NSRect) {
        guard let textView, let manager = textView.layoutManager, let container = textView.textContainer, let storage = textView.textStorage else { return }
        let font = textView.font ?? CodeTheme.font(size: CodeTheme.fontSize)
        let visible = textView.visibleRect
        let glyphs = manager.glyphRange(forBoundingRect: visible, in: container)
        let characters = manager.characterRange(forGlyphRange: glyphs, actualGlyphRange: nil)
        let first = file.line(at: characters.location)
        let last = storage.length == 0 ? 1 : file.line(at: max(characters.location, NSMaxRange(characters) - 1))
        for line in first...max(first, last) {
            guard let fragment = textView.rect(forLine: line) else { continue }
            let y = convert(NSPoint(x: 0, y: fragment.minY), from: textView).y
            let current = line == textView.currentLine
            if current {
                CodeTheme.currentLine.setFill()
                NSRect(x: 0, y: y, width: bounds.width, height: fragment.height).fill()
            }
            let label = String(line) as NSString
            let attributes: [NSAttributedString.Key: Any] = [.font: font, .foregroundColor: current ? CodeTheme.currentNumber : CodeTheme.number]
            let size = label.size(withAttributes: attributes)
            label.draw(at: NSPoint(x: bounds.width - CodeTheme.gutterPadding - size.width, y: y + (fragment.height - size.height) / 2), withAttributes: attributes)
        }
    }
}
