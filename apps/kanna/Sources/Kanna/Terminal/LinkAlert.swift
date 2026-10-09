import AppKit
import KannaLinks

@MainActor
enum LinkAlert {
    static func block(_ reason: UntrustedURL.DenialReason, showing target: String, in window: NSWindow?, copy: @escaping @MainActor (String) -> Void) {
        let alert = NSAlert()
        alert.alertStyle = .warning
        alert.icon = NSImage(named: NSImage.cautionName)
        alert.messageText = "Kanna Blocked This Link"
        alert.informativeText = reason.message
        alert.accessoryView = targetView(target)
        alert.addButton(withTitle: "OK")
        alert.addButton(withTitle: "Copy Link")
        present(alert, in: window) { response in
            guard response == .alertSecondButtonReturn else { return }
            copy(target)
        }
    }

    private static func present(_ alert: NSAlert, in window: NSWindow?, then done: @escaping @MainActor (NSApplication.ModalResponse) -> Void) {
        DispatchQueue.main.async {
            MainActor.assumeIsolated {
                if let window = window ?? NSApp.keyWindow {
                    alert.beginSheetModal(for: window) { response in MainActor.assumeIsolated { done(response) } }
                } else {
                    done(alert.runModal())
                }
            }
        }
    }

    private static func targetView(_ target: String) -> NSView {
        let scroll = NSScrollView(frame: NSRect(x: 0, y: 0, width: 480, height: 96))
        scroll.borderType = .bezelBorder
        scroll.hasVerticalScroller = true
        scroll.autohidesScrollers = true
        let text = NSTextView(frame: scroll.contentView.bounds)
        text.isEditable = false
        text.isSelectable = true
        text.isRichText = false
        text.font = .monospacedSystemFont(ofSize: NSFont.systemFontSize, weight: .regular)
        text.textContainerInset = NSSize(width: 6, height: 6)
        text.string = target
        text.textContainer?.widthTracksTextView = true
        text.textContainer?.containerSize = NSSize(width: scroll.contentSize.width, height: .greatestFiniteMagnitude)
        scroll.documentView = text
        return scroll
    }
}
