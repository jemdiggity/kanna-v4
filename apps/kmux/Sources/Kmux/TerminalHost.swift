import AppKit
import KmuxCore

/// Runs pane contents: Ghostty terminals for `term` panes.
@MainActor
final class TerminalHost: PaneHost {
    let runtime: GhosttyRuntime
    weak var core: Core?
    private(set) var views: [String: PaneView] = [:]
    var onFocus: ((String) -> Void)?
    var onCloseRequest: ((String) -> Void)?

    init(runtime: GhosttyRuntime) {
        self.runtime = runtime
        runtime.onChildExited = { [weak self] view, code in
            guard let id = self?.paneID(of: view) else { return }
            self?.core?.update(id, state: .exited, exitCode: code)
        }
        runtime.onClose = { [weak self] view in
            guard let id = self?.paneID(of: view) else { return }
            self?.onCloseRequest?(id)
        }
    }

    func terminal(_ id: String) -> TerminalSurfaceView? { views[id]?.content as? TerminalSurfaceView }

    func start(_ pane: Pane) {
        let id = pane.id
        let terminal = TerminalSurfaceView(frame: NSRect(x: 0, y: 0, width: 800, height: 600))
        terminal.onFocus = { [weak self] in self?.onFocus?(id) }
        let view = PaneView(id: id, content: terminal)
        views[id] = view
        let started = runtime.attach(terminal, command: pane.command, cwd: pane.cwd)
        DispatchQueue.main.async { [weak self] in
            if started { self?.core?.update(id, state: .running) } else { self?.core?.update(id, state: .failed, error: "Ghostty could not create a terminal") }
        }
    }

    func stop(_ pane: Pane) {
        guard let view = views.removeValue(forKey: pane.id) else { return }
        if let terminal = view.content as? TerminalSurfaceView { runtime.detach(terminal) }
        view.removeFromSuperview()
    }

    func paneID(of terminal: TerminalSurfaceView) -> String? {
        views.first { $0.value.content === terminal }?.key
    }
}
