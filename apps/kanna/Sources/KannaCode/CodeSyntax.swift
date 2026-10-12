import Foundation
import KannaSyntaxC

public struct CodeSpan: Equatable, Sendable {
    public let range: NSRange
    public let capture: String
}

public struct CodeScope: Equatable, Sendable {
    public let first: Int
    public let last: Int
}

public struct CodeHighlights: Equatable, Sendable {
    public var spans: [CodeSpan] = []
    public var scopes: [CodeScope] = []
}

public enum CodeSyntax {
    public static let largest = 5 * 1024 * 1024

    static let captures: [String] = (0..<kanna_syntax_capture_count()).map { String(cString: kanna_syntax_capture_name($0)) }

    public static func language(for path: String, firstLine: String) -> String? {
        let name = (path as NSString).lastPathComponent
        guard let found = kanna_syntax_language(name, firstLine) else { return nil }
        return String(cString: found)
    }

    public static func highlight(_ text: String, language: String) -> CodeHighlights {
        var bytes = Array(text.utf8)
        guard bytes.count <= largest else { return CodeHighlights() }
        guard let result = bytes.withUnsafeMutableBufferPointer({ kanna_syntax_highlight(language, $0.baseAddress, $0.count) }) else { return CodeHighlights() }
        defer { kanna_syntax_free(result) }
        let raw = result.pointee
        let names = captures
        let spans = UnsafeBufferPointer(start: raw.spans, count: raw.span_count).map { span in
            CodeSpan(range: NSRange(location: Int(span.start), length: Int(span.end - span.start)), capture: Int(span.capture) < names.count ? names[Int(span.capture)] : "")
        }
        let scopes = UnsafeBufferPointer(start: raw.scopes, count: raw.scope_count).map { CodeScope(first: Int($0.first), last: Int($0.last)) }
        return CodeHighlights(spans: spans, scopes: scopes)
    }
}
