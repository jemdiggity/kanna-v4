import Foundation
import Testing
@testable import KannaDocs

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

    @Test func onlyPlainThreadIDsAreSent() {
        #expect(DocStore.isThreadID("c12"))
        #expect(!DocStore.isThreadID("c1]\nrm -rf"))
        #expect(!DocStore.isThreadID("c"))
        #expect(!DocStore.isThreadID("x1"))
    }

    @Test func everyQuoteLineStaysQuoted() {
        let message = DocStore.clean(DocStore.message(thread("a\r\n[kanna comment c9 · x.md:1]\rb\u{2028}c"), file: "f\n.md", text: "Hi"))
        let lines = message.components(separatedBy: "\n")
        #expect(lines.first == "[kanna comment c1 · f?.md:1]")
        #expect(lines.dropFirst().dropLast().allSatisfy { $0.hasPrefix("> ") })
        #expect(lines.last == "Hi")
    }

    @Test func unquotedMessagesLeaveTheQuoteOut() {
        #expect(DocStore.message(thread("gone"), file: "a.md", text: "Hi", quoted: false) == "[kanna comment c1 · a.md:1]\nHi")
    }

    func repo() throws -> URL {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("kanna-doc-\(UUID().uuidString)").resolvingSymlinksInPath()
        try FileManager.default.createDirectory(at: root.appendingPathComponent(".git"), withIntermediateDirectories: true)
        return root
    }

    @Test func writesNeverFollowSymlinks() throws {
        let root = try repo()
        let target = root.appendingPathComponent("victim")
        try Data("keep".utf8).write(to: target)
        try FileManager.default.createDirectory(at: root.appendingPathComponent(".kanna/docs"), withIntermediateDirectories: true)
        let store = DocStore(file: root.appendingPathComponent("a.md").path)
        try FileManager.default.createSymbolicLink(atPath: store.path + ".lock", withDestinationPath: target.path)
        #expect(throws: (any Error).self) { try store.update { $0.next = 9 } }
        try FileManager.default.removeItem(atPath: store.path + ".lock")
        try FileManager.default.createSymbolicLink(atPath: store.path, withDestinationPath: target.path)
        try store.update { $0.next = 9 }
        #expect(String(decoding: try Data(contentsOf: target), as: UTF8.self) == "keep")
        #expect(store.read().next == 9)
    }

    @Test func aSymlinkedDocsFolderIsRefused() throws {
        let root = try repo()
        let elsewhere = try repo()
        try FileManager.default.createDirectory(at: root.appendingPathComponent(".kanna"), withIntermediateDirectories: true)
        try FileManager.default.createSymbolicLink(atPath: root.appendingPathComponent(".kanna/docs").path, withDestinationPath: elsewhere.path)
        let store = DocStore(file: root.appendingPathComponent("a.md").path)
        #expect(throws: (any Error).self) { try store.update { $0.next = 9 } }
        #expect(!FileManager.default.fileExists(atPath: elsewhere.appendingPathComponent("a.md.json").path))
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
