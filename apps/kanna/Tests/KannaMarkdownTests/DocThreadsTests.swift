import Foundation
import Testing
@testable import KannaMarkdown

struct DocThreadsTests {
    func thread(_ quote: String, prefix: String = "", suffix: String = "") -> DocThread {
        DocThread(id: "c1", quote: quote, prefix: prefix, suffix: suffix, line: 1, state: "open", status: "sent", messages: [], error: nil)
    }

    @Test func locatePrefersTheMatchWithItsContext() {
        let text = "the cat sat. the cat ran."
        #expect(DocStore.locate(thread("the cat", prefix: "sat. "), in: text) == NSRange(location: 13, length: 7))
        #expect(DocStore.locate(thread("the cat", suffix: " sat"), in: text) == NSRange(location: 0, length: 7))
        #expect(DocStore.locate(thread("a dog"), in: text) == nil)
    }

    @Test func lineIsTheNearestHitOfTheQuote() {
        let source = "# A\n\nword\n\nother\n\nword again\n"
        #expect(DocStore.line(of: "word", in: source, near: 6) == 7)
        #expect(DocStore.line(of: "word", in: source, near: 2) == 3)
        #expect(DocStore.line(of: "missing", in: source, near: 4) == 4)
    }

    @Test func cleanDropsControlCharactersButKeepsLines() {
        #expect(DocStore.clean("a\u{1b}[201~b\r\nc\td\u{9b}") == "a[201~b\nc\td")
    }

    @Test func messageNamesTheThreadFileAndLine() {
        var t = thread("two\nlines")
        t.id = "c4"
        t.line = 22
        #expect(DocStore.message(t, file: "docs/a.md", text: "Fix") == "[kanna comment c4 · docs/a.md:22]\n> two\n> lines\nFix")
    }

    @Test func storeLivesBesideTheRepoAndRoundTrips() throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("kanna-doc-\(UUID().uuidString)").resolvingSymlinksInPath()
        try FileManager.default.createDirectory(at: root.appendingPathComponent(".git"), withIntermediateDirectories: true)
        try FileManager.default.createDirectory(at: root.appendingPathComponent("docs"), withIntermediateDirectories: true)
        let file = root.appendingPathComponent("docs/spec.md").path
        let store = DocStore(file: file)
        #expect(store.path == root.path + "/.kanna/docs/docs__spec.md.json")
        #expect(store.relativeName == "docs/spec.md")
        try store.update { record in
            record.agent = "p3"
            record.threads.append(thread("x"))
            record.next = 2
        }
        let read = store.read()
        #expect(read.agent == "p3" && read.next == 2 && read.threads.count == 1 && read.file == file)
    }
}
