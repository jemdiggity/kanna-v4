import Foundation

public struct CodeFile: Equatable {
    public static let sniffBytes = 8192

    public let text: String
    public let lineStarts: [Int]

    public init(text: String) {
        self.text = text
        var starts = [0]
        var offset = 0
        for unit in text.utf16 {
            offset += 1
            if unit == 0x0a { starts.append(offset) }
        }
        if starts.count > 1, starts.last == offset { starts.removeLast() }
        lineStarts = starts
    }

    public var lineCount: Int { lineStarts.count }

    public func line(at offset: Int) -> Int {
        var low = 0
        var high = lineStarts.count - 1
        while low < high {
            let mid = (low + high + 1) / 2
            if lineStarts[mid] <= offset { low = mid } else { high = mid - 1 }
        }
        return low + 1
    }

    public func start(ofLine line: Int) -> Int { lineStarts[min(max(line, 1), lineCount) - 1] }

    public var lines: [Substring] { text.split(separator: "\n", omittingEmptySubsequences: false) }

    public func changedLines(from old: CodeFile) -> Set<Int> {
        var changed = Set<Int>()
        for change in lines.difference(from: old.lines) {
            if case .insert(let offset, _, _) = change { changed.insert(offset + 1) }
        }
        return changed.filter { $0 <= lineCount }
    }

    public enum Failure: Error, Equatable {
        case missing(String)
        case folder(String)
        case binary(String)

        public var message: String {
            switch self {
            case .missing(let path): "No such file: \(path)"
            case .folder(let path): "\(path) is a folder, not a file"
            case .binary(let path): "\(path) isn't a text file"
            }
        }
    }

    public static func read(_ path: String) -> Result<CodeFile, Failure> {
        var directory: ObjCBool = false
        guard FileManager.default.fileExists(atPath: path, isDirectory: &directory) else { return .failure(.missing(path)) }
        if directory.boolValue { return .failure(.folder(path)) }
        guard let data = FileManager.default.contents(atPath: path) else { return .failure(.missing(path)) }
        if data.prefix(sniffBytes).contains(0) { return .failure(.binary(path)) }
        return .success(CodeFile(text: String(decoding: data, as: UTF8.self)))
    }
}
