import CoreServices
import Foundation
import UniformTypeIdentifiers

public struct UntrustedURL: Equatable, Sendable {
    public enum DenialReason: String, Equatable, Sendable {
        case malformedURL
        case unsafeCharacters
        case invalidWebURL
        case unsupportedScheme
        case inaccessibleFile
        case unsafeFile

        public var message: String {
            switch self {
            case .malformedURL: "The target is not an absolute URL with a scheme."
            case .unsafeCharacters: "The target contains invisible or line-breaking characters."
            case .invalidWebURL: "The web target does not contain a valid host."
            case .unsupportedScheme: "Only web, mail and local document links open from terminal output."
            case .inaccessibleFile: "The local target does not exist or is not a regular file or folder."
            case .unsafeFile: "Opening this local target could run code."
            }
        }
    }

    public enum Decision: Equatable, Sendable {
        case allow(URL)
        case deny(DenialReason)

        public var name: String {
            switch self {
            case .allow: "allow"
            case .deny(let reason): "deny:\(reason.rawValue)"
            }
        }
    }

    public let string: String
    let url: URL?
    let handler: @Sendable (URL) -> String?

    public static func == (a: UntrustedURL, b: UntrustedURL) -> Bool { a.string == b.string && a.url == b.url }

    public init(_ string: String, handler: @escaping @Sendable (URL) -> String? = UntrustedURL.defaultHandler) {
        self.string = string
        self.handler = handler
        url = string.unicodeScalars.contains(where: Self.isUnsafeCharacter) ? nil : URL(string: string)
    }

    public init(clicked text: String, handler: @escaping @Sendable (URL) -> String? = UntrustedURL.defaultHandler) {
        self.init(text, handler: handler)
        if url?.scheme == nil, !text.unicodeScalars.contains(where: Self.isUnsafeCharacter), text.hasPrefix("/") || text.hasPrefix("~") {
            self = UntrustedURL(text, url: URL(filePath: NSString(string: text).standardizingPath), handler: handler)
        }
    }

    private init(_ string: String, url: URL, handler: @escaping @Sendable (URL) -> String?) {
        self.string = string
        self.url = url
        self.handler = handler
    }

    public var decision: Decision {
        guard !string.isEmpty else { return .deny(.malformedURL) }
        guard !string.unicodeScalars.contains(where: Self.isUnsafeCharacter) else { return .deny(.unsafeCharacters) }
        guard let url, let scheme = url.scheme?.lowercased(), !scheme.isEmpty else { return .deny(.malformedURL) }
        switch scheme {
        case "http", "https":
            guard let host = url.host(percentEncoded: false), !host.isEmpty,
                  !host.unicodeScalars.contains(where: { Self.isUnsafeCharacter($0) || $0 == " " }) else { return .deny(.invalidWebURL) }
            return .allow(url)
        case "mailto":
            guard let components = URLComponents(url: url, resolvingAgainstBaseURL: false), !components.path.isEmpty else {
                return .deny(.malformedURL)
            }
            return .allow(url)
        case "file":
            return fileDecision(for: url)
        default:
            return .deny(.unsupportedScheme)
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

    public static let defaultHandler: @Sendable (URL) -> String? = { url in
        guard let app = LSCopyDefaultApplicationURLForURL(url as CFURL, .all, nil)?.takeRetainedValue() as URL? else { return nil }
        return Bundle(url: app)?.bundleIdentifier
    }
}

private extension UntrustedURL {
    func fileDecision(for url: URL) -> Decision {
        guard url.isFileURL, url.query == nil, url.fragment == nil, url.user == nil, url.port == nil else { return .deny(.malformedURL) }
        if let host = url.host(percentEncoded: false), !host.isEmpty, host.caseInsensitiveCompare("localhost") != .orderedSame {
            return .deny(.malformedURL)
        }
        let canonical = URL(filePath: url.standardizedFileURL.resolvingSymlinksInPath().path)
        let values: URLResourceValues
        do {
            values = try canonical.resourceValues(forKeys: [
                .contentTypeKey, .isDirectoryKey, .isExecutableKey, .isRegularFileKey, .isPackageKey, .isAliasFileKey, .isSymbolicLinkKey,
            ])
        } catch {
            return .deny(.inaccessibleFile)
        }
        guard values.isDirectory == true || values.isRegularFile == true else { return .deny(.inaccessibleFile) }
        guard !isUnsafeFile(canonical, values: values) else { return .deny(.unsafeFile) }
        return .allow(canonical)
    }

    func isUnsafeFile(_ url: URL, values: URLResourceValues) -> Bool {
        if values.isSymbolicLink == true || values.isAliasFile == true || values.isPackage == true { return true }
        let names = url.pathComponents.dropFirst().map { $0.lowercased() }
        if names.contains(where: { name in Self.unsafePathExtensions.contains { name.hasSuffix(".\($0)") } }) { return true }
        if let type = values.contentType, Self.unsafeContentTypes.contains(where: { type.conforms(to: $0) }) { return true }
        if values.isDirectory != true, values.isExecutable == true { return true }
        if values.isDirectory != true, let app = handler(url), Self.unsafeHandlers.contains(app.lowercased()) { return true }
        return false
    }
}

extension UntrustedURL {
    static func isUnsafeCharacter(_ scalar: Unicode.Scalar) -> Bool {
        switch scalar.value {
        case 0x00...0x1F, 0x7F...0xA0: true
        case 0x061C, 0x115F, 0x1160, 0x180E, 0x200B...0x200F, 0x202A...0x202E, 0x2066...0x2069: true
        case 0x2028...0x2029, 0x205F, 0x3000, 0x3164: true
        case 0x2060...0x2064, 0xFEFF, 0xFFA0, 0xFFF9...0xFFFB: true
        default: false
        }
    }

    static let unsafePathExtensions: Set<String> = [
        "action", "afploc", "app", "appex", "applescript", "bundle", "class", "command", "component", "configprofile", "definition",
        "desktop", "fileloc", "framework", "ftploc", "inetloc", "jar", "jnlp", "kext", "mailloc", "mdimporter", "mobileconfig",
        "mpkg", "newsloc", "osax", "pkg", "plugin", "prefpane", "qlgenerator", "saver", "scpt", "scptd", "service", "shortcut",
        "systemextension", "terminal", "tool", "url", "webarchive", "webloc", "workflow", "xpc",
    ]

    static let unsafeContentTypes: [UTType] = [
        .application, .executable, .script, .bundle, .package, .aliasFile, .symbolicLink, .internetLocation, .internetShortcut,
        .urlBookmarkData, .webArchive,
    ]

    static let unsafeHandlers: Set<String> = [
        "com.apple.terminal", "com.googlecode.iterm2", "com.mitchellh.ghostty", "net.kovidgoyal.kitty", "io.alacritty",
        "com.github.wez.wezterm", "dev.warp.warp-stable", "dev.kanna.app", "dev.kanna.app.dev", "dev.kanna.kanna",
        "com.apple.scripteditor2", "com.apple.automator", "com.apple.automatorinstaller", "com.apple.automator.runner",
        "com.apple.installer", "com.apple.shortcuts", "com.apple.systempreferences", "com.apple.systemsettings",
        "com.apple.jar-launcher", "com.apple.javawebstart", "com.apple.archiveutility", "com.apple.diskimagemounter",
        "com.apple.ticket-viewer", "com.apple.keychainaccess",
    ]
}
