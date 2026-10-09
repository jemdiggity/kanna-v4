struct Pane: Hashable, Sendable {
    let id: String
}

@MainActor
protocol TerminalBackend: AnyObject {
    func start(_ view: TerminalSurfaceView, pane: Pane) -> Bool
    func stop(_ view: TerminalSurfaceView)
}
