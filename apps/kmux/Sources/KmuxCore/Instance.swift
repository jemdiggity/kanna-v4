import Foundation

/// One running kmux: a name and the socket it listens on. Several instances
/// can run at once, each with its own windows. The default instance listens
/// on `kmux.sock`; one named `work` listens on `kmux-work.sock` beside it.
/// (Other brands use their own names: `kanna.sock` in `…/kanna`.)
public struct Instance: Sendable, Equatable {
    public static let defaultName = "default"

    public let name: String
    public let socketPath: String

    public var isDefault: Bool { name == Self.defaultName }

    /// `~/Library/Application Support/kmux`, where the sockets live.
    public static var directory: String {
        let support = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        return support.appendingPathComponent(Brand.current.id).path
    }

    public static func socketPath(for name: String) -> String {
        let brand = Brand.current.id
        let file = name == defaultName ? "\(brand).sock" : "\(brand)-\(name).sock"
        return (directory as NSString).appendingPathComponent(file)
    }

    /// Names are short and safe in a file name: letters, digits, `-` and `_`.
    public static func validate(_ name: String) throws {
        let allowed = CharacterSet.alphanumerics.union(CharacterSet(charactersIn: "-_"))
        guard !name.isEmpty, name.count <= 32, name.unicodeScalars.allSatisfy({ allowed.contains($0) && $0.isASCII }) else {
            throw KmuxError("bad_request", "bad instance name \"\(name)\": use letters, digits, - and _ (up to 32)")
        }
    }

    public init(name: String, socketPath: String? = nil) {
        self.name = name
        self.socketPath = socketPath ?? Self.socketPath(for: name)
    }

    /// The instance this process should be: `--instance NAME` on the command
    /// line, else `$KMUX_INSTANCE`, else the default. `$KMUX_SOCKET` overrides
    /// the socket path.
    public static func current(arguments: [String] = CommandLine.arguments, environment: [String: String] = ProcessInfo.processInfo.environment) throws -> Instance {
        let brand = Brand.current
        var name = environment[brand.variable("INSTANCE")].flatMap { $0.isEmpty ? nil : $0 } ?? defaultName
        if let flag = arguments.firstIndex(of: "--instance") {
            guard flag + 1 < arguments.count else { throw KmuxError("bad_request", "--instance needs a NAME") }
            name = arguments[flag + 1]
        }
        try validate(name)
        let socket = environment[brand.variable("SOCKET")].flatMap { $0.isEmpty ? nil : $0 }
        return Instance(name: name, socketPath: socket)
    }

    /// The names of instances whose sockets exist in `directory`, default first.
    /// (Some may be stale: check with `SocketServer.isListening`.)
    public static func socketNames() -> [String] {
        let files = (try? FileManager.default.contentsOfDirectory(atPath: directory)) ?? []
        let brand = Brand.current.id
        let names = files.compactMap { file -> String? in
            if file == "\(brand).sock" { return defaultName }
            guard file.hasPrefix("\(brand)-"), file.hasSuffix(".sock") else { return nil }
            return String(file.dropFirst(brand.count + 1).dropLast(5))
        }
        return names.sorted { ($0 == defaultName ? "" : $0) < ($1 == defaultName ? "" : $1) }
    }

    /// The first unused name of the form 2, 3, …, for "New Instance".
    public static func unusedName() -> String {
        var n = 2
        while SocketServer.isListening(socketPath(for: String(n))) { n += 1 }
        return String(n)
    }
}
