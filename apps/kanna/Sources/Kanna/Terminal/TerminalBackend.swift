// From kmux's spike/ghostty-external-io, apps/kmux/Sources/Kmux/Terminal/TerminalBackend.swift.
// Trimmed for Kanna: the pane is a bare identity (no KmuxCore model), and
// there is no pty backend, since Kanna's terminals never run in the app.

/// A pane, as far as its terminal's backend needs to know: which one it is.
/// The layout around it (windows, tabs, splits) is the app's alone.
struct Pane: Hashable, Sendable {
    let id: String
}

/// What runs a terminal pane behind its Ghostty surface. In Kanna the
/// terminal lives outside the app (in the daemon, from step 0.4), so the
/// surface has no pty of its own: see ExternalIOBackend.
@MainActor
protocol TerminalBackend: AnyObject {
    /// Creates `view`'s surface for `pane`; false if it could not start.
    func start(_ view: TerminalSurfaceView, pane: Pane) -> Bool
    /// Frees `view`'s surface; the pane is closing.
    func stop(_ view: TerminalSurfaceView)
}
