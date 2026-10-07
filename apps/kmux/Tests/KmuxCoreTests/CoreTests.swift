import Darwin
import Foundation
import Testing

@testable import KmuxCore

@MainActor
final class FakeHost: PaneHost {
    weak var core: Core?
    var fail: Set<String> = []
    var stopped: [String] = []

    func start(_ pane: Pane) {
        let id = pane.id, failing = fail.contains(pane.command ?? "")
        Task { @MainActor in
            if failing { self.core?.update(id, state: .failed, error: "command not found") } else { self.core?.update(id, state: .running) }
        }
    }

    func stop(_ pane: Pane) { stopped.append(pane.id) }
}

@MainActor
func makeCore() -> (Core, FakeHost) {
    let core = Core(), host = FakeHost()
    host.core = core
    core.host = host
    return (core, host)
}

@MainActor
@Suite struct CoreTests {
    @Test func capabilities() async {
        let (core, _) = makeCore()
        let reply = await core.handle(["id": 1, "cmd": "capabilities"])
        #expect(reply["ok"] == true)
        #expect(reply["id"] == 1)
        #expect(reply["mux"] == "kmux")
    }

    @Test func openWaitsForRunningAndCreatesAWindow() async {
        let (core, _) = makeCore()
        let reply = await core.handle(["id": 1, "cmd": "open", "args": ["type": "term", "name": "server", "cmd": "npm run dev"]])
        #expect(reply["ok"] == true)
        #expect(reply["window"] == "w1")
        #expect(reply["pane"]?["state"] == "running")
        #expect(reply["pane"]?["name"] == "server")
    }

    @Test func openSplitsTheFocusedPaneByHalf() async {
        let (core, _) = makeCore()
        _ = await core.handle(["cmd": "open", "args": ["type": "term", "name": "a"]])
        _ = await core.handle(["cmd": "open", "args": ["type": "term", "name": "b", "split": "right", "size": "1/3"]])
        _ = await core.handle(["cmd": "open", "args": ["type": "term", "name": "c", "split": "right"]])
        let list = await core.handle(["cmd": "list"])
        let layout: JSON = ["split": "row", "children": [["pane": "a", "size": "2/3"], ["pane": "b", "size": "1/6"], ["pane": "c", "size": "1/6"]]]
        #expect(list["windows"] == [["id": "w1", "key": true, "tabs": [["id": "t1", "title": "a · b · c", "active": true, "layout": layout]]]])
    }

    @Test func autoSplitFollowsThePaneShape() async {
        let (core, _) = makeCore()
        core.paneIsWide = { _ in false }
        _ = await core.handle(["cmd": "open", "args": ["type": "term", "name": "a"]])
        _ = await core.handle(["cmd": "open", "args": ["type": "term", "name": "b"]])
        let list = await core.handle(["cmd": "list"])
        if case .array(let windows) = list["windows"], case .array(let tabs) = windows[0]["tabs"] {
            #expect(tabs[0]["layout"]?["split"] == "column")
        } else {
            Issue.record("no windows in \(list)")
        }
    }

    @Test func openReportsStartFailures() async {
        let (core, host) = makeCore()
        host.fail = ["nope"]
        let reply = await core.handle(["cmd": "open", "args": ["type": "term", "cmd": "nope"]])
        #expect(reply["ok"] == false)
        #expect(reply["error"]?["code"] == "start_failed")
        #expect(reply["error"]?["message"] == "command not found")
    }

    @Test func errors() async {
        let (core, _) = makeCore()
        _ = await core.handle(["cmd": "open", "args": ["type": "term", "name": "a"]])
        let cases: [(JSON, String)] = [
            (["cmd": "nope"], "bad_request"),
            (["cmd": "open", "args": ["type": "nope"]], "bad_request"),
            (["cmd": "open", "args": ["type": "term", "name": "a"]], "name_taken"),
            (["cmd": "open", "args": ["type": "term", "size": "3/2"]], "bad_request"),
            (["cmd": "open", "args": ["type": "term", "window": "w9"]], "not_found"),
            (["cmd": "close", "args": ["pane": "zzz"]], "not_found"),
            (["cmd": "close"], "bad_request"),
        ]
        for (request, code) in cases {
            let reply = await core.handle(request)
            #expect(reply["error"]?["code"] == .string(code), "\(request)")
        }
    }

    @Test func closeRedistributesAndDropsEmptyWindows() async {
        let (core, host) = makeCore()
        _ = await core.handle(["cmd": "open", "args": ["type": "term", "name": "a"]])
        _ = await core.handle(["cmd": "open", "args": ["type": "term", "name": "b", "split": "right", "size": "1/4"]])
        _ = await core.handle(["cmd": "open", "args": ["type": "term", "name": "solo", "window": "new"]])
        #expect(core.model.windows.count == 2)

        let closed = await core.handle(["cmd": "close", "args": ["pane": "solo"]])
        #expect(closed["closed"] == ["p3"])
        #expect(core.model.windows.map(\.id) == ["w1"])
        #expect(core.model.key == "w1")

        _ = await core.handle(["cmd": "close", "args": ["pane": "a"]])
        #expect(core.model.tree(core.model.windows[0].tabs[0].root) == ["pane": "b"])
        #expect(core.model.windows[0].focused == "p2")
        #expect(host.stopped == ["p3", "p1"])
    }

    @Test func fractions() {
        #expect(Fraction.parse("1/3")! == 1.0 / 3)
        #expect(Fraction.parse("25%") == 0.25)
        #expect(Fraction.parse(0.5) == 0.5)
        #expect(Fraction.parse("x") == nil)
        #expect(Fraction.format(2.0 / 3) == "2/3")
        #expect(Fraction.format(0.37) == "37%")
    }
}

@Suite struct SocketServerTests {
    @Test func answersNDJSONRequestsInOrder() async throws {
        let path = "/tmp/claude/kmux-test-\(getpid()).sock"
        try? FileManager.default.createDirectory(atPath: "/tmp/claude", withIntermediateDirectories: true)
        let server = SocketServer(path: path) { request in
            ["id": request["id"] ?? nil, "ok": true, "echo": request["cmd"] ?? nil]
        }
        try server.start()
        defer { server.stop() }

        let lines = try await Task.detached {
            try roundTrip(path, "{\"id\":1,\"cmd\":\"a\"}\n{\"id\":2,\"cmd\":\"b\"}\nnot json\n", replies: 3)
        }.value
        #expect(lines.count == 3)
        #expect(try JSON.parse(Data(lines[0].utf8)) == ["id": 1, "ok": true, "echo": "a"])
        #expect(try JSON.parse(Data(lines[1].utf8)) == ["id": 2, "ok": true, "echo": "b"])
        #expect(try JSON.parse(Data(lines[2].utf8))["error"]?["code"] == "bad_request")
        #expect(SocketServer.isListening(path))
    }
}

func roundTrip(_ path: String, _ text: String, replies: Int) throws -> [String] {
    let fd = socket(AF_UNIX, SOCK_STREAM, 0)
    defer { close(fd) }
    var address = SocketServer.address(path)
    let connected = withUnsafePointer(to: &address) {
        $0.withMemoryRebound(to: sockaddr.self, capacity: 1) { connect(fd, $0, socklen_t(MemoryLayout<sockaddr_un>.size)) }
    }
    guard connected == 0 else { throw KmuxError("connect", String(cString: strerror(errno))) }
    _ = text.withCString { write(fd, $0, strlen($0)) }
    var received = Data()
    var chunk = [UInt8](repeating: 0, count: 4096)
    while received.filter({ $0 == UInt8(ascii: "\n") }).count < replies {
        let count = read(fd, &chunk, chunk.count)
        if count <= 0 { break }
        received.append(chunk, count: count)
    }
    return String(decoding: received, as: UTF8.self).split(separator: "\n").map(String.init)
}
