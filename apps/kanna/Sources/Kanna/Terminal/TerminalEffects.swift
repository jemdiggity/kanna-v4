import AppKit
import GhosttyKit
import UserNotifications

@MainActor
final class TerminalEffects {
    let recording: Bool
    let general: NSPasteboard
    let selection: NSPasteboard
    private(set) var opened: [String] = []
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

    func open(_ url: String) {
        if recording { return opened.append(url) }
        let target = URL(string: url).flatMap { $0.scheme == nil ? nil : $0 } ?? URL(fileURLWithPath: (url as NSString).expandingTildeInPath)
        NSWorkspace.shared.open(target)
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
