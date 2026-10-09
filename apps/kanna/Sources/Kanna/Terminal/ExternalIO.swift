// From kmux's spike/ghostty-external-io, apps/kmux/Sources/Kmux/Terminal/ExternalIO.swift.
// Kanna's version: what the surface writes and asks for goes to a
// TerminalLink (the far end that owns the terminal) instead of being
// recorded for debug.io, and output the link pushes is queued in order when
// Ghostty's mailbox is full. Built only against GhosttyKit from the Ghostty
// fork (kmux-external-io); see kmux's docs/research/ghostty-external-io.md.
import AppKit
import GhosttyKit

/// The far end of external-I/O terminals: whatever owns each terminal and
/// the program in it: Kanna's daemon (DaemonLink), or EchoLink in echo mode.
@MainActor
protocol TerminalLink: AnyObject {
    /// `terminal`'s surface is up; output may be pushed to it from now on.
    func opened(_ terminal: ExternalTerminal)
    /// What the surface wrote out, as it would to a pty: keys, paste, mouse
    /// and focus reports. Never replies to terminal queries (the fork drops those).
    func input(_ bytes: Data, from terminal: ExternalTerminal)
    /// The view now fits `cols`×`rows` cells. The terminal keeps its grid
    /// until the link applies one (`setGrid`): the owner decides the size.
    func resizeRequested(_ terminal: ExternalTerminal, cols: UInt16, rows: UInt16, width: UInt32, height: UInt32)
    /// The pane is closing; the surface is about to be freed.
    func closed(_ terminal: ExternalTerminal)
}

/// One surface with no pty: it parses and draws the output its link pushes,
/// and sends what is typed to the link.
@MainActor
final class ExternalTerminal {
    let pane: Pane
    fileprivate weak var view: TerminalSurfaceView?
    var surface: ghostty_surface_t? { view?.surface }

    /// Output and grid changes not yet taken by Ghostty's mailbox, in stream order.
    private enum Pending { case output(Data), grid(UInt16, UInt16) }
    private var pending: [Pending] = []
    private var retrying = false

    fileprivate init(pane: Pane, view: TerminalSurfaceView) {
        self.pane = pane
        self.view = view
    }

    /// Bytes the program wrote, to parse and draw.
    func output(_ bytes: Data) {
        guard !bytes.isEmpty else { return }
        enqueue(.output(bytes))
    }

    /// The terminal's size in cells, applied in order with output.
    func setGrid(cols: UInt16, rows: UInt16) { enqueue(.grid(cols, rows)) }

    /// Replaces the screen with a GHOSTSNP snapshot; false if it was refused.
    func loadSnapshot(_ bytes: Data) -> Bool {
        guard let surface else { return false }
        return bytes.withUnsafeBytes { ghostty_surface_load_snapshot(surface, $0.bindMemory(to: UInt8.self).baseAddress, bytes.count) }
    }

    /// The program exited with `status`; the pane shows it (show_child_exited).
    func exited(status: Int32) {
        if let surface { ghostty_surface_set_process_exited(surface, status) }
    }

    private func enqueue(_ item: Pending) {
        pending.append(item)
        drain()
    }

    /// Hands queued items to Ghostty until its mailbox is full, then retries
    /// shortly after; nothing overtakes what is still queued.
    private func drain() {
        guard let surface else { return pending.removeAll() }
        while let item = pending.first {
            let taken = switch item {
            case .output(let bytes): bytes.withUnsafeBytes { ghostty_surface_process_output(surface, $0.bindMemory(to: UInt8.self).baseAddress, bytes.count) }
            case .grid(let cols, let rows): ghostty_surface_set_grid_size(surface, cols, rows)
            }
            guard taken else { return retryLater() }
            pending.removeFirst()
        }
    }

    private func retryLater() {
        guard !retrying else { return }
        retrying = true
        DispatchQueue.main.asyncAfter(deadline: .now() + .milliseconds(2)) { [weak self] in
            MainActor.assumeIsolated {
                self?.retrying = false
                self?.drain()
            }
        }
    }
}

/// Terminal panes whose surfaces have no pty: they draw what `link` pushes
/// and send it what they write.
@MainActor
final class ExternalIOBackend: TerminalBackend {
    /// Ghostty's I/O callbacks carry only the surface userdata (the view).
    nonisolated(unsafe) private static weak var shared: ExternalIOBackend?
    private let runtime: GhosttyRuntime
    private let link: TerminalLink
    private var terminals: [UInt: ExternalTerminal] = [:]

    init(runtime: GhosttyRuntime, link: TerminalLink) {
        self.runtime = runtime
        self.link = link
        Self.shared = self
    }

    func terminal(_ view: TerminalSurfaceView) -> ExternalTerminal? { terminals[view.address] }

    func start(_ view: TerminalSurfaceView, pane: Pane) -> Bool {
        var options = GhosttyRuntime.options(for: view)
        options.io_mode = GHOSTTY_IO_MODE_EXTERNAL
        // Called on Ghostty's I/O thread.
        options.io_write_cb = { userdata, bytes, count in
            guard let bytes, count > 0 else { return }
            let data = Data(bytes: bytes, count: count)
            ExternalIOBackend.onMain(userdata) { backend, terminal in backend.link.input(data, from: terminal) }
        }
        // Called on the main thread when the view's size changes; the
        // terminal keeps its grid until the link applies one.
        options.io_resize_request_cb = { userdata, cols, rows, width, height in
            ExternalIOBackend.onMain(userdata) { backend, terminal in
                backend.link.resizeRequested(terminal, cols: cols, rows: rows, width: width, height: height)
            }
        }
        let terminal = ExternalTerminal(pane: pane, view: view)
        terminals[view.address] = terminal
        guard runtime.attach(view, options: &options) else {
            terminals.removeValue(forKey: view.address)
            return false
        }
        link.opened(terminal)
        return true
    }

    func stop(_ view: TerminalSurfaceView) {
        if let terminal = terminals.removeValue(forKey: view.address) { link.closed(terminal) }
        runtime.detach(view)
    }

    /// Runs `body` for the callback's terminal on the main thread, always
    /// later: never inside Ghostty's own call (a resize is reported while the
    /// surface is still being created or resized).
    nonisolated private static func onMain(_ userdata: UnsafeMutableRawPointer?, _ body: @escaping @MainActor (ExternalIOBackend, ExternalTerminal) -> Void) {
        let address = UInt(bitPattern: userdata)
        DispatchQueue.main.async {
            MainActor.assumeIsolated {
                guard let backend = ExternalIOBackend.shared, let terminal = backend.terminals[address] else { return }
                body(backend, terminal)
            }
        }
    }
}
