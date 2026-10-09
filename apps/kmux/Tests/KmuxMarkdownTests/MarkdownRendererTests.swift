import AppKit
import Markdown
import Testing
@testable import KmuxMarkdown

@MainActor
struct MarkdownRendererTests {
    private func render(_ source: String) -> NSAttributedString {
        var renderer = MarkdownRenderer(folder: URL(fileURLWithPath: "/tmp"), diagrams: DiagramCache())
        return renderer.render(Document(parsing: source))
    }

    private func attachments(_ text: NSAttributedString) -> [NSTextAttachmentCell] {
        var cells: [NSTextAttachmentCell] = []
        text.enumerateAttribute(.attachment, in: NSRange(location: 0, length: text.length)) { value, _, _ in
            if let cell = (value as? NSTextAttachment)?.attachmentCell as? NSTextAttachmentCell { cells.append(cell) }
        }
        return cells
    }

    @Test func blocksBecomeTextWithoutMarkdownSyntax() {
        let text = render("# Title\n\nSome **bold** and `code`.\n\n- one\n- [x] done\n\n1. first\n2. second\n\n> quoted\n").string
        #expect(text.contains("Title\nSome bold and code."))
        #expect(text.contains("•\tone"))
        #expect(text.contains("☑\tdone"))
        #expect(text.contains("1.\tfirst") && text.contains("2.\tsecond"))
        #expect(text.contains("quoted"))
        #expect(!text.contains("**") && !text.contains("`") && !text.contains("> "))
    }

    @Test func headingsGetGitHubAnchors() {
        let text = render("# Hello, World!\n\n## Hello, World!\n")
        var slugs: [String] = []
        text.enumerateAttribute(.kmuxHeading, in: NSRange(location: 0, length: text.length)) { value, _, _ in
            if let slug = value as? String, slugs.last != slug { slugs.append(slug) }
        }
        #expect(slugs == ["hello-world", "hello-world-1"])
    }

    @Test func mermaidIsDrawnNotShownAsSource() {
        let text = render("```mermaid\nflowchart LR\n  A[cart] --> B[paid]\n```\n")
        let diagrams = attachments(text).compactMap { $0 as? DiagramCell }
        #expect(diagrams.count == 1)
        #expect(diagrams.first?.scene.texts.contains("cart") == true)
        #expect(!text.string.contains("flowchart"))
    }

    @Test func unsupportedAndInvalidDiagramsShowTheirSource() {
        let pie = render("```mermaid\npie\n  \"a\": 1\n```\n").string
        #expect(pie.contains("Unsupported diagram type: pie") && pie.contains("\"a\": 1"))
        let broken = render("```mermaid\nflowchart LR\n  A -->\n```\n").string
        #expect(broken.contains("Diagram error:") && broken.contains("A -->"))
    }

    @Test func tablesAreTextTables() {
        let text = render("| a | b |\n|---|--:|\n| 1 | 2 |\n")
        let style = text.attribute(.paragraphStyle, at: (text.string as NSString).range(of: "2").location, effectiveRange: nil) as? NSParagraphStyle
        let block = style?.textBlocks.first as? NSTextTableBlock
        #expect(block?.table.numberOfColumns == 2)
        #expect(block?.startingRow == 1 && block?.startingColumn == 1)
        #expect(style?.alignment == .right)
    }

    @Test func zoomStepsFollowSafari() {
        #expect(MarkdownPaneView.zoomSteps.first == 0.5 && MarkdownPaneView.zoomSteps.last == 3 && MarkdownPaneView.zoomSteps.contains(1.25))
    }
}
