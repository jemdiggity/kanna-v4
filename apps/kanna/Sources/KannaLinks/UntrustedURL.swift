import Foundation
import UniformTypeIdentifiers

public struct UntrustedURL: Equatable, Sendable {
    public enum DenialReason: String, Equatable, Sendable {
        case malformedURL
        case unsafeCharacters
        case invalidWebURL
        case inaccessibleFile
        case unsafeFile

        public var message: String {
            switch self {
            case .malformedURL: "The target is not an absolute URL with a scheme."
            case .unsafeCharacters: "The target contains invisible or line-breaking characters."
            case .invalidWebURL: "The web target does not contain a valid host."
            case .inaccessibleFile: "The local target does not exist or is not a regular file or directory."
            case .unsafeFile: "Opening this local target could execute code."
            }
        }
    }

    public enum Decision: Equatable, Sendable {
        case allow(URL)
        case confirm(URL)
        case deny(DenialReason)

        public var name: String {
            switch self {
            case .allow: "allow"
            case .confirm: "confirm"
            case .deny(let reason): "deny:\(reason.rawValue)"
            }
        }
    }

    public let string: String

    public init(_ string: String) {
        self.string = string
    }

    public init(clicked text: String) {
        guard !text.unicodeScalars.contains(where: Self.isUnsafeCharacter), URL(string: text)?.scheme == nil,
              text.hasPrefix("/") || text.hasPrefix("~") else {
            self.init(text)
            return
        }
        self.init(URL(filePath: NSString(string: text).standardizingPath).absoluteString)
    }

    public var decision: Decision {
        guard !string.isEmpty else { return .deny(.malformedURL) }
        guard !string.unicodeScalars.contains(where: Self.isUnsafeCharacter) else { return .deny(.unsafeCharacters) }
        guard let url = URL(string: string), let scheme = url.scheme?.lowercased(), !scheme.isEmpty else { return .deny(.malformedURL) }
        switch scheme {
        case "http", "https":
            guard let host = url.host, !host.isEmpty else { return .deny(.invalidWebURL) }
            return .allow(url)
        case "mailto":
            guard let components = URLComponents(url: url, resolvingAgainstBaseURL: false), !components.path.isEmpty else {
                return .deny(.malformedURL)
            }
            return .allow(url)
        case "file":
            return fileDecision(for: url)
        default:
            return .confirm(url)
        }
    }

    public var displayString: String {
        let normalized: String
        if let url = URL(string: string), url.scheme != nil {
            normalized = url.isFileURL ? url.standardizedFileURL.resolvingSymlinksInPath().path : string
        } else {
            normalized = URL(filePath: string).standardizedFileURL.path
        }
        var result = String()
        result.reserveCapacity(normalized.count)
        for scalar in normalized.unicodeScalars {
            if Self.isUnsafeCharacter(scalar) {
                result += "\\u{\(String(scalar.value, radix: 16, uppercase: true))}"
            } else {
                result.unicodeScalars.append(scalar)
            }
        }
        return result
    }
}

private extension UntrustedURL {
    func fileDecision(for url: URL) -> Decision {
        guard url.isFileURL, url.query == nil, url.fragment == nil else { return .deny(.malformedURL) }
        if let host = url.host, !host.isEmpty, host.caseInsensitiveCompare("localhost") != .orderedSame {
            return .deny(.malformedURL)
        }
        let canonicalURL = url.standardizedFileURL.resolvingSymlinksInPath()
        let values: URLResourceValues
        do {
            values = try canonicalURL.resourceValues(forKeys: [.contentTypeKey, .isDirectoryKey, .isExecutableKey, .isRegularFileKey])
        } catch {
            return .deny(.inaccessibleFile)
        }
        guard values.isDirectory == true || values.isRegularFile == true else { return .deny(.inaccessibleFile) }
        guard !Self.isUnsafeFile(canonicalURL, values: values) else { return .deny(.unsafeFile) }
        return .allow(canonicalURL)
    }

    static func isUnsafeFile(_ url: URL, values: URLResourceValues) -> Bool {
        if unsafePathExtensions.contains(url.pathExtension.lowercased()) { return true }
        if let type = values.contentType, unsafeContentTypes.contains(where: { type.conforms(to: $0) }) { return true }
        return values.isDirectory != true && values.isExecutable == true
    }

    static func isUnsafeCharacter(_ scalar: Unicode.Scalar) -> Bool {
        switch scalar.value {
        case 0x00...0x1F, 0x7F...0x9F: true
        case 0x061C, 0x200B...0x200F, 0x202A...0x202E, 0x2066...0x2069: true
        case 0x2028...0x2029: true
        case 0x2060, 0xFEFF: true
        default: false
        }
    }

    static let unsafePathExtensions: Set<String> = [
        "action", "app", "applescript", "class", "command", "desktop", "inetloc", "jar", "mobileconfig",
        "mpkg", "pkg", "scpt", "terminal", "tool", "url", "webloc", "workflow",
    ]

    static let unsafeContentTypes: [UTType] = [.application, .executable, .script]
}
