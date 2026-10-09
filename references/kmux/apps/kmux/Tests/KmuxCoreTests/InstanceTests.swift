import Foundation
import Testing

@testable import KmuxCore

@Suite struct InstanceTests {
    @Test func defaultInstanceUsesKmuxSock() throws {
        let instance = try Instance.current(arguments: ["kmux"], environment: [:])
        #expect(instance.isDefault)
        #expect(instance.socketPath.hasSuffix("/kmux/kmux.sock"))
    }

    @Test func namedInstancesGetTheirOwnSocket() throws {
        let fromEnvironment = try Instance.current(arguments: ["kmux"], environment: ["KMUX_INSTANCE": "work"])
        #expect(fromEnvironment.name == "work")
        #expect(fromEnvironment.socketPath.hasSuffix("/kmux/kmux-work.sock"))
        let fromArgument = try Instance.current(arguments: ["kmux", "--instance", "2"], environment: ["KMUX_INSTANCE": "work"])
        #expect(fromArgument.name == "2")
        #expect(fromArgument.socketPath.hasSuffix("/kmux/kmux-2.sock"))
    }

    @Test func kmuxSocketOverridesThePath() throws {
        let instance = try Instance.current(arguments: ["kmux", "--instance", "x"], environment: ["KMUX_SOCKET": "/tmp/k.sock"])
        #expect(instance.name == "x")
        #expect(instance.socketPath == "/tmp/k.sock")
    }

    @Test func badNamesAreRejected() {
        for name in ["", "a/b", "../x", "with space", String(repeating: "a", count: 33)] {
            #expect(throws: KmuxError.self) { try Instance.current(arguments: ["kmux", "--instance", name], environment: [:]) }
        }
        #expect(throws: KmuxError.self) { try Instance.current(arguments: ["kmux", "--instance"], environment: [:]) }
    }

    @MainActor @Test func capabilitiesNameTheInstance() async {
        let core = Core()
        core.instance = Instance(name: "work")
        let reply = await core.handle(["id": 1, "cmd": "capabilities"])
        #expect(reply["instance"] == "work")
    }

    @Test func brandsNameTheirVariables() {
        #expect(Brand.current == .kmux)
        #expect(Brand.kmux.variable("SOCKET") == "KMUX_SOCKET")
        #expect(Brand(id: "kanna", displayName: "Kanna").variable("PANE") == "KANNA_PANE")
    }
}
