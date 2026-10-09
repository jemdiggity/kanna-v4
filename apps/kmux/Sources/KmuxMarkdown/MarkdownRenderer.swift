import AppKit
import KmuxDiagram
import Markdown

extension NSAttributedString.Key {
    /// A heading's anchor (its GitHub-style slug), for `#links`.
    static let kmuxHeading = NSAttributedString.Key("kmux.heading")
    /// A block drawn behind the text: a code block, a quote's bar or a rule.
    static let kmuxBlock = NSAttributedString.Key("kmux.block")
}

/// How a block is decorated behind its text (see MarkdownLayoutManager).
final class BlockDecoration: NSObject {
    enum Kind { case code, quote, rule }
    let kind: Kind
    /// One per code block or quote, so neighbours draw separately.
    let id: Int
    /// Where the decoration starts (the block's indent), in points.
    let indent: CGFloat
    init(_ kind: Kind, id: Int, indent: CGFloat) {
        self.kind = kind
        self.id = id
        self.indent = indent
    }
}

/// Mermaid layouts by source, so a reload reuses them and diagrams never flash.
@MainActor
final class DiagramCache {
    private var layouts: [String: Result<DiagramLayout, DiagramError>] = [:]
    private(set) var lastLayoutMs: Double = 0

    func layout(_ source: String) -> Result<DiagramLayout, DiagramError> {
        if let cached = layouts[source] { return cached }
        let start = Date()
        let result: Result<DiagramLayout, DiagramError>
        do { result = .success(try Diagram.layout(source: source)) } catch let error as DiagramError { result = .failure(error) } catch {
            result = .failure(DiagramError(message: "\(error)"))
        }
        lastLayoutMs = Date().timeIntervalSince(start) * 1000
        layouts[source] = result
        return result
    }

    /// Drops layouts no longer in the document.
    func keep(only sources: Set<String>) { layouts = layouts.filter { sources.contains($0.key) } }
}

/// Turns a markdown document into attributed text for a read-only TextKit
/// view: GitHub-flavoured markdown, tables as text tables, local images and
/// Mermaid diagrams as attachments.
@MainActor
struct MarkdownRenderer {
    /// The document's folder: relative images resolve against it.
    var folder: URL
    var diagrams: DiagramCache

    private var blockCount = 0
    private var slugs: [String: Int] = [:]
    private(set) var diagramSources: [String] = []

    init(folder: URL, diagrams: DiagramCache) {
        self.folder = folder
        self.diagrams = diagrams
    }

    private struct Context {
        var indent: CGFloat = 0
        var quote: BlockDecoration?
        var color: NSColor = MarkdownTheme.text
        /// Inside a list item: tighter spacing between its paragraphs.
        var tight = false
    }

    // MARK: Fonts and paragraphs

    var bodyFont: NSFont { .systemFont(ofSize: MarkdownTheme.bodySize) }
    var monoFont: NSFont { .monospacedSystemFont(ofSize: MarkdownTheme.monoSize, weight: .regular) }

    private func paragraph(_ context: Context, before: CGFloat = 0, after: CGFloat = 10, lineSpacing: CGFloat = 3,
                           headIndent: CGFloat? = nil) -> NSMutableParagraphStyle {
        let style = NSMutableParagraphStyle()
        style.lineSpacing = lineSpacing
        style.paragraphSpacingBefore = before
        style.paragraphSpacing = context.tight ? min(after, 4) : after
        style.firstLineHeadIndent = context.indent
        style.headIndent = headIndent ?? context.indent
        return style
    }

    private func attributes(_ context: Context, font: NSFont? = nil) -> [NSAttributedString.Key: Any] {
        var attributes: [NSAttributedString.Key: Any] = [.font: font ?? bodyFont, .foregroundColor: context.color]
        if let quote = context.quote { attributes[.kmuxBlock] = quote }
        return attributes
    }

    // MARK: Blocks

    mutating func render(_ document: Document) -> NSAttributedString {
        let out = NSMutableAttributedString()
        for child in document.children { block(child, Context(), into: out) }
        // No trailing paragraph break: the last block's spacing would add empty room.
        while out.length > 0, out.string.hasSuffix("\n") { out.deleteCharacters(in: NSRange(location: out.length - 1, length: 1)) }
        return out
    }

    private mutating func block(_ markup: any Markup, _ context: Context, into out: NSMutableAttributedString) {
        switch markup {
        case let heading as Heading:
            let level = min(max(heading.level, 1), 6)
            let font = NSFont.systemFont(ofSize: MarkdownTheme.headingSizes[level - 1], weight: level <= 2 ? .bold : .semibold)
            let text = inline(heading.children, attributes(context, font: font))
            let style = paragraph(context, before: level <= 2 ? 14 : 10, after: level <= 2 ? 10 : 6, lineSpacing: 2)
            text.addAttribute(.paragraphStyle, value: style, range: NSRange(location: 0, length: text.length))
            text.addAttribute(.kmuxHeading, value: slug(heading.plainText), range: NSRange(location: 0, length: text.length))
            out.append(text)
            out.append(newline(style, font, context))

        case let para as Paragraph:
            let text = inline(para.children, attributes(context))
            let style = paragraph(context)
            text.addAttribute(.paragraphStyle, value: style, range: NSRange(location: 0, length: text.length))
            out.append(text)
            out.append(newline(style, bodyFont, context))

        case let quote as BlockQuote:
            blockCount += 1
            var inner = context
            inner.indent += 16
            inner.color = MarkdownTheme.muted
            inner.quote = BlockDecoration(.quote, id: blockCount, indent: context.indent)
            for child in quote.children { block(child, inner, into: out) }

        case let list as UnorderedList:
            listItems(Array(list.listItems), ordered: nil, context, into: out)

        case let list as OrderedList:
            listItems(Array(list.listItems), ordered: Int(list.startIndex), context, into: out)

        case let code as CodeBlock:
            if code.language?.lowercased() == "mermaid" {
                diagram(code.code, context, into: out)
            } else {
                codeBlock(code.code, context, into: out)
            }

        case is ThematicBreak:
            blockCount += 1
            let style = paragraph(context, before: 6, after: 12)
            out.append(NSAttributedString(string: "\u{00A0}\n", attributes: [
                .font: bodyFont, .paragraphStyle: style, .kmuxBlock: BlockDecoration(.rule, id: blockCount, indent: context.indent),
            ]))

        case let table as Markdown.Table:
            self.table(table, context, into: out)

        case let html as HTMLBlock:
            // Raw HTML isn't interpreted; it shows as written.
            codeBlock(html.rawHTML, context, into: out)

        default:
            for child in markup.children { block(child, context, into: out) }
        }
    }

    /// A paragraph's closing line break, carrying its quote (if any) so a quote's bar is unbroken.
    private func newline(_ style: NSParagraphStyle, _ font: NSFont, _ context: Context) -> NSAttributedString {
        var attributes: [NSAttributedString.Key: Any] = [.paragraphStyle: style, .font: font]
        if let quote = context.quote { attributes[.kmuxBlock] = quote }
        return NSAttributedString(string: "\n", attributes: attributes)
    }

    private mutating func listItems(_ items: [ListItem], ordered start: Int?, _ context: Context, into out: NSMutableAttributedString) {
        let depth = Int((context.indent / 24).rounded())
        let bullets = ["•", "◦", "▪"]
        let widest = start.map { "\($0 + items.count - 1)." } ?? "☐"
        let markerWidth = max(18, (widest + " ").size(withAttributes: [.font: bodyFont]).width + 6)
        for (index, item) in items.enumerated() {
            var marker = start.map { "\($0 + index)." } ?? bullets[depth % bullets.count]
            switch item.checkbox {
            case .checked: marker = "☑"
            case .unchecked: marker = "☐"
            case nil: break
            }
            var inner = context
            inner.indent = context.indent + markerWidth
            inner.tight = true
            let children = Array(item.children)
            for (position, child) in children.enumerated() {
                let start = out.length
                block(child, inner, into: out)
                guard position == 0, child is Paragraph else { continue }
                // The marker hangs in the indent before the first paragraph.
                let style = (out.attribute(.paragraphStyle, at: start, effectiveRange: nil) as? NSParagraphStyle)?.mutableCopy() as? NSMutableParagraphStyle
                    ?? paragraph(inner)
                style.firstLineHeadIndent = context.indent
                style.tabStops = [NSTextTab(textAlignment: .left, location: inner.indent)]
                let markerText = NSAttributedString(string: marker + "\t", attributes: [
                    .font: bodyFont, .foregroundColor: item.checkbox == .checked ? MarkdownTheme.accent : MarkdownTheme.muted, .paragraphStyle: style,
                ])
                out.insert(markerText, at: start)
                let range = (out.string as NSString).paragraphRange(for: NSRange(location: start, length: 0))
                out.addAttribute(.paragraphStyle, value: style, range: range)
            }
            if children.isEmpty || !(children[0] is Paragraph) {
                // An empty item, or one starting with a block: the marker gets its own line.
                out.append(NSAttributedString(string: marker + "\n", attributes: [.font: bodyFont, .foregroundColor: MarkdownTheme.muted, .paragraphStyle: paragraph(context)]))
            }
        }
        // Space after the whole list, as after a paragraph.
        if out.length > 0, let style = (out.attribute(.paragraphStyle, at: out.length - 1, effectiveRange: nil) as? NSParagraphStyle)?.mutableCopy() as? NSMutableParagraphStyle {
            style.paragraphSpacing = context.tight ? 4 : 10
            out.addAttribute(.paragraphStyle, value: style, range: (out.string as NSString).paragraphRange(for: NSRange(location: out.length - 1, length: 0)))
        }
    }

    private mutating func codeBlock(_ code: String, _ context: Context, into out: NSMutableAttributedString) {
        blockCount += 1
        let decoration = BlockDecoration(.code, id: blockCount, indent: context.indent)
        let pad: CGFloat = 12
        var lines = code.components(separatedBy: "\n")
        if lines.count > 1, lines.last == "" { lines.removeLast() }
        for (index, line) in lines.enumerated() {
            var inner = context
            inner.indent += pad
            inner.tight = false
            let style = paragraph(inner, before: index == 0 ? 10 : 0, after: index == lines.count - 1 ? 20 : 0, lineSpacing: 2)
            style.tailIndent = -pad
            style.lineBreakMode = .byWordWrapping
            out.append(NSAttributedString(string: line + "\n", attributes: [
                .font: monoFont, .foregroundColor: MarkdownTheme.text, .paragraphStyle: style, .kmuxBlock: decoration,
            ]))
        }
    }

    private mutating func diagram(_ source: String, _ context: Context, into out: NSMutableAttributedString) {
        diagramSources.append(source)
        switch diagrams.layout(source) {
        case .success(let layout) where layout.scene != nil:
            let attachment = NSTextAttachment()
            attachment.attachmentCell = DiagramCell(scene: layout.scene!, type: layout.type)
            let text = NSMutableAttributedString(attachment: attachment)
            let style = paragraph(context, before: 6, after: 14)
            style.alignment = .center
            text.addAttributes([.paragraphStyle: style, .font: bodyFont], range: NSRange(location: 0, length: text.length))
            out.append(text)
            out.append(NSAttributedString(string: "\n", attributes: [.paragraphStyle: style, .font: bodyFont]))
        case .success(let layout):
            note("Unsupported diagram type: \(layout.type)", color: MarkdownTheme.muted, context, into: out)
            codeBlock(source, context, into: out)
        case .failure(let error):
            note("Diagram error: \(error.message)", color: MarkdownTheme.bad, context, into: out)
            codeBlock(source, context, into: out)
        }
    }

    private func note(_ text: String, color: NSColor, _ context: Context, into out: NSMutableAttributedString) {
        out.append(NSAttributedString(string: text + "\n", attributes: [
            .font: NSFont.systemFont(ofSize: 12.5, weight: .medium), .foregroundColor: color, .paragraphStyle: paragraph(context, after: 0),
        ]))
    }

    private mutating func table(_ table: Markdown.Table, _ context: Context, into out: NSMutableAttributedString) {
        let grid = NSTextTable()
        grid.numberOfColumns = max(1, table.maxColumnCount)
        grid.layoutAlgorithm = .automaticLayoutAlgorithm
        grid.collapsesBorders = true
        grid.hidesEmptyCells = false
        let rows: [[Markdown.Table.Cell]] = [Array(table.head.cells)] + table.body.rows.map { Array($0.cells) }
        let alignments = table.columnAlignments
        for (rowIndex, row) in rows.enumerated() {
            for (column, cell) in row.enumerated() {
                let block = NSTextTableBlock(table: grid, startingRow: rowIndex, rowSpan: 1, startingColumn: column, columnSpan: 1)
                block.setWidth(1, type: .absoluteValueType, for: .border)
                block.setBorderColor(MarkdownTheme.line)
                block.setWidth(6, type: .absoluteValueType, for: .padding, edge: .minY)
                block.setWidth(6, type: .absoluteValueType, for: .padding, edge: .maxY)
                block.setWidth(12, type: .absoluteValueType, for: .padding, edge: .minX)
                block.setWidth(12, type: .absoluteValueType, for: .padding, edge: .maxX)
                if rowIndex == 0 { block.backgroundColor = MarkdownTheme.panel }
                let style = NSMutableParagraphStyle()
                style.textBlocks = [block]
                style.lineSpacing = 2
                switch column < alignments.count ? alignments[column] : nil {
                case .center: style.alignment = .center
                case .right: style.alignment = .right
                default: style.alignment = .left
                }
                let font = rowIndex == 0 ? NSFontManager.shared.convert(bodyFont, toHaveTrait: .boldFontMask) : bodyFont
                let text = inline(cell.children, attributes(context, font: font))
                text.append(NSAttributedString(string: "\n", attributes: [.font: font]))
                text.addAttribute(.paragraphStyle, value: style, range: NSRange(location: 0, length: text.length))
                out.append(text)
            }
        }
        // Room after the table, as after a paragraph.
        out.append(NSAttributedString(string: "\n", attributes: [.font: NSFont.systemFont(ofSize: 4), .paragraphStyle: paragraph(context, after: 6)]))
    }

    // MARK: Inline

    private func inline(_ children: some Sequence<any Markup>, _ base: [NSAttributedString.Key: Any]) -> NSMutableAttributedString {
        let out = NSMutableAttributedString()
        for child in children { inline(child, base, into: out) }
        return out
    }

    private func inline(_ markup: any Markup, _ base: [NSAttributedString.Key: Any], into out: NSMutableAttributedString) {
        var attributes = base
        let font = base[.font] as? NSFont ?? bodyFont
        switch markup {
        case let text as Markdown.Text:
            out.append(NSAttributedString(string: text.string, attributes: base))
            return
        case is SoftBreak:
            out.append(NSAttributedString(string: " ", attributes: base))
            return
        case is LineBreak:
            out.append(NSAttributedString(string: "\u{2028}", attributes: base))
            return
        case let code as InlineCode:
            attributes[.font] = NSFont.monospacedSystemFont(ofSize: font.pointSize * 0.9, weight: .regular)
            attributes[.backgroundColor] = MarkdownTheme.panel2
            out.append(NSAttributedString(string: code.code, attributes: attributes))
            return
        case let html as InlineHTML:
            out.append(NSAttributedString(string: html.rawHTML, attributes: base))
            return
        case let image as Markdown.Image:
            if let attachment = imageAttachment(image.source) {
                out.append(NSAttributedString(attachment: attachment))
            } else {
                attributes[.foregroundColor] = MarkdownTheme.muted
                if let source = image.source { attributes[.link] = source }
                out.append(NSAttributedString(string: image.plainText.isEmpty ? (image.source ?? "image") : image.plainText, attributes: attributes))
            }
            return
        case is Emphasis:
            attributes[.font] = NSFontManager.shared.convert(font, toHaveTrait: .italicFontMask)
        case is Strong:
            attributes[.font] = NSFontManager.shared.convert(font, toHaveTrait: .boldFontMask)
        case is Strikethrough:
            attributes[.strikethroughStyle] = NSUnderlineStyle.single.rawValue
        case let link as Markdown.Link:
            if let destination = link.destination { attributes[.link] = destination }
        default:
            break
        }
        for child in markup.children { inline(child, attributes, into: out) }
    }

    private func imageAttachment(_ source: String?) -> NSTextAttachment? {
        guard let source, !source.contains("://") || source.hasPrefix("file://") else { return nil }
        let path = source.removingPercentEncoding ?? source
        let url = path.hasPrefix("file://") ? URL(string: path) : path.hasPrefix("/") ? URL(fileURLWithPath: path) : folder.appendingPathComponent(path)
        guard let url, let image = NSImage(contentsOf: url) else { return nil }
        let attachment = NSTextAttachment()
        attachment.attachmentCell = ImageCell(image: image)
        return attachment
    }

    // MARK: Anchors

    /// GitHub's heading anchors: lower case, punctuation dropped, spaces to
    /// hyphens, repeats numbered (as in kanna-v3).
    private mutating func slug(_ text: String) -> String {
        let base = Self.slugBase(text)
        let count = slugs[base, default: 0]
        slugs[base] = count + 1
        return count == 0 ? base : "\(base)-\(count)"
    }

    static func slugBase(_ text: String) -> String {
        text.lowercased().replacingOccurrences(of: #"[^\p{L}\p{M}\p{N} _-]"#, with: "", options: .regularExpression)
            .replacingOccurrences(of: " ", with: "-")
    }
}
