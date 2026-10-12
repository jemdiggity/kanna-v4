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
    public var onZoom: (() -> Void)?
    public var onHistory: ((Bool) -> Void)?
    public var zoom: CGFloat { fontSize / CodeTheme.fontSize }
    public var currentLine: Int { textView.currentLine }

    let scrollView = CodeScrollView()
    let textView: CodeTextView
    let gutter: CodeGutter
    let sticky = CodeStickyView()
    private let layoutManager = NSLayoutManager()
    private var stamp: (Date, Int)?
    private var poll: Timer?
    private var fontSize = CodeTheme.fontSize
    private var scrollObserver: NSObjectProtocol?
    private var pendingLine: Int?
    public private(set) var comment: CodeComment?
    private var box: CodeCommentBox?
    private var commentUnrevealed = false

    public init(path: String, line: Int?, zoom: CGFloat = 1, comment: CodeComment? = nil) {
        self.path = path
        self.comment = comment
        fontSize = Self.clamp((CodeTheme.fontSize * zoom * 2).rounded() / 2)
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
        commentUnrevealed = comment != nil
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
        textView.isVerticallyResizable = false
        textView.isHorizontallyResizable = false
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
        scrollView.onTile = { [weak self] in self?.updateSticky() }
        textView.onMove = { [weak self] move in self?.perform(move) }
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
        placeComment()
        updateSticky()
        if let line = pendingLine, bounds.height > 0 {
            pendingLine = nil
            if comment != nil { revealComment() } else { reveal(line: line) }
        }
    }

    static let smallest: CGFloat = 9
    static let largest: CGFloat = 32

    static func clamp(_ size: CGFloat) -> CGFloat { min(max(size, smallest), largest) }

    public func zoom(by step: Int) {
        let target = step == 0 ? CodeTheme.fontSize : Self.clamp(fontSize + CGFloat(step))
        guard target != fontSize else { return }
        let top = topLine
        fontSize = target
        textView.font = CodeTheme.font(size: fontSize)
        let attributes: [NSAttributedString.Key: Any] = [.font: CodeTheme.font(size: fontSize), .foregroundColor: CodeTheme.text]
        textView.textStorage?.setAttributedString(NSAttributedString(string: file.text, attributes: attributes))
        apply(highlights)
        textView.gap = 0
        placeComment()
        sizeText()
        if let rect = textView.rect(forLine: top) {
            scrollView.contentView.scroll(to: NSPoint(x: -scrollView.contentView.contentInsets.left, y: max(0, rect.minY - textView.textContainerInset.height)))
            scrollView.reflectScrolledClipView(scrollView.contentView)
        }
        gutter.needsDisplay = true
        onZoom?()
    }

    public override func otherMouseDown(with event: NSEvent) {
        switch event.buttonNumber {
        case 3: onHistory?(true)
        case 4: onHistory?(false)
        default: super.otherMouseDown(with: event)
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
        let old = file
        load()
        gutter.changed = file.changedLines(from: old)
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
        applyComment()
        sizeText()
        highlight()
    }

    private func applyComment() {
        box?.removeFromSuperview()
        box = nil
        textView.gap = 0
        let shown = comment.flatMap { failure == nil && $0.first <= file.lineCount ? $0 : nil }
        textView.comment = shown
        gutter.comment = shown
        guard let shown else { return }
        let made = CodeCommentBox(comment: shown)
        box = made
        textView.addSubview(made)
        placeComment()
    }

    private func placeComment() {
        guard let box, let comment = textView.comment, let storage = textView.textStorage else { return }
        let width = min(CodeCommentBox.widest, max(200, bounds.width - gutter.ruleThickness - textView.textContainerInset.width - 32))
        let gap = box.height(width: width) + CodeCommentBox.margin * 2
        if gap != textView.gap {
            let start = file.start(ofLine: comment.first)
            let end = comment.first < file.lineCount ? file.start(ofLine: comment.first + 1) : storage.length
            let style = NSMutableParagraphStyle()
            style.paragraphSpacingBefore = gap
            storage.beginEditing()
            storage.removeAttribute(.paragraphStyle, range: NSRange(location: 0, length: storage.length))
            if end > start { storage.addAttribute(.paragraphStyle, value: style, range: NSRange(location: start, length: end - start)) }
            storage.endEditing()
            textView.gap = gap
            sizeText()
        }
        if let raw = textView.rawRect(forLine: comment.first) {
            box.place(at: NSPoint(x: textView.textContainerInset.width + 8, y: raw.minY + CodeCommentBox.margin), width: width)
        }
        textView.needsDisplay = true
        gutter.needsDisplay = true
    }

    public func revealComment() {
        guard let comment = textView.comment, let raw = textView.rawRect(forLine: comment.first) else { return }
        let rows = min(CodeScopes.most, CodeScopes.open(at: comment.first, in: highlights.scopes).count)
        let clip = scrollView.contentView
        let y = raw.minY - clip.contentInsets.top - CGFloat(rows) * lineHeight
        clip.scroll(to: NSPoint(x: -clip.contentInsets.left, y: max(-clip.contentInsets.top, y)))
        scrollView.reflectScrolledClipView(clip)
        textView.currentLine = comment.first
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
        if commentUnrevealed, bounds.height > 0, pendingLine == nil {
            commentUnrevealed = false
            revealComment()
        }
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
        let top = topLine
        var rows = CodeScopes.pinned(top: top, in: highlights.scopes)
        if let comment = textView.comment, top < comment.first { rows = rows.filter { $0 < comment.first } }
        let clip = scrollView.contentView
        let height = CGFloat(rows.count) * lineHeight
        let edge = convert(clip.frame, from: scrollView).maxY - clip.contentInsets.top
        sticky.frame = NSRect(x: 0, y: edge - height, width: bounds.width, height: height)
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
            let text = NSMutableAttributedString(attributedString: storage.attributedSubstring(from: range))
            text.removeAttribute(.paragraphStyle, range: NSRange(location: 0, length: text.length))
            return CodeStickyView.Row(line: line, text: text)
        }
    }

    public func reveal(line: Int, toTop: Bool) {
        guard let rect = textView.rect(forLine: CodeScopes.top(showing: line, in: highlights.scopes)) else { return }
        let clip = scrollView.contentView
        clip.scroll(to: NSPoint(x: -clip.contentInsets.left, y: rect.minY - textView.textContainerInset.height - clip.contentInsets.top))
        scrollView.reflectScrolledClipView(clip)
        textView.currentLine = min(max(line, 1), file.lineCount)
    }

    public func reveal(line: Int) {
        guard let rect = textView.rect(forLine: line) else { return }
        let clip = scrollView.contentView
        let visible = clip.bounds.height - clip.contentInsets.top
        clip.scroll(to: NSPoint(x: -clip.contentInsets.left, y: max(-clip.contentInsets.top, rect.minY - clip.contentInsets.top - visible / 3)))
        scrollView.reflectScrolledClipView(clip)
        textView.currentLine = min(max(line, 1), file.lineCount)
        textView.flash(line: textView.currentLine)
    }

    public func perform(_ move: CodeMove) {
        let clip = scrollView.contentView
        let visible = clip.bounds.height - clip.contentInsets.top
        switch move {
        case .lines(let count): scroll(by: CGFloat(count) * lineHeight)
        case .pages(let pages): scroll(by: CGFloat(pages) * visible)
        case .top: scroll(to: -clip.contentInsets.top)
        case .bottom: scroll(to: .greatestFiniteMagnitude)
        case .blank(let count):
            let lines = file.lines
            var line = anchor
            for _ in 0..<abs(count) {
                repeat { line += count > 0 ? 1 : -1 } while line > 1 && line < lines.count && !lines[line - 1].allSatisfy(\.isWhitespace)
            }
            reveal(line: min(max(line, 1), file.lineCount), toTop: true)
        case .scope(let count):
            let anchor = anchor
            let starts = Array(Set(highlights.scopes.map(\.first))).sorted()
            let after = starts.filter { $0 > anchor }, before = starts.filter { $0 < anchor }
            let target = count > 0 ? after.dropFirst(count - 1).first : before.dropLast(-count - 1).last
            if let target { reveal(line: target, toTop: true) }
        case .find(let forward):
            textView.finderAction(.showFindInterface)
            if !forward { textView.finderAction(.previousMatch) }
        case .match(let forward): textView.finderAction(forward ? .nextMatch : .previousMatch)
        case .stopFinding: textView.finderAction(.hideFindInterface)
        }
    }

    private var anchor: Int {
        let under = topLine + pinned.count
        let clip = scrollView.contentView
        guard let rect = textView.rect(forLine: textView.currentLine), textView.currentLine >= under, rect.maxY <= clip.bounds.maxY else { return under }
        return textView.currentLine
    }

    private func scroll(by delta: CGFloat) { scroll(to: scrollView.contentView.bounds.minY + delta) }

    private func scroll(to y: CGFloat) {
        let clip = scrollView.contentView
        textView.layoutManager?.ensureLayout(for: textView.textContainer!)
        let used = textView.layoutManager?.usedRect(for: textView.textContainer!).height ?? 0
        let end = max(-clip.contentInsets.top, used + textView.textContainerInset.height * 2 - clip.bounds.height)
        let highest = max(end, clip.bounds.minY)
        clip.scroll(to: NSPoint(x: clip.bounds.minX, y: min(max(y, -clip.contentInsets.top), highest)))
        scrollView.reflectScrolledClipView(clip)
    }

    public var debugNumbers: [String: Double] {
        let clip = scrollView.contentView
        return ["clip_top": Double(convert(clip.frame, from: scrollView).maxY), "height": Double(bounds.height), "sticky_top": Double(sticky.frame.maxY), "find_bar_height": Double(scrollView.findBarView?.frame.height ?? -1), "find_bar_y": Double(scrollView.findBarView.map { convert($0.frame, from: $0.superview).minY } ?? -1), "find_bar": scrollView.isFindBarVisible ? 1 : 0, "selection_line": Double(file.line(at: textView.selectedRange().location)), "selection_length": Double(textView.selectedRange().length), "zoom": Double(zoom), "font_size": Double(fontSize), "spans": Double(highlights.spans.count), "scopes": Double(highlights.scopes.count), "lines": Double(file.lineCount), "top": Double(topLine), "current": Double(currentLine), "gutter": Double(gutter.ruleThickness),
                "clip_x": Double(clip.frame.minX), "clip_width": Double(clip.frame.width), "scroll_x": Double(clip.bounds.minX),
                "scroll_y": Double(clip.bounds.minY), "changed": Double(gutter.changed.min() ?? 0), "pinned": Double(pinned.count), "indent_unit": Double(textView.indentUnit), "text_x": Double(textView.frame.minX), "text_width": Double(textView.frame.width),
                "comment_first": Double(textView.comment?.first ?? 0), "comment_last": Double(textView.comment?.last ?? 0), "comment_gap": Double(textView.gap),
                "comment_box_height": Double(box?.frame.height ?? 0), "comment_box_visible": box.map { clip.bounds.intersects($0.frame) && $0.frame.minY >= clip.bounds.minY ? 1 : 0 } ?? 0]
    }

    public var commentText: String? { box?.text }

    public var topLine: Int {
        let y = scrollView.contentView.bounds.minY + scrollView.contentView.contentInsets.top + textView.textContainerInset.height
        guard textView.textStorage?.length ?? 0 > 0 else { return 1 }
        let index = layoutManager.characterIndexForGlyph(at: layoutManager.glyphIndex(for: NSPoint(x: 0, y: y), in: textView.textContainer!))
        let line = file.line(at: index)
        if line == textView.comment?.first, let rect = textView.rect(forLine: line), y < rect.minY { return max(1, line - 1) }
        return line
    }
}

@MainActor
final class CodeScrollView: NSScrollView {
    var onTile: (() -> Void)?

    override func tile() {
        super.tile()
        onTile?()
    }
}

@MainActor
final class CodeTextView: NSTextView {
    var onFocus: (() -> Void)?
    var onCurrentLine: (() -> Void)?
    var onMove: ((CodeMove) -> Void)?
    private var keys = CodeKeys()
    var contextMenu: (() -> NSMenu?)?
    var file = CodeFile(text: "")
    var indents: [Int] = []
    var indentUnit = 4
    var currentLine = 1 { didSet { if currentLine != oldValue { needsDisplay = true; onCurrentLine?() } } }
    var comment: CodeComment? { didSet { needsDisplay = true } }
    var gap: CGFloat = 0
    private var flashing: Int?
    private var flashTask: DispatchWorkItem?

    func rect(forLine line: Int) -> NSRect? {
        guard var rect = rawRect(forLine: line) else { return nil }
        if line == comment?.first, gap > 0 {
            rect.origin.y += gap
            rect.size.height -= gap
        }
        return rect
    }

    func rawRect(forLine line: Int) -> NSRect? {
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
        if let comment, let top = self.rect(forLine: comment.first), let bottom = self.rect(forLine: comment.last) {
            CodeTheme.tint.setFill()
            NSRect(x: 0, y: top.minY, width: max(bounds.width, visibleRect.maxX), height: bottom.maxY - top.minY).intersection(rect).fill()
        }
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

    override func draw(_ dirtyRect: NSRect) {
        super.draw(dirtyRect)
        guard let comment, let top = rawRect(forLine: comment.first), let bottom = rect(forLine: comment.last) else { return }
        CodeTheme.dim.setFill()
        NSRect(x: 0, y: 0, width: bounds.width, height: top.minY).intersection(dirtyRect).fill(using: .sourceOver)
        NSRect(x: 0, y: bottom.maxY, width: bounds.width, height: max(0, bounds.height - bottom.maxY)).intersection(dirtyRect).fill(using: .sourceOver)
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

    func finderAction(_ action: NSTextFinder.Action) {
        let item = NSMenuItem()
        item.tag = action.rawValue
        performTextFinderAction(item)
    }

    override func keyDown(with event: NSEvent) {
        let modifiers = event.modifierFlags.intersection([.command, .control, .option, .shift])
        guard !modifiers.contains(.command), !modifiers.contains(.option) else { return super.keyDown(with: event) }
        if event.keyCode == 49, modifiers.contains(.shift) { return onMove?(.pages(-1)) ?? () }
        let control = modifiers.contains(.control)
        let characters = (control ? event.charactersIgnoringModifiers : event.characters) ?? ""
        if let move = keys.press(characters, control: control) { return onMove?(move) ?? () }
        if characters.count == 1, let key = characters.first, key.isLetter || key.isNumber || "[]{}/?".contains(key) { return }
        super.keyDown(with: event)
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
    var changed: Set<Int> = [] { didSet { needsDisplay = true } }
    var comment: CodeComment? { didSet { needsDisplay = true } }

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
            let commented = comment?.contains(line) == true
            if commented {
                CodeTheme.tint.setFill()
                NSRect(x: 0, y: y, width: bounds.width, height: fragment.height).fill()
            }
            if current {
                CodeTheme.currentLine.setFill()
                NSRect(x: 0, y: y, width: bounds.width, height: fragment.height).fill()
            }
            if commented {
                CodeTheme.tintEdge.setFill()
                NSRect(x: 0, y: y, width: 2, height: fragment.height).fill()
            }
            if changed.contains(line) {
                CodeTheme.changed.setFill()
                NSBezierPath(roundedRect: NSRect(x: 3, y: y + 2, width: 3, height: fragment.height - 4), xRadius: 1.5, yRadius: 1.5).fill()
            }
            let label = String(line) as NSString
            let attributes: [NSAttributedString.Key: Any] = [.font: font, .foregroundColor: current || commented ? CodeTheme.currentNumber : CodeTheme.number]
            let size = label.size(withAttributes: attributes)
            label.draw(at: NSPoint(x: bounds.width - CodeTheme.gutterPadding - size.width, y: y + (fragment.height - size.height) / 2), withAttributes: attributes)
        }
    }
}
