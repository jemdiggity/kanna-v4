import Foundation
import KannaLayoutC

@MainActor
final class AppLayout {
    nonisolated(unsafe) private let handle: OpaquePointer
    var shapes: (() -> [String: JSON])?

    init(statePath: String?, fresh: Bool) {
        var problem: UnsafeMutablePointer<CChar>?
        handle = kanna_layout_open(statePath, fresh, &problem)
        if let problem {
            FileHandle.standardError.write(Data("kanna: \(String(cString: problem))\n".utf8))
            kanna_layout_free_string(problem)
        }
    }

    deinit { kanna_layout_close(handle) }

    func call(_ request: JSON) -> JSON {
        let text = String(decoding: request.encoded(), as: UTF8.self)
        guard let reply = kanna_layout_call(handle, text) else { return ["ok": false, "error": ["code": "internal", "message": "no reply"]] }
        defer { kanna_layout_free_string(reply) }
        return (try? JSON.parse(Data(String(cString: reply).utf8))) ?? ["ok": false, "error": ["code": "internal", "message": "a reply that isn't JSON"]]
    }

    private func measure() {
        guard let shapes = shapes?() else { return }
        _ = call(["call": "shapes", "panes": .object(shapes)])
    }

    func attach(_ panes: JSON) -> JSON {
        measure()
        return call(["call": "attach", "panes": panes])
    }

    func serve(_ request: JSON) -> JSON {
        if request["op"]?.string == "place" { measure() }
        return call(["call": "serve", "request": request])
    }

    func command(_ cmd: String, _ args: [String: JSON]) -> JSON {
        serve(["op": "command", "cmd": .string(cmd), "args": .object(args)])
    }

    var view: JSON { call(["call": "view"]) }

    func focused(_ pane: String) -> Bool { call(["call": "focused", "pane": .string(pane)])["changed"]?.bool == true }

    func divider(tab: String, path: [Int], index: Int, share: Double) -> JSON {
        call(["call": "divider", "tab": .string(tab), "path": .array(path.map { .number(Double($0)) }), "index": .number(Double(index)), "share": .number(share)])
    }

    func frame(_ window: String, _ rect: CGRect) -> Bool {
        let frame: JSON = ["x": .number(rect.minX), "y": .number(rect.minY), "w": .number(rect.width), "h": .number(rect.height)]
        return call(["call": "frame", "window": .string(window), "frame": frame])["changed"]?.bool == true
    }

    func resolve(_ reference: String) -> String? { call(["call": "resolve", "pane": .string(reference)])["id"]?.string }

    func save() {
        let reply = call(["call": "save"])
        if reply["ok"]?.bool != true {
            FileHandle.standardError.write(Data("kanna: could not save the layout: \(reply["error"]?["message"]?.string ?? "")\n".utf8))
        }
    }
}
