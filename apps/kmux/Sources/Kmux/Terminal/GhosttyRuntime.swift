// Runtime callbacks follow Ghostty macOS Ghostty.App.swift (MIT; Ghostty 4ec3122), via kanna v3.
import AppKit
import GhosttyKit

/// The process-wide Ghostty app. Every surface's userdata is its
/// TerminalSurfaceView, which the callbacks resolve through `live`.
@MainActor
final class GhosttyRuntime {
    /// Ghostty's C callbacks carry no context for the app itself.
    nonisolated(unsafe) static var shared: GhosttyRuntime?

    let app: ghostty_app_t
    private let config: ghostty_config_t
    /// Keyed by the surface userdata address, so a callback for a surface that
    /// was already freed finds nothing instead of a dangling object.
    private var live: [UInt: TerminalSurfaceView] = [:]
    private var timer: Timer?
    var onChildExited: ((TerminalSurfaceView, Int) -> Void)?
    var onClose: ((TerminalSurfaceView) -> Void)?
    var onMuxAction: ((TerminalSurfaceView, ghostty_action_s) -> Bool)?

    init() throws {
        guard let cfg = ghostty_config_new() else { throw Self.error("ghostty_config_new failed") }
        ghostty_config_load_default_files(cfg)
        ghostty_config_load_recursive_files(cfg)
        ghostty_config_finalize(cfg)

        var runtime = ghostty_runtime_config_s()
        runtime.supports_selection_clipboard = true
        runtime.wakeup_cb = { _ in DispatchQueue.main.async { MainActor.assumeIsolated { GhosttyRuntime.shared?.tick() } } }
        runtime.action_cb = { _, target, action in
            guard target.tag == GHOSTTY_TARGET_SURFACE,
                  let userdata = target.target.surface.flatMap({ ghostty_surface_userdata($0) }) else { return false }
            let address = UInt(bitPattern: userdata)
            if action.tag != GHOSTTY_ACTION_SHOW_CHILD_EXITED {
                // Mux actions (splits, tabs, windows) from the user's Ghostty
                // keybindings, when no menu item took the key first.
                guard Thread.isMainThread else { return false }
                return MainActor.assumeIsolated {
                    guard let runtime = GhosttyRuntime.shared, let view = runtime.live[address] else { return false }
                    return runtime.onMuxAction?(view, action) ?? false
                }
            }
            let code = Int(action.action.child_exited.exit_code)
            let report: @Sendable () -> Void = { MainActor.assumeIsolated { GhosttyRuntime.shared?.childExited(address, code) } }
            if Thread.isMainThread { report() } else { DispatchQueue.main.async { report() } }
            return true
        }
        runtime.read_clipboard_cb = { userdata, _, state, _, _, _ in
            let address = UInt(bitPattern: userdata)
            return MainActor.assumeIsolated { GhosttyRuntime.shared?.readClipboard(address, state: state) ?? GHOSTTY_CLIPBOARD_READ_UNSUPPORTED }
        }
        runtime.confirm_read_clipboard_cb = { userdata, confirm, state, request in
            let address = UInt(bitPattern: userdata)
            MainActor.assumeIsolated { GhosttyRuntime.shared?.confirmClipboard(address, confirm, state: state, request: request) }
        }
        runtime.write_clipboard_cb = { _, _, content, count, confirm in
            MainActor.assumeIsolated { GhosttyRuntime.writeClipboard(content: content, count: count, confirm: confirm) }
        }
        runtime.close_surface_cb = { userdata, _ in
            let address = UInt(bitPattern: userdata)
            DispatchQueue.main.async {
                MainActor.assumeIsolated {
                    guard let runtime = GhosttyRuntime.shared, let view = runtime.live[address] else { return }
                    runtime.onClose?(view)
                }
            }
        }
        guard let value = ghostty_app_new(&runtime, cfg) else {
            ghostty_config_free(cfg)
            throw Self.error("ghostty_app_new failed")
        }
        app = value
        config = cfg
        ghostty_app_set_focus(value, true)
        Self.shared = self
        timer = Timer.scheduledTimer(withTimeInterval: 1.0 / 60.0, repeats: true) { _ in
            MainActor.assumeIsolated { GhosttyRuntime.shared?.tick() }
        }
    }

    /// Starts `command` (or the user's shell) in a new surface drawn by `view`.
    /// With a command, the pane stays on screen after it exits.
    func attach(_ view: TerminalSurfaceView, command: String?, cwd: String?) -> Bool {
        var options = ghostty_surface_config_new()
        options.platform_tag = GHOSTTY_PLATFORM_MACOS
        options.platform = ghostty_platform_u(macos: ghostty_platform_macos_s(nsview: Unmanaged.passUnretained(view).toOpaque()))
        options.userdata = Unmanaged.passUnretained(view).toOpaque()
        options.scale_factor = Double(NSScreen.main?.backingScaleFactor ?? 2)
        options.wait_after_command = command != nil
        let directory = cwd.map { ($0 as NSString).expandingTildeInPath }
        // Ghostty runs a command as `exec -l <command>`, which takes a single
        // program; going through the user's shell keeps `a; b` and pipes
        // working, and `exec -l` still makes it a login shell.
        let command = command.map { "\(Self.shell) -c \(Self.quote($0))" }
        let surface = withOptionalCString(command) { commandPointer in
            withOptionalCString(directory) { directoryPointer in
                options.command = commandPointer
                options.working_directory = directoryPointer
                return ghostty_surface_new(app, &options)
            }
        }
        guard let surface else { return false }
        live[view.address] = view
        view.surface = surface
        return true
    }

    /// The menu shortcut the user's Ghostty config binds to `action`.
    func shortcut(for action: String) -> Shortcut? {
        Shortcut(ghostty_config_trigger(config, action, UInt(action.utf8.count)))
    }

    func detach(_ view: TerminalSurfaceView) {
        live.removeValue(forKey: view.address)
        if let surface = view.surface { ghostty_surface_free(surface) }
        view.surface = nil
    }

    private static let shell = ProcessInfo.processInfo.environment["SHELL"].flatMap { $0.isEmpty ? nil : $0 } ?? "/bin/zsh"

    private static func quote(_ text: String) -> String { "'" + text.replacingOccurrences(of: "'", with: "'\\''") + "'" }

    private func tick() { ghostty_app_tick(app) }

    private func childExited(_ address: UInt, _ code: Int) {
        guard let view = live[address] else { return }
        onChildExited?(view, code)
    }

    private func readClipboard(_ address: UInt, state: UnsafeMutableRawPointer?) -> ghostty_clipboard_read_result_e {
        // Only a user-initiated paste may read; OSC 52 reads arrive with no
        // input event in flight and are answered with nothing.
        guard let view = live[address], view.isHandlingInputEvent, let surface = view.surface,
              let string = NSPasteboard.general.string(forType: .string) else { return GHOSTTY_CLIPBOARD_READ_UNAVAILABLE }
        let mime = strdup("text/plain")!
        let text = strdup(string)!
        defer { free(mime); free(text) }
        var item = ghostty_clipboard_content_s(mime: mime, data: text, len: strlen(text))
        withUnsafePointer(to: &item) { itemPointer in
            var result = ghostty_clipboard_complete_s(contents: itemPointer, contents_len: 1, available: nil, available_len: 0, confirmed: false, remember: false)
            ghostty_surface_complete_clipboard_request(surface, &result, state)
        }
        return GHOSTTY_CLIPBOARD_READ_STARTED
    }

    private func confirmClipboard(_ address: UInt, _ confirm: UnsafePointer<ghostty_clipboard_confirm_s>?, state: UnsafeMutableRawPointer?, request: ghostty_clipboard_request_e) {
        guard let surface = live[address]?.surface else { return }
        guard request == GHOSTTY_CLIPBOARD_REQUEST_PASTE, let confirm else {
            ghostty_surface_deny_clipboard_request(surface, state)
            return
        }
        let alert = NSAlert()
        alert.messageText = "Allow terminal paste?"
        alert.informativeText = "The terminal requests clipboard data."
        alert.addButton(withTitle: "Paste")
        alert.addButton(withTitle: "Cancel")
        guard alert.runModal() == .alertFirstButtonReturn else {
            ghostty_surface_deny_clipboard_request(surface, state)
            return
        }
        let value = confirm.pointee
        let items: [ghostty_clipboard_content_s] = value.contents.map { contents in (0..<value.contents_len).map { contents[$0] } } ?? []
        var complete = ghostty_clipboard_complete_s(contents: nil, contents_len: 0, available: value.available, available_len: value.available_len, confirmed: true, remember: false)
        items.withUnsafeBufferPointer { buffer in
            complete.contents = buffer.baseAddress
            complete.contents_len = buffer.count
            ghostty_surface_complete_clipboard_request(surface, &complete, state)
        }
    }

    private static func writeClipboard(content: UnsafePointer<ghostty_clipboard_content_s>?, count: Int, confirm: Bool) {
        guard let content, count > 0 else { return }
        let value = content[0]
        guard String(cString: value.mime) == "text/plain", let bytes = value.data else { return }
        let text = String(decoding: UnsafeRawBufferPointer(start: bytes, count: value.len), as: UTF8.self)
        if confirm {
            let alert = NSAlert()
            alert.messageText = "Allow terminal clipboard write?"
            alert.informativeText = text
            alert.addButton(withTitle: "Allow")
            alert.addButton(withTitle: "Cancel")
            guard alert.runModal() == .alertFirstButtonReturn else { return }
        }
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(text, forType: .string)
    }

    private static func error(_ message: String) -> NSError { NSError(domain: "kmux", code: 1, userInfo: [NSLocalizedDescriptionKey: message]) }
}

private func withOptionalCString<T>(_ string: String?, _ body: (UnsafePointer<CChar>?) -> T) -> T {
    guard let string else { return body(nil) }
    return string.withCString { body($0) }
}
