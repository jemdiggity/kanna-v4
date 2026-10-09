import AppKit
import Testing
@testable import KmuxMarkdown

/// A document must never be able to launch anything: links to local files
/// other than markdown only reveal them in Finder.
@MainActor
struct MarkdownLinkTests {
    /// A folder with a document, a script, an app-like folder, another
    /// document and a markdown-named symlink to the script.
    private func fixture() throws -> URL {
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent("kmux-links-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: folder.appendingPathComponent("x.app"), withIntermediateDirectories: true)
        try "# Doc".write(to: folder.appendingPathComponent("doc.md"), atomically: true, encoding: .utf8)
        try "# Other".write(to: folder.appendingPathComponent("other.md"), atomically: true, encoding: .utf8)
        try "#!/bin/sh\ntouch /tmp/pwned\n".write(to: folder.appendingPathComponent("run.command"), atomically: true, encoding: .utf8)
        try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: folder.appendingPathComponent("run.command").path)
        try FileManager.default.createSymbolicLink(at: folder.appendingPathComponent("evil.md"), withDestinationURL: folder.appendingPathComponent("run.command"))
        return folder
    }

    @Test func localFilesAreRevealedNeverOpened() throws {
        let folder = try fixture()
        defer { try? FileManager.default.removeItem(at: folder) }
        let doc = folder.appendingPathComponent("doc.md").path
        let script = folder.appendingPathComponent("run.command").standardizedFileURL
        #expect(MarkdownPaneView.action(for: "run.command", from: doc) == .reveal(script))
        #expect(MarkdownPaneView.action(for: "x.app", from: doc) == .reveal(folder.appendingPathComponent("x.app").standardizedFileURL))
        // A markdown name is not enough: the symlink leads to a script.
        #expect(MarkdownPaneView.action(for: "evil.md", from: doc) == .reveal(folder.appendingPathComponent("evil.md").standardizedFileURL))
        #expect(MarkdownPaneView.action(for: "other.md#part", from: doc) == .showMarkdown(folder.appendingPathComponent("other.md").resolvingSymlinksInPath().path))
        #expect(MarkdownPaneView.action(for: "missing.md", from: doc) == .ignore)
    }

    @Test func onlyWebLinksOpen() {
        #expect(MarkdownPaneView.action(for: "https://example.com", from: "/tmp/doc.md") == .openWeb(URL(string: "https://example.com")!))
        #expect(MarkdownPaneView.action(for: "mailto:a@b.c", from: "/tmp/doc.md") == .openWeb(URL(string: "mailto:a@b.c")!))
        #expect(MarkdownPaneView.action(for: "javascript:alert(1)", from: "/tmp/doc.md") == .ignore)
        #expect(MarkdownPaneView.action(for: "x-apple.systempreferences:com.apple.preference.security", from: "/tmp/doc.md") == .ignore)
        #expect(MarkdownPaneView.action(for: "#tables", from: "/tmp/doc.md") == .scroll("tables"))
    }

    @Test func clickingAScriptLinkDoesNotOpenIt() throws {
        let folder = try fixture()
        defer { try? FileManager.default.removeItem(at: folder) }
        let pane = MarkdownPaneView(path: folder.appendingPathComponent("doc.md").path)
        defer { pane.stop() }
        var opened: [URL] = [], revealed: [URL] = [], shown: [String] = []
        pane.openWeb = { opened.append($0) }
        pane.reveal = { revealed.append($0) }
        pane.onOpenMarkdown = { shown.append($0) }
        for link in ["run.command", "file://\(folder.path)/run.command", "evil.md", "x.app"] { pane.follow(link) }
        #expect(opened.isEmpty)
        #expect(shown.isEmpty)
        #expect(revealed.count == 4)
        #expect(!FileManager.default.fileExists(atPath: "/tmp/pwned"))
    }
}
