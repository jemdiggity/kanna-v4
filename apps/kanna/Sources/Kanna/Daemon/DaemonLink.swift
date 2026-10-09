import Foundation

@MainActor
final class DaemonLink: TerminalLink {
    private var connection: DaemonConnection!
    private let commands: Commands
    private var terminals: [String: ExternalTerminal] = [:]
    private var sizes: [String: JSON] = [:]
    private(set) var attached = false
    var onAttach: ((JSON) -> Void)?
    var onLayout: ((JSON) -> JSON)?
    var helloPanes: (() -> JSON)?
    var onStart: ((JSON) -> Void)?
    var onStop: ((String) -> Void)?
    var onPane: ((JSON) -> Void)?
    private var requests = 0

    var socketPath: String { connection.path }

    init(socketPath: String, commands: Commands) {
        self.commands = commands
        connection = DaemonConnection(
            path: socketPath,
            hello: { [weak self] in
                ["id": 0, "cmd": "app.hello", "args": ["pid": .number(Double(ProcessInfo.processInfo.processIdentifier)), "panes": self?.helloPanes?() ?? []]]
            },
            connected: { [weak self] in self?.connected() },
            received: { [weak self] message, delivery in self?.received(message, delivery) },
            disconnected: { [weak self] in self?.disconnected() })
    }

    func start() { connection.start() }
    func stop() { connection.stop() }

    private func send(_ cmd: String, _ args: [String: JSON]) {
        connection.send(["cmd": .string(cmd), "args": .object(args)])
    }

    private func connected() {
        for pane in terminals.keys.sorted() {
            send("attach", ["pane": .string(pane)])
            if case .object(let size)? = sizes[pane] { send("resize", size) }
        }
    }

    private func disconnected() {
        if attached { FileHandle.standardError.write(Data("kanna: lost the daemon at \(socketPath)\n".utf8)) }
        attached = false
    }

    private func received(_ message: JSON, _ delivery: DaemonConnection.Delivery) {
        let connection = connection!
        var consumed = true
        defer { if consumed { connection.consumed(delivery) } }
        if let ok = message["ok"]?.bool {
            guard message["id"]?.number == 0 else {
                if !ok { FileHandle.standardError.write(Data("kanna: the daemon refused a request: \(message["error"]?["message"]?.string ?? "")\n".utf8)) }
                return
            }
            attached = ok
            if ok {
                connection.accepted()
                onAttach?(message["panes"] ?? [])
            } else {
                connection.refused()
                FileHandle.standardError.write(Data("kanna: the daemon refused this app: \(message["error"]?["message"]?.string ?? "")\n".utf8))
            }
            return
        }
        guard let cmd = message["cmd"]?.string else { return }
        if cmd == "layout", let id = message["id"] {
            var reply = onLayout?(message["args"] ?? [:]) ?? ["ok": false, "error": ["code": "no_layout", "message": "this app keeps no layout"]]
            if case .object(var fields) = reply {
                fields["id"] = id
                reply = .object(fields)
            }
            connection.send(reply, generation: delivery.generation)
            return
        }
        if message["id"] != nil {
            Task { [commands] in connection.send(await commands.handle(message), generation: delivery.generation) }
            return
        }
        let args = message["args"] ?? [:]
        let terminal = args["pane"]?.string.flatMap { terminals[$0] }
        switch cmd {
        case "start":
            if let pane = args["pane"] { onStart?(pane) }
        case "stop":
            if let pane = args["pane"]?.string { onStop?(pane) }
        case "pane":
            if let pane = args["pane"] { onPane?(pane) }
        case "output":
            if let terminal, let data = args["data"]?.string.flatMap({ Data(base64Encoded: $0) }) {
                consumed = false
                terminal.output(data) { connection.consumed(delivery) }
            }
        case "snapshot":
            if let terminal, let data = args["data"]?.string.flatMap({ Data(base64Encoded: $0) }) {
                consumed = false
                terminal.loadSnapshot(data) { connection.consumed(delivery) }
            }
        case "grid":
            if let cols = args["cols"]?.number.flatMap({ UInt16(exactly: $0) }), let rows = args["rows"]?.number.flatMap({ UInt16(exactly: $0) }) {
                terminal?.setGrid(cols: cols, rows: rows)
            }
        case "exited":
            if let code = args["code"]?.number.flatMap({ Int32(exactly: $0) }) { terminal?.exited(status: code) }
        default: FileHandle.standardError.write(Data("kanna: unknown message from the daemon: \(cmd)\n".utf8))
        }
    }

    func update(_ pane: String, _ fields: [String: JSON]) {
        guard attached else { return }
        var args = fields
        args["pane"] = .string(pane)
        send("update", args)
    }

    func request(_ cmd: String, _ args: [String: JSON]) {
        guard attached else { return }
        requests += 1
        connection.send(["id": .number(Double(requests)), "cmd": .string(cmd), "args": .object(args)])
    }

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
