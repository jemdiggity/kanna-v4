import Foundation

public enum CodeScopes {
    public static let most = 5

    public static func open(at line: Int, in scopes: [CodeScope]) -> [Int] {
        var out: [Int] = []
        for scope in scopes {
            if scope.first >= line { break }
            if scope.last >= line, out.last != scope.first { out.append(scope.first) }
        }
        return out
    }

    public static func pinned(top: Int, in scopes: [CodeScope], most: Int = most) -> [Int] {
        var rows: [Int] = []
        guard top > 1 else { return rows }
        for count in 1...most {
            let open = Self.open(at: top + count, in: scopes)
            guard open.count >= count else { continue }
            let candidate = Array(open.prefix(count))
            if candidate.enumerated().allSatisfy({ $0.element < top + $0.offset }) { rows = candidate }
        }
        return rows
    }

    public static func top(showing line: Int, in scopes: [CodeScope]) -> Int {
        var top = line
        while top > max(1, line - most - 1) {
            if top + pinned(top: top, in: scopes).count <= line { return top }
            top -= 1
        }
        return top
    }

    public static func indents(_ file: CodeFile, tab: Int = 4) -> [Int] {
        let units = Array(file.text.utf16)
        var columns: [Int?] = []
        for (index, start) in file.lineStarts.enumerated() {
            let end = index + 1 < file.lineStarts.count ? file.lineStarts[index + 1] : units.count
            var column = 0
            var blank = true
            for unit in units[start..<end] {
                if unit == 0x20 { column += 1 } else if unit == 0x09 { column += tab - column % tab } else {
                    blank = unit == 0x0a || unit == 0x0d
                    break
                }
            }
            columns.append(blank ? nil : column)
        }
        var next = 0
        var out = Array(repeating: 0, count: columns.count)
        for index in columns.indices.reversed() {
            if let column = columns[index] { next = column }
            out[index] = columns[index] ?? next
        }
        return out
    }

    public static func unit(_ indents: [Int]) -> Int {
        var steps: [Int: Int] = [:]
        var previous = 0
        for column in indents {
            if column > previous { steps[column - previous, default: 0] += 1 }
            previous = column
        }
        return steps.filter { $0.key >= 2 && $0.key <= 8 }.max { $0.value < $1.value }?.key ?? 4
    }
}
