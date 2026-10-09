// From kmux apps/kmux/Sources/KmuxCore/SocketServer.swift (at kmux.rev); its error is KannaError.
import Darwin
import Foundation

/// The control socket: a Unix domain socket carrying newline-delimited JSON.
/// Each connection is served on its own thread; requests on a connection are
/// answered in order, and every request is handled on the main actor.
final class SocketServer: @unchecked Sendable {
    typealias Handler = @MainActor @Sendable (JSON) async -> JSON

    let path: String
    private let handler: Handler
    private let lock = NSLock()
    private var listener: Int32 = -1
    private var connections = 0
    private static let maxConnections = 16
    private static let maxLineBytes = 1 << 20

    init(path: String, handler: @escaping Handler) {
        self.path = path
        self.handler = handler
    }

    func start() throws {
        guard path.utf8.count < MemoryLayout.size(ofValue: sockaddr_un().sun_path) else {
            throw KannaError("bad_request", "socket path is too long: \(path)")
        }
        try FileManager.default.createDirectory(atPath: (path as NSString).deletingLastPathComponent, withIntermediateDirectories: true)
        if Self.isListening(path) { throw KannaError("bad_request", "another Kanna.app is already listening on \(path)") }
        var info = stat()
        if lstat(path, &info) == 0 {
            guard info.st_mode & S_IFMT == S_IFSOCK else { throw KannaError("bad_request", "\(path) exists and is not a socket") }
            unlink(path)
        }

        let fd = socket(AF_UNIX, SOCK_STREAM, 0)
        guard fd >= 0 else { throw posixError("socket") }
        var address = Self.address(path)
        let mask = umask(0o177)
        let bound = withUnsafePointer(to: &address) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) { bind(fd, $0, socklen_t(MemoryLayout<sockaddr_un>.size)) }
        }
        umask(mask)
        guard bound == 0 else {
            let error = posixError("bind")
            close(fd)
            throw error
        }
        guard listen(fd, 16) == 0 else {
            let error = posixError("listen")
            close(fd)
            unlink(path)
            throw error
        }
        lock.withLock { listener = fd }

        let thread = Thread { [self] in acceptLoop(fd) }
        thread.name = "kanna.socket"
        thread.start()
    }

    func stop() {
        let fd = lock.withLock {
            let fd = listener
            listener = -1
            return fd
        }
        guard fd >= 0 else { return }
        shutdown(fd, SHUT_RDWR)
        close(fd)
        var info = stat()
        if lstat(path, &info) == 0, info.st_mode & S_IFMT == S_IFSOCK { unlink(path) }
    }

    private func acceptLoop(_ fd: Int32) {
        while true {
            let client = accept(fd, nil, nil)
            if client < 0 {
                if errno == EINTR { continue }
                return
            }
            let admitted = lock.withLock {
                guard listener == fd, connections < Self.maxConnections else { return false }
                connections += 1
                return true
            }
            guard admitted else {
                close(client)
                continue
            }
            var noSigPipe: Int32 = 1
            setsockopt(client, SOL_SOCKET, SO_NOSIGPIPE, &noSigPipe, socklen_t(MemoryLayout<Int32>.size))
            Thread { [self] in
                serve(client)
                lock.withLock { connections -= 1 }
            }.start()
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
                var reply = (line.count > Self.maxLineBytes ? Self.tooLong : respond(Data(line))).encoded()
                reply.append(UInt8(ascii: "\n"))
                guard writeAll(client, reply) else { return }
            }
            if buffer.count > Self.maxLineBytes {
                var reply = Self.tooLong.encoded()
                reply.append(UInt8(ascii: "\n"))
                _ = writeAll(client, reply)
                return
            }
        }
    }

    private static let tooLong: JSON = ["id": nil, "ok": false, "error": ["code": "bad_request", "message": "request is longer than 1 MiB"]]

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

    private func posixError(_ call: String) -> KannaError {
        KannaError("bad_request", "\(call) failed on \(path): \(String(cString: strerror(errno)))")
    }
}

private final class ReplyBox: @unchecked Sendable {
    var value: JSON = .null
}
