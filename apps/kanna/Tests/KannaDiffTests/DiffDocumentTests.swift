import Foundation
import Testing
@testable import KannaDiff

struct DiffDocumentTests {
    let json = """
    {"folder":"/r","args":["--merge-base","main"],"command":"git diff --merge-base main","top":"/r","added":4,"removed":2,"error":null,
     "files":[
      {"path":"Cargo.lock","status":"M","added":1,"removed":0,"hunks":[],"fold":"lock","note":"Cargo.lock is a lock file: not shown."},
      {"path":"src/a.rs","status":"M","added":2,"removed":2,"hunks":[
        {"old":4,"new":4,"scope":"fn main() {","lines":[[" ","a",4,4],["-","let x = 1;",5,null],["-","gone();",6,null],["+","let x = 2;",null,5],[" ","b",7,6]]},
        {"old":20,"new":19,"scope":"","lines":[["+","added()",null,19]]}]},
      {"path":"b.rs","status":"A","added":1,"removed":0,"untracked":true,"hunks":[{"old":0,"new":1,"scope":"","lines":[["+","new",null,1]]}]}]}
    """

    var result: DiffResult { DiffResult.decode(Data(json.utf8))! }

    @Test func unifiedRowsHaveHeadersFoldsAndEveryLine() {
        let rows = DiffDocument(result: result, view: .unified).rows
        #expect(rows.map(\.kind) == [.header, .note, .header, .fold, .context, .removed, .removed, .added, .context, .fold, .added, .fold, .header, .added])
        #expect(rows[1].text == "Cargo.lock is a lock file: not shown. Show the diff")
        #expect(rows[3].text == "⋯ 3 unchanged lines" && rows[3].scope == "fn main() {")
        #expect(rows[9].text == "⋯ 12 unchanged lines")
        #expect(rows.map(\.at) == [nil, nil, nil, nil, 0, 1, 2, 3, 4, nil, 5, nil, nil, 0])
    }

    @Test func aChangedLinesChangedWordsAreMarked() {
        let rows = DiffDocument(result: result, view: .unified).rows
        #expect(rows[5].word == NSRange(location: 8, length: 1))
        #expect(rows[7].word == NSRange(location: 8, length: 1))
        #expect(rows[6].word == nil)
        #expect(DiffDocument.words("abc", "xyz") == (nil, nil))
    }

    @Test func splitPairsRemovedWithAddedAndFillsTheShortSideWithBlanks() {
        let document = DiffDocument(result: result, view: .split)
        #expect(document.columns.count == 2)
        let (left, right) = (document.columns[0], document.columns[1])
        #expect(left.count == right.count)
        #expect(left[5...6].map(\.kind) == [.removed, .removed])
        #expect(right[5...6].map(\.kind) == [.added, .blank])
        #expect(left[9].kind == .blank && right[9].kind == .added)
    }

    @Test func aClosedFileIsItsHeaderAlone() {
        let rows = DiffDocument(result: result, view: .unified, closed: ["src/a.rs"]).rows
        #expect(rows.map(\.kind) == [.header, .note, .header, .header, .added])
    }

    @Test func separatorsInALineBecomeOneVisibleCharacter() {
        let line = "a\r\u{2028}b"
        let shown = DiffPaneView.oneLine(line)
        #expect(shown == "a\u{2424}\u{2424}b")
        #expect((shown as NSString).length == (line as NSString).length)
    }
}
