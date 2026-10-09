import Darwin
import Foundation

/// The app's connection to Kanna's daemon: one Unix socket carrying JSON
/// lines both ways (docs/kanna-spec.md §4.5). It connects when the daemon is
/// there and again whenever it comes back; what it reads is handed to the
/// main actor in order.
final class DaemonConnection: @unchecked Sendable {
    let path: String
    /// Called on the main actor: connected, each message, disconnected.
    private let connected: @MainActor @Sendable () -> Void
    private let received: @MainActor @Sendable (JSON) -> Void
    private let disconnected: @MainActor @Sendable () -> Void
    private let writes = DispatchQueue(label: "kanna.daemon.write")
    private let lock = NSLock()
    private var fd: Int32 = -1
    private var stopped = false

    init(path: String, connected: @escaping @MainActor @Sendable () -> Void, received: @escaping @MainActor @Sendable (JSON) -> Void,
         disconnected: @escaping @MainActor @Sendable () -> Void) {
        self.path = path
        self.connected = connected
        self.received = received
        self.disconnected = disconnected
    }

    func start() {
        let thread = Thread { [self] in run() }
        thread.name = "kanna.daemon.read"
        thread.start()
    }

    func stop() {
        lock.withLock {
            stopped = true
            if fd >= 0 { shutdown(fd, SHUT_RDWR) }
        }
    }

    /// Sends one message; dropped if the daemon isn't there.
    func send(_ message: JSON) {
        var encoded = message.encoded()
        encoded.append(UInt8(ascii: "\n"))
        let line = encoded
        writes.async { [self] in
            let fd = lock.withLock { self.fd }
            if fd >= 0 { _ = Self.writeAll(fd, line) }
        }
    }

    private func run() {
        while !lock.withLock({ stopped }) {
            guard let socket = Self.connect(path) else {
                Thread.sleep(forTimeInterval: 0.2)
                continue
            }
            lock.withLock { fd = socket }
            let connected = connected
            DispatchQueue.main.async { MainActor.assumeIsolated { connected() } }
            readLines(socket)
            // Writes in flight finish (or fail) before the descriptor goes.
            writes.sync { lock.withLock { fd = -1 } }
            close(socket)
            let disconnected = disconnected
            DispatchQueue.main.async { MainActor.assumeIsolated { disconnected() } }
            Thread.sleep(forTimeInterval: 0.5)
        }
    }

    private func readLines(_ socket: Int32) {
        var buffer = Data()
        var chunk = [UInt8](repeating: 0, count: 65536)
        while true {
            let count = read(socket, &chunk, chunk.count)
            if count < 0 && errno == EINTR { continue }
            if count <= 0 { return }
            buffer.append(chunk, count: count)
            while let newline = buffer.firstIndex(of: UInt8(ascii: "\n")) {
                let line = Data(buffer[buffer.startIndex..<newline])
                buffer.removeSubrange(buffer.startIndex...newline)
                guard let message = try? JSON.parse(line) else {
                    FileHandle.standardError.write(Data("kanna: the daemon sent a line that isn't JSON\n".utf8))
                    continue
                }
                let received = received
                DispatchQueue.main.async { MainActor.assumeIsolated { received(message) } }
            }
        }
    }

    private static func connect(_ path: String) -> Int32? {
        let fd = socket(AF_UNIX, SOCK_STREAM, 0)
        guard fd >= 0 else { return nil }
        var address = SocketServer.address(path)
        let ok = withUnsafePointer(to: &address) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) { Darwin.connect(fd, $0, socklen_t(MemoryLayout<sockaddr_un>.size)) }
        } == 0
        guard ok else {
            close(fd)
            return nil
        }
        var noSigPipe: Int32 = 1
        setsockopt(fd, SOL_SOCKET, SO_NOSIGPIPE, &noSigPipe, socklen_t(MemoryLayout<Int32>.size))
        return fd
    }

    private static func writeAll(_ fd: Int32, _ data: Data) -> Bool {
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
}
