import Foundation

public struct DocMessage: Codable, Equatable {
    public var who: String
    public var text: String
    public var at: String
}

public struct DocThread: Codable, Equatable {
    public var id: String
    public var quote: String
    public var prefix: String?
    public var suffix: String?
    public var line: Int
    public var state: String
    public var status: String?
    public var messages: [DocMessage]
    public var error: String?

    public var isOpen: Bool { state == "open" }
}

public struct DocRecord: Codable, Equatable {
    public var file: String
    public var agent: String?
    public var socket: String?
    public var next: Int
    public var threads: [DocThread]

    init(file: String) {
        self.file = file
        next = 1
        threads = []
    }
}

public struct DocStore {
    public let file: String
    public let path: String

    public init(file: String) {
        self.file = file
        let root = Self.repoRoot(of: file)
        let relative = Self.relative(file, to: root)
        path = root + "/.kanna/docs/" + relative.replacingOccurrences(of: "/", with: "__") + ".json"
    }

    public var relativeName: String { Self.relative(file, to: Self.repoRoot(of: file)) }

    static func repoRoot(of file: String) -> String {
        let start = URL(fileURLWithPath: file).deletingLastPathComponent().standardizedFileURL
        var folder = start
        while true {
            if FileManager.default.fileExists(atPath: folder.appendingPathComponent(".git").path) { return folder.path }
            let up = folder.deletingLastPathComponent()
            if up.path == folder.path { return start.path }
            folder = up
        }
    }

    static func relative(_ file: String, to root: String) -> String {
        file.hasPrefix(root + "/") ? String(file.dropFirst(root.count + 1)) : (file as NSString).lastPathComponent
    }

    public var stamp: Date? { (try? FileManager.default.attributesOfItem(atPath: path))?[.modificationDate] as? Date }

    public func read() -> DocRecord {
        guard let data = FileManager.default.contents(atPath: path), let record = try? JSONDecoder().decode(DocRecord.self, from: data) else {
            return DocRecord(file: file)
        }
        return record
    }

    @discardableResult
    public func update<T>(_ change: (inout DocRecord) -> T) throws -> T {
        try FileManager.default.createDirectory(atPath: (path as NSString).deletingLastPathComponent, withIntermediateDirectories: true)
        let lock = open(path + ".lock", O_WRONLY | O_CREAT, 0o644)
        guard lock >= 0 else { throw CocoaError(.fileWriteNoPermission) }
        defer { close(lock) }
        flock(lock, LOCK_EX)
        defer { flock(lock, LOCK_UN) }
        var record = read()
        let result = change(&record)
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .withoutEscapingSlashes]
        let temporary = (path as NSString).deletingPathExtension + ".tmp"
        try encoder.encode(record).write(to: URL(fileURLWithPath: temporary))
        _ = rename(temporary, path)
        return result
    }

    public static func now() -> String {
        let formatter = DateFormatter()
        formatter.locale = Locale(identifier: "en_US_POSIX")
        formatter.dateFormat = "yyyy-MM-dd'T'HH:mm:ss"
        return formatter.string(from: Date())
    }

    public static func clean(_ text: String) -> String {
        let normalized = text.replacingOccurrences(of: "\r\n", with: "\n").replacingOccurrences(of: "\r", with: "\n")
        return String(String.UnicodeScalarView(normalized.unicodeScalars.filter { scalar in
            let value = scalar.value
            return value == 0x0a || value == 0x09 || !(value < 0x20 || (0x7f...0x9f).contains(value))
        }))
    }

    public static func message(_ thread: DocThread, file: String, text: String) -> String {
        let quote = thread.quote.split(separator: "\n", omittingEmptySubsequences: false).map { "> " + $0 }.joined(separator: "\n")
        return "[kanna comment \(thread.id) · \(file):\(thread.line)]\n\(quote.isEmpty ? "> " : quote)\n\(text)"
    }

    public static func line(of quote: String, in source: String, near hint: Int) -> Int {
        let words = quote.split(whereSeparator: \.isWhitespace)
        for probe in [quote, words.prefix(3).joined(separator: " "), words.prefix(1).joined(separator: " ")] where !probe.isEmpty {
            var hits: [Int] = []
            var search = source.startIndex..<source.endIndex
            while let found = source.range(of: probe, range: search) {
                hits.append(source[..<found.lowerBound].reduce(1) { $1 == "\n" ? $0 + 1 : $0 })
                search = source.index(after: found.lowerBound)..<source.endIndex
            }
            if let best = hits.min(by: { abs($0 - hint) < abs($1 - hint) }) { return best }
        }
        return hint
    }

    public static func locate(_ thread: DocThread, in text: String) -> NSRange? {
        let hay = text as NSString
        let quote = thread.quote
        guard !quote.isEmpty else { return nil }
        let prefix = Array((thread.prefix ?? "").utf16), suffix = Array((thread.suffix ?? "").utf16)
        var best: (at: Int, score: Int)?
        var from = 0
        while from < hay.length {
            let found = hay.range(of: quote, options: [.literal], range: NSRange(location: from, length: hay.length - from))
            guard found.location != NSNotFound else { break }
            let before = Array(hay.substring(with: NSRange(location: max(0, found.location - 32), length: min(32, found.location))).utf16)
            let afterStart = NSMaxRange(found)
            let after = Array(hay.substring(with: NSRange(location: afterStart, length: min(32, hay.length - afterStart))).utf16)
            var score = 0
            while score < min(before.count, prefix.count), before[before.count - 1 - score] == prefix[prefix.count - 1 - score] { score += 1 }
            var tail = 0
            while tail < min(after.count, suffix.count), after[tail] == suffix[tail] { tail += 1 }
            if best == nil || score + tail > best!.score { best = (found.location, score + tail) }
            from = found.location + 1
        }
        return best.map { NSRange(location: $0.at, length: (quote as NSString).length) }
    }
}
