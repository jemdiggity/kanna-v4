import AppKit
import GhosttyKit

@MainActor
final class GhosttyRuntime {
    nonisolated(unsafe) static var shared: GhosttyRuntime?

    let app: ghostty_app_t
    private let config: ghostty_config_t
    private var live: [UInt: TerminalSurfaceView] = [:]
    private var timer: Timer?

    init() throws {
        guard let cfg = ghostty_config_new() else { throw Self.error("ghostty_config_new failed") }
        ghostty_config_load_default_files(cfg)
        ghostty_config_load_recursive_files(cfg)
        ghostty_config_finalize(cfg)

        var runtime = ghostty_runtime_config_s()
        runtime.supports_selection_clipboard = true
        runtime.wakeup_cb = { _ in DispatchQueue.main.async { MainActor.assumeIsolated { GhosttyRuntime.shared?.tick() } } }
        runtime.action_cb = { _, _, _ in false }
        runtime.read_clipboard_cb = { userdata, _, state, _, _, _ in
            let id = UInt(bitPattern: userdata)
            return MainActor.assumeIsolated { GhosttyRuntime.shared?.readClipboard(id, state: state) ?? GHOSTTY_CLIPBOARD_READ_UNSUPPORTED }
        }
        runtime.confirm_read_clipboard_cb = { userdata, confirm, state, request in
            let id = UInt(bitPattern: userdata)
            MainActor.assumeIsolated { GhosttyRuntime.shared?.confirmClipboard(id, confirm, state: state, request: request) }
        }
        runtime.write_clipboard_cb = { userdata, _, content, count, confirm in
            let id = UInt(bitPattern: userdata)
            MainActor.assumeIsolated { GhosttyRuntime.shared?.writeClipboard(id, content: content, count: count, confirm: confirm) }
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

    static func options(for view: TerminalSurfaceView) -> ghostty_surface_config_s {
        var options = ghostty_surface_config_new()
        options.platform_tag = GHOSTTY_PLATFORM_MACOS
        options.platform = ghostty_platform_u(macos: ghostty_platform_macos_s(nsview: Unmanaged.passUnretained(view).toOpaque()))
        options.userdata = UnsafeMutableRawPointer(bitPattern: view.surfaceID)
        options.scale_factor = Double(NSScreen.main?.backingScaleFactor ?? 2)
        return options
    }

    func attach(_ view: TerminalSurfaceView, options: inout ghostty_surface_config_s) -> Bool {
        guard let surface = ghostty_surface_new(app, &options) else { return false }
        live[view.surfaceID] = view
        view.surface = surface
        return true
    }

    func detach(_ view: TerminalSurfaceView) {
        live.removeValue(forKey: view.surfaceID)
        if let surface = view.surface { ghostty_surface_free(surface) }
        view.surface = nil
    }

    private func tick() { ghostty_app_tick(app) }

    private func readClipboard(_ id: UInt, state: UnsafeMutableRawPointer?) -> ghostty_clipboard_read_result_e {
        guard let view = live[id], view.isHandlingInputEvent, let surface = view.surface,
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

    private func confirmClipboard(_ id: UInt, _ confirm: UnsafePointer<ghostty_clipboard_confirm_s>?, state: UnsafeMutableRawPointer?, request: ghostty_clipboard_request_e) {
        let items = request == GHOSTTY_CLIPBOARD_REQUEST_PASTE ? confirm.map { ClipboardItems($0.pointee) } : nil
        let token = UInt(bitPattern: state)
        DispatchQueue.main.async {
            MainActor.assumeIsolated { GhosttyRuntime.shared?.askToPaste(id, items, state: token) }
        }
    }

    private func askToPaste(_ id: UInt, _ items: ClipboardItems?, state: UInt) {
        guard let view = live[id], let surface = view.surface else { return }
        guard let items, let window = view.window else {
            return ghostty_surface_deny_clipboard_request(surface, UnsafeMutableRawPointer(bitPattern: state))
        }
        let alert = NSAlert()
        alert.messageText = "Allow terminal paste?"
        alert.informativeText = "The terminal requests clipboard data."
        alert.addButton(withTitle: "Paste")
        alert.addButton(withTitle: "Cancel")
        alert.beginSheetModal(for: window) { response in
            MainActor.assumeIsolated {
                guard let surface = GhosttyRuntime.shared?.live[id]?.surface else { return }
                let state = UnsafeMutableRawPointer(bitPattern: state)
                if response == .alertFirstButtonReturn {
                    items.complete(surface, state: state)
                } else {
                    ghostty_surface_deny_clipboard_request(surface, state)
                }
            }
        }
    }

    private func writeClipboard(_ id: UInt, content: UnsafePointer<ghostty_clipboard_content_s>?, count: Int, confirm: Bool) {
        guard let content, count > 0 else { return }
        let value = content[0]
        guard let mime = value.mime, String(cString: mime) == "text/plain", let bytes = value.data else { return }
        let text = String(decoding: UnsafeRawBufferPointer(start: bytes, count: value.len), as: UTF8.self)
        guard confirm else { return Self.setClipboard(text) }
        DispatchQueue.main.async {
            MainActor.assumeIsolated {
                guard let window = GhosttyRuntime.shared?.live[id]?.window else { return }
                let alert = NSAlert()
                alert.messageText = "Allow terminal clipboard write?"
                alert.informativeText = text
                alert.addButton(withTitle: "Allow")
                alert.addButton(withTitle: "Cancel")
                alert.beginSheetModal(for: window) { response in
                    if response == .alertFirstButtonReturn { MainActor.assumeIsolated { Self.setClipboard(text) } }
                }
            }
        }
    }

    private static func setClipboard(_ text: String) {
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(text, forType: .string)
    }

    private static func error(_ message: String) -> NSError { NSError(domain: "kanna", code: 1, userInfo: [NSLocalizedDescriptionKey: message]) }
}

private struct ClipboardItems: Sendable {
    let contents: [(mime: String, data: Data)]
    let available: [String]

    init(_ value: ghostty_clipboard_confirm_s) {
        contents = (0..<value.contents_len).compactMap { index in
            guard let item = value.contents?[index], let mime = item.mime else { return nil }
            return (String(cString: mime), item.data.map { Data(bytes: $0, count: item.len) } ?? Data())
        }
        available = (0..<value.available_len).compactMap { index in value.available?[index].map { String(cString: $0) } }
    }

    @MainActor
    func complete(_ surface: ghostty_surface_t, state: UnsafeMutableRawPointer?) {
        let mimes = contents.map { strdup($0.mime) }
        let datas = contents.map { item -> UnsafeMutablePointer<CChar> in
            let buffer = UnsafeMutablePointer<CChar>.allocate(capacity: max(1, item.data.count))
            item.data.copyBytes(to: UnsafeMutableRawPointer(buffer).assumingMemoryBound(to: UInt8.self), count: item.data.count)
            return buffer
        }
        let names: [UnsafePointer<CChar>?] = available.map { UnsafePointer(strdup($0)) }
        defer {
            mimes.forEach { free($0) }
            datas.forEach { $0.deallocate() }
            names.forEach { free(UnsafeMutableRawPointer(mutating: $0)) }
        }
        let items = contents.indices.map { ghostty_clipboard_content_s(mime: mimes[$0], data: datas[$0], len: contents[$0].data.count) }
        items.withUnsafeBufferPointer { buffer in
            names.withUnsafeBufferPointer { available in
                var complete = ghostty_clipboard_complete_s(contents: buffer.baseAddress, contents_len: buffer.count, available: available.baseAddress,
                                                            available_len: available.count, confirmed: true, remember: false)
                ghostty_surface_complete_clipboard_request(surface, &complete, state)
            }
        }
    }
}
