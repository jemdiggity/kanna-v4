import Foundation

public enum DiffRowKind: String, Sendable, Equatable {
    case header
    case fold
    case note
    case context
    case added
    case removed
    case blank
}

public struct DiffRow: Sendable, Equatable {
    public var kind: DiffRowKind
    public var file: Int
    public var text: String
    public var old: Int?
    public var new: Int?
    public var word: NSRange?
    public var scope: String = ""
    public var at: Int?

    public var isChange: Bool { kind == .added || kind == .removed }
    public var line: Int? { kind == .removed ? old : new }
}

public enum DiffView: String, Sendable {
    case unified
    case split
}

public struct DiffDocument: Sendable, Equatable {
    public var columns: [[DiffRow]]

    public var rows: [DiffRow] { columns[0] }

    public init(result: DiffResult?, view: DiffView, closed: Set<String> = []) {
        var left: [DiffRow] = []
        var right: [DiffRow] = []
        func both(_ row: DiffRow) {
            left.append(row)
            right.append(row)
        }
        for (index, file) in (result?.files ?? []).enumerated() {
            let header = DiffRow(kind: .header, file: index, text: file.path)
            both(header)
            if closed.contains(file.path) { continue }
            if file.fold != nil {
                both(DiffRow(kind: .note, file: index, text: (file.note ?? "Not shown.") + " Show the diff"))
                continue
            }
            if file.binary == true {
                both(DiffRow(kind: .note, file: index, text: "Binary file changed."))
                continue
            }
            if file.hunks.isEmpty {
                both(DiffRow(kind: .note, file: index, text: file.status == "R" ? "Renamed from \(file.oldPath ?? "")." : "No changes in the text."))
                continue
            }
            var end = 1
            var at = 0
            for hunk in file.hunks {
                let gap = hunk.old - end
                if hunk.old > 0, gap > 0 {
                    both(DiffRow(kind: .fold, file: index, text: "⋯ \(gap) unchanged line\(gap == 1 ? "" : "s")", scope: hunk.scope))
                }
                let lines = Self.rows(hunk.lines, file: index, first: at)
                at += hunk.lines.count
                if view == .unified {
                    left += lines
                    right += lines
                } else {
                    let (l, r) = Self.split(lines, file: index)
                    left += l
                    right += r
                }
                end = hunk.oldEnd
            }
            if (file.status == "M" || file.status == "R") && file.untracked != true {
                both(DiffRow(kind: .fold, file: index, text: "⋯"))
            }
        }
        columns = view == .unified ? [left] : [left, right]
    }

    static func rows(_ lines: [DiffLine], file: Int, first: Int = 0) -> [DiffRow] {
        var rows = lines.enumerated().map { offset, line in
            DiffRow(kind: line.kind == .added ? .added : line.kind == .removed ? .removed : .context, file: file, text: line.text, old: line.old, new: line.new, at: first + offset)
        }
        var i = 0
        while i < rows.count {
            guard rows[i].kind == .removed else { i += 1; continue }
            var removed: [Int] = []
            while i < rows.count, rows[i].kind == .removed { removed.append(i); i += 1 }
            var added: [Int] = []
            while i < rows.count, rows[i].kind == .added { added.append(i); i += 1 }
            for (r, a) in zip(removed, added) {
                let (old, new) = words(rows[r].text, rows[a].text)
                rows[r].word = old
                rows[a].word = new
            }
        }
        return rows
    }

    static func split(_ rows: [DiffRow], file: Int) -> ([DiffRow], [DiffRow]) {
        var left: [DiffRow] = []
        var right: [DiffRow] = []
        var i = 0
        while i < rows.count {
            if rows[i].kind == .context {
                left.append(rows[i])
                right.append(rows[i])
                i += 1
                continue
            }
            var removed: [DiffRow] = []
            while i < rows.count, rows[i].kind == .removed { removed.append(rows[i]); i += 1 }
            var added: [DiffRow] = []
            while i < rows.count, rows[i].kind == .added { added.append(rows[i]); i += 1 }
            let blank = DiffRow(kind: .blank, file: file, text: "")
            for k in 0..<max(removed.count, added.count) {
                left.append(k < removed.count ? removed[k] : blank)
                right.append(k < added.count ? added[k] : blank)
            }
        }
        return (left, right)
    }

    static func words(_ old: String, _ new: String) -> (NSRange?, NSRange?) {
        let a = Array(old.utf16)
        let b = Array(new.utf16)
        guard a != b else { return (nil, nil) }
        var start = 0
        while start < a.count, start < b.count, a[start] == b[start] { start += 1 }
        var end = 0
        while end < a.count - start, end < b.count - start, a[a.count - 1 - end] == b[b.count - 1 - end] { end += 1 }
        guard start + end >= 3 else { return (nil, nil) }
        let oldLength = a.count - start - end
        let newLength = b.count - start - end
        return (oldLength > 0 ? NSRange(location: start, length: oldLength) : nil, newLength > 0 ? NSRange(location: start, length: newLength) : nil)
    }
}
