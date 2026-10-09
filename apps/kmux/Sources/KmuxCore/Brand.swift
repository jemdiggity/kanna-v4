import Foundation

/// Which product this build is: kmux itself, or a product built on it (Kanna
/// ships its own copy as Kanna.app). The brand names the socket folder and
/// files and the environment variables, so the two never talk to each other's
/// windows. The build script writes it into Info.plist; plain builds and tests
/// are kmux.
public struct Brand: Sendable, Equatable {
    /// Lower case: the socket folder and files (`kmux`, `kmux.sock`).
    public let id: String
    /// What the user sees: window title, app menu.
    public let displayName: String
    /// A program every terminal runs through, if the product hosts its own
    /// terminals (Kanna keeps them alive across app restarts): kmux runs
    /// `WRAPPER` for a shell, or `WRAPPER -- SHELL -c COMMAND` for a command.
    /// A relative path is inside the app bundle's Contents.
    public let terminalWrapper: String?

    public static let kmux = Brand(id: "kmux", displayName: "kmux")

    public static let current: Brand = {
        let info = Bundle.main.infoDictionary ?? [:]
        // KMUX_TERMINAL_WRAPPER overrides it, for tests.
        let wrapper = (ProcessInfo.processInfo.environment["KMUX_TERMINAL_WRAPPER"] ?? info["KmuxTerminalWrapper"] as? String)
            .flatMap { $0.isEmpty ? nil : $0 }
            .map { $0.hasPrefix("/") ? $0 : Bundle.main.bundleURL.appendingPathComponent("Contents").appendingPathComponent($0).path }
        guard let id = info["KmuxBrand"] as? String, !id.isEmpty else { return Brand(id: "kmux", displayName: "kmux", terminalWrapper: wrapper) }
        return Brand(id: id, displayName: info["CFBundleName"] as? String ?? id, terminalWrapper: wrapper)
    }()

    public init(id: String, displayName: String, terminalWrapper: String? = nil) {
        self.id = id
        self.displayName = displayName
        self.terminalWrapper = terminalWrapper
    }

    /// `KMUX_SOCKET` for kmux, `KANNA_SOCKET` for Kanna.
    public func variable(_ name: String) -> String { "\(id.uppercased())_\(name)" }
}
