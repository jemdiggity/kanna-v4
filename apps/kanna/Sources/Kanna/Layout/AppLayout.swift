import Foundation
import KannaLayoutC

@MainActor
final class AppLayout {
    nonisolated(unsafe) private let handle: OpaquePointer

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

    func attach(_ panes: JSON) -> JSON { call(["call": "attach", "panes": panes]) }
    func serve(_ request: JSON) -> JSON { call(["call": "serve", "request": request]) }
    var view: JSON { call(["call": "view"]) }

    func save() {
        let reply = call(["call": "save"])
        if reply["ok"]?.bool != true {
            FileHandle.standardError.write(Data("kanna: could not save the layout: \(reply["error"]?["message"]?.string ?? "")\n".utf8))
        }
    }
}
