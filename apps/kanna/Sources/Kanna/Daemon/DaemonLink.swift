import Foundation

/// Kanna's terminals live in the daemon: this link attaches each terminal
/// view to its terminal there, and carries the rest of the app link
/// (docs/kanna-spec.md §4.5): the layout to mirror, and requests for the app.
@MainActor
final class DaemonLink: TerminalLink {
    private var connection: DaemonConnection!
    private let commands: Commands
    /// The terminals shown, by pane ID.
    private var terminals: [String: ExternalTerminal] = [:]
    /// The size each view last asked for, to ask again after a reconnect.
    private var sizes: [String: JSON] = [:]
    /// The daemon accepted this app (`app.hello`).
    private(set) var attached = false
    /// The layout to mirror, as the daemon sends it.
    var onLayout: ((JSON) -> Void)?

    var socketPath: String { connection.path }

    init(socketPath: String, commands: Commands) {
        self.commands = commands
        connection = DaemonConnection(
            path: socketPath,
            connected: { [weak self] in self?.connected() },
            received: { [weak self] in self?.received($0) },
            disconnected: { [weak self] in self?.disconnected() })
    }

    func start() { connection.start() }
    func stop() { connection.stop() }

    private func send(_ cmd: String, _ args: [String: JSON]) {
        connection.send(["cmd": .string(cmd), "args": .object(args)])
    }

    // MARK: The connection

    private func connected() {
        connection.send(["id": 0, "cmd": "app.hello", "args": ["pid": .number(Double(ProcessInfo.processInfo.processIdentifier))]])
        // After a reconnect: the terminals shown, again.
        for pane in terminals.keys.sorted() {
            send("attach", ["pane": .string(pane)])
            if case .object(let size)? = sizes[pane] { send("resize", size) }
        }
    }

    private func disconnected() {
        if attached { FileHandle.standardError.write(Data("kanna: lost the daemon at \(socketPath)\n".utf8)) }
        attached = false
    }

    private func received(_ message: JSON) {
        // The answer to app.hello, the only request the app makes.
        if let ok = message["ok"]?.bool {
            attached = ok
            if !ok { FileHandle.standardError.write(Data("kanna: the daemon refused this app: \(message["error"]?["message"]?.string ?? "")\n".utf8)) }
            return
        }
        guard let cmd = message["cmd"]?.string else { return }
        // A request for the app (debug.*): answered as the control protocol would.
        if message["id"] != nil {
            Task { [commands, connection] in connection?.send(await commands.handle(message)) }
            return
        }
        let args = message["args"] ?? [:]
        let terminal = args["pane"]?.string.flatMap { terminals[$0] }
        switch cmd {
        case "layout": onLayout?(args)
        case "output":
            if let data = args["data"]?.string.flatMap({ Data(base64Encoded: $0) }) { terminal?.output(data) }
        case "grid":
            if let cols = args["cols"]?.number, let rows = args["rows"]?.number { terminal?.setGrid(cols: UInt16(cols), rows: UInt16(rows)) }
        case "exited":
            if let code = args["code"]?.number { terminal?.exited(status: Int32(code)) }
        default: FileHandle.standardError.write(Data("kanna: unknown message from the daemon: \(cmd)\n".utf8))
        }
    }

    // MARK: TerminalLink

    func opened(_ terminal: ExternalTerminal) {
        terminals[terminal.pane.id] = terminal
        send("attach", ["pane": .string(terminal.pane.id)])
    }

    func input(_ bytes: Data, from terminal: ExternalTerminal) {
        send("input", ["pane": .string(terminal.pane.id), "data": .string(bytes.base64EncodedString())])
    }

    func resizeRequested(_ terminal: ExternalTerminal, cols: UInt16, rows: UInt16, width: UInt32, height: UInt32) {
        let size: [String: JSON] = ["pane": .string(terminal.pane.id), "cols": .number(Double(cols)), "rows": .number(Double(rows)),
                                    "width": .number(Double(width)), "height": .number(Double(height))]
        sizes[terminal.pane.id] = .object(size)
        send("resize", size)
    }

    func closed(_ terminal: ExternalTerminal) {
        terminals.removeValue(forKey: terminal.pane.id)
        sizes.removeValue(forKey: terminal.pane.id)
        send("detach", ["pane": .string(terminal.pane.id)])
    }
}
