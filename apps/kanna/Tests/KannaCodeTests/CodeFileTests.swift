import Foundation
import Testing
@testable import KannaCode

struct CodeFileTests {
    @Test func linesStartAfterEachNewline() {
        let file = CodeFile(text: "fn main() {\n    run();\n}\n")
        #expect(file.lineStarts == [0, 12, 23])
        #expect(file.lineCount == 3)
        #expect(file.line(at: 0) == 1)
        #expect(file.line(at: 11) == 1)
        #expect(file.line(at: 12) == 2)
        #expect(file.line(at: 23) == 3)
    }

    @Test func aLastLineWithoutANewlineCounts() {
        #expect(CodeFile(text: "a\nb").lineCount == 2)
        #expect(CodeFile(text: "").lineCount == 1)
    }

    @Test func aLinePastTheEndIsTheLastLine() {
        let file = CodeFile(text: "a\nb\nc\n")
        #expect(file.start(ofLine: 99) == 4)
        #expect(file.start(ofLine: 0) == 0)
    }

    @Test func linesCountUTF16LikeTheTextView() {
        let file = CodeFile(text: "let 🦀 = 1;\nnext")
        #expect(file.lineStarts == [0, ("let 🦀 = 1;\n" as NSString).length])
    }

    @Test func changedLinesAreTheNewFilesInsertedOrEditedLines() {
        let old = CodeFile(text: "a\nb\nc\nd\n")
        #expect(CodeFile(text: "a\nB\nc\nd\n").changedLines(from: old) == [2])
        #expect(CodeFile(text: "a\nb\nnew\nc\nd\n").changedLines(from: old) == [3])
        #expect(CodeFile(text: "a\nc\nd\n").changedLines(from: old) == [])
        #expect(CodeFile(text: "a\nb\nc\nd\n").changedLines(from: old) == [])
    }

    @Test func missingFoldersAndBinariesFailWithTheirMessage() throws {
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent("kanna-code-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: folder) }
        let binary = folder.appendingPathComponent("a.bin")
        try Data([0x7f, 0x45, 0x00, 0x01]).write(to: binary)
        let text = folder.appendingPathComponent("a.rs")
        try Data("fn main() {}\n".utf8).write(to: text)
        #expect(CodeFile.read(folder.appendingPathComponent("gone.rs").path) == .failure(.missing(folder.appendingPathComponent("gone.rs").path)))
        #expect(CodeFile.read(folder.path) == .failure(.folder(folder.path)))
        #expect(CodeFile.read(binary.path) == .failure(.binary(binary.path)))
        #expect(CodeFile.read(text.path) == .success(CodeFile(text: "fn main() {}\n")))
        #expect(CodeFile.Failure.folder("/x").message == "/x is a folder, not a file")
    }
}
