import Darwin
import Foundation

/// The control socket: a Unix domain socket carrying newline-delimited JSON.
/// Each connection is served on its own thread; requests on a connection are
/// answered in order, and every request is handled on the main actor.
public final class SocketServer: @unchecked Sendable {
    public typealias Handler = @MainActor @Sendable (JSON) async -> JSON

    public let path: String
    private let handler: Handler
    private var listener: Int32 = -1

    public static var defaultPath: String {
        if let path = ProcessInfo.processInfo.environment["KMUX_SOCKET"], !path.isEmpty { return path }
        let support = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        return support.appendingPathComponent("kmux/kmux.sock").path
    }

    public init(path: String = SocketServer.defaultPath, handler: @escaping Handler) {
        self.path = path
        self.handler = handler
    }

    public func start() throws {
        guard path.utf8.count < MemoryLayout.size(ofValue: sockaddr_un().sun_path) else {
            throw KmuxError("bad_request", "socket path is too long: \(path)")
        }
        try FileManager.default.createDirectory(atPath: (path as NSString).deletingLastPathComponent, withIntermediateDirectories: true)
        if Self.isListening(path) { throw KmuxError("bad_request", "another kmux is already listening on \(path)") }
        unlink(path)

        listener = socket(AF_UNIX, SOCK_STREAM, 0)
        guard listener >= 0 else { throw posixError("socket") }
        var address = Self.address(path)
        let bound = withUnsafePointer(to: &address) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) { bind(listener, $0, socklen_t(MemoryLayout<sockaddr_un>.size)) }
        }
        guard bound == 0 else { throw posixError("bind") }
        chmod(path, 0o600)
        guard listen(listener, 16) == 0 else { throw posixError("listen") }

        let thread = Thread { [self] in acceptLoop() }
        thread.name = "kmux.socket"
        thread.start()
    }

    public func stop() {
        if listener >= 0 { close(listener) }
        listener = -1
        unlink(path)
    }

    private func acceptLoop() {
        while true {
            let client = accept(listener, nil, nil)
            if client < 0 {
                if errno == EINTR { continue }
                return
            }
            var noSigPipe: Int32 = 1
            setsockopt(client, SOL_SOCKET, SO_NOSIGPIPE, &noSigPipe, socklen_t(MemoryLayout<Int32>.size))
            Thread { [self] in serve(client) }.start()
        }
    }

    private func serve(_ client: Int32) {
        defer { close(client) }
        var buffer = Data()
        var chunk = [UInt8](repeating: 0, count: 65536)
        while true {
            let count = read(client, &chunk, chunk.count)
            if count <= 0 { return }
            buffer.append(chunk, count: count)
            while let newline = buffer.firstIndex(of: UInt8(ascii: "\n")) {
                let line = buffer[buffer.startIndex..<newline]
                buffer.removeSubrange(buffer.startIndex...newline)
                if line.allSatisfy({ $0 == UInt8(ascii: " ") || $0 == UInt8(ascii: "\r") }) { continue }
                var reply = respond(Data(line)).encoded()
                reply.append(UInt8(ascii: "\n"))
                guard writeAll(client, reply) else { return }
            }
        }
    }

    private func respond(_ line: Data) -> JSON {
        guard let request = try? JSON.parse(line) else {
            return ["id": nil, "ok": false, "error": ["code": "bad_request", "message": "request is not valid JSON"]]
        }
        let box = ReplyBox()
        let done = DispatchSemaphore(value: 0)
        let handler = handler
        Task { @MainActor in
            box.value = await handler(request)
            done.signal()
        }
        done.wait()
        return box.value
    }

    private func writeAll(_ fd: Int32, _ data: Data) -> Bool {
        data.withUnsafeBytes { raw in
            var offset = 0
            while offset < raw.count {
                let written = write(fd, raw.baseAddress! + offset, raw.count - offset)
                if written < 0 {
                    if errno == EINTR { continue }
                    return false
                }
                offset += written
            }
            return true
        }
    }

    static func address(_ path: String) -> sockaddr_un {
        var address = sockaddr_un()
        address.sun_family = sa_family_t(AF_UNIX)
        withUnsafeMutableBytes(of: &address.sun_path) { raw in
            let bytes = Array(path.utf8)
            raw.copyBytes(from: bytes.prefix(raw.count - 1))
        }
        return address
    }

    static func isListening(_ path: String) -> Bool {
        let fd = socket(AF_UNIX, SOCK_STREAM, 0)
        guard fd >= 0 else { return false }
        defer { close(fd) }
        var address = address(path)
        return withUnsafePointer(to: &address) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) { connect(fd, $0, socklen_t(MemoryLayout<sockaddr_un>.size)) }
        } == 0
    }

    private func posixError(_ call: String) -> KmuxError {
        KmuxError("bad_request", "\(call) failed on \(path): \(String(cString: strerror(errno)))")
    }
}

private final class ReplyBox: @unchecked Sendable {
    var value: JSON = .null
}
