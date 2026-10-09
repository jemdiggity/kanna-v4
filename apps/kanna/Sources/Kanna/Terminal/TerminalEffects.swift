import AppKit
import GhosttyKit
import KannaLinks
import UserNotifications

@MainActor
final class TerminalEffects {
    let recording: Bool
    let general: NSPasteboard
    let selection: NSPasteboard
    private(set) var opened: [String] = []
    private(set) var links: [(url: String, decision: String)] = []
    private(set) var notifications: [(title: String, body: String)] = []
    private(set) var bells = 0
    var authorized: Bool?

    init(recording: Bool) {
        self.recording = recording
        if recording {
            general = NSPasteboard(name: NSPasteboard.Name("dev.kanna.debug.\(ProcessInfo.processInfo.processIdentifier)"))
            selection = NSPasteboard(name: NSPasteboard.Name("dev.kanna.debug.\(ProcessInfo.processInfo.processIdentifier).selection"))
        } else {
            general = .general
            selection = NSPasteboard(name: NSPasteboard.Name("dev.kanna.selection"))
        }
    }

    func pasteboard(_ location: ghostty_clipboard_e) -> NSPasteboard {
        location == GHOSTTY_CLIPBOARD_SELECTION ? selection : general
    }

    func read(_ location: ghostty_clipboard_e) -> String? {
        pasteboard(location).string(forType: .string)
    }

    func write(_ text: String, to location: ghostty_clipboard_e) {
        let board = pasteboard(location)
        board.clearContents()
        board.setString(text, forType: .string)
    }

    func open(_ text: String, kind: ghostty_action_open_url_kind_e, in window: NSWindow?) {
        if kind == GHOSTTY_ACTION_OPEN_URL_KIND_TEXT || kind == GHOSTTY_ACTION_OPEN_URL_KIND_HTML {
            return openScreenFile(text, asText: kind == GHOSTTY_ACTION_OPEN_URL_KIND_TEXT)
        }
        let link = kind == GHOSTTY_ACTION_OPEN_URL_KIND_OSC8 ? UntrustedURL(text) : UntrustedURL(clicked: text)
        let decision = link.decision
        if recording {
            links.append((text, decision.name))
            if case .allow(let url) = decision { opened.append(url.absoluteString) }
            return
        }
        switch decision {
        case .allow(let url): NSWorkspace.shared.open(url)
        case .confirm(let url): LinkAlert.confirm(url, showing: link.displayString, in: window)
        case .deny(let reason):
            LinkAlert.block(reason, showing: link.displayString, in: window) { [weak self] target in
                self?.write(target, to: GHOSTTY_CLIPBOARD_STANDARD)
            }
        }
    }

    private func openScreenFile(_ path: String, asText: Bool) {
        let url = URL(filePath: NSString(string: path).standardizingPath)
        if recording { return opened.append(url.absoluteString) }
        guard asText, let editor = NSWorkspace.shared.urlForApplication(toOpen: url) ?? NSWorkspace.shared.urlForApplication(toOpen: .plainText) else {
            NSWorkspace.shared.open(url)
            return
        }
        NSWorkspace.shared.open([url], withApplicationAt: editor, configuration: NSWorkspace.OpenConfiguration())
    }

    func ring(_ features: UInt32, in window: NSWindow?) {
        bells += 1
        if recording { return }
        if features & 0b1 != 0 { NSSound.beep() }
        if features & 0b100 != 0, !NSApp.isActive { NSApp.requestUserAttention(.informationalRequest) }
    }

    func notify(title: String, body: String) {
        if recording { return notifications.append((title, body)) }
        if authorized == true { return Self.post(title: title, body: body) }
        UNUserNotificationCenter.current().requestAuthorization(options: [.alert, .sound]) { granted, _ in
            Task { @MainActor in
                GhosttyRuntime.shared?.effects.authorized = granted
                if granted { TerminalEffects.post(title: title, body: body) }
            }
        }
    }

    private static func post(title: String, body: String) {
        let content = UNMutableNotificationContent()
        content.title = title.isEmpty ? "Kanna" : title
        content.body = body
        UNUserNotificationCenter.current().add(UNNotificationRequest(identifier: UUID().uuidString, content: content, trigger: nil))
    }
}
