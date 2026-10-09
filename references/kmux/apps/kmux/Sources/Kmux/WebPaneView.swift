import AppKit
import WebKit

/// A web pane: a WKWebView, a notice while a server isn't answering yet
/// (kmux keeps retrying, so `kmux open web localhost:3000` can come before
/// `npm run dev`), and a URL editor for ⌘L.
@MainActor
final class WebPaneView: NSView, WKNavigationDelegate, NSTextFieldDelegate {
    let webView: FocusReportingWebView
    var onFocus: (() -> Void)? { didSet { webView.onFocus = onFocus } }
    /// The user asked for a new URL in the editor.
    var onNavigate: ((String) -> Void)?
    /// The page moved to a new URL (a link, a redirect).
    var onURLChange: ((String) -> Void)?
    private(set) var url: URL?
    private let notice = NSTextField(wrappingLabelWithString: "")
    private var editor: NSTextField?
    private var retry: Timer?

    init(url: String) {
        let configuration = WKWebViewConfiguration()
        configuration.websiteDataStore = .default()
        webView = FocusReportingWebView(frame: .zero, configuration: configuration)
        super.init(frame: .zero)
        webView.navigationDelegate = self
        addSubview(webView)
        notice.alignment = .center
        notice.font = .systemFont(ofSize: 13)
        notice.textColor = .secondaryLabelColor
        notice.isHidden = true
        addSubview(notice)
        load(url)
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    func load(_ string: String) {
        retry?.invalidate()
        notice.isHidden = true
        guard let url = URL(string: string) else {
            show("Not a URL: \(string)")
            return
        }
        self.url = url
        webView.load(URLRequest(url: url))
    }

    func stop() {
        retry?.invalidate()
        webView.stopLoading()
    }

    override func layout() {
        super.layout()
        webView.frame = bounds
        let width = min(bounds.width - 40, 420)
        let size = notice.sizeThatFits(NSSize(width: width, height: .greatestFiniteMagnitude))
        notice.frame = NSRect(x: (bounds.width - width) / 2, y: (bounds.height - size.height) / 2, width: width, height: size.height)
        editor?.frame = NSRect(x: 8, y: bounds.height - 34, width: bounds.width - 16, height: 26)
    }

    private func show(_ message: String) {
        notice.stringValue = message
        notice.isHidden = false
        needsLayout = true
    }

    // MARK: Navigation

    func webView(_ webView: WKWebView, didCommit navigation: WKNavigation!) {
        notice.isHidden = true
        if let current = webView.url, current != url {
            url = current
            onURLChange?(current.absoluteString)
        }
    }

    func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: Error) {
        failed(error)
    }

    func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: Error) {
        failed(error)
    }

    private func failed(_ error: Error) {
        let error = error as NSError
        if error.domain == NSURLErrorDomain, error.code == NSURLErrorCancelled { return }
        guard let url else { return }
        let refused = error.domain == NSURLErrorDomain && [NSURLErrorCannotConnectToHost, NSURLErrorNetworkConnectionLost, NSURLErrorTimedOut].contains(error.code)
        if refused, ["localhost", "127.0.0.1", "::1"].contains(url.host ?? "") {
            show("Waiting for \(url.host ?? "")\(url.port.map { ":\($0)" } ?? "")…\nkmux keeps retrying until the server responds.")
            retry = Timer.scheduledTimer(withTimeInterval: 1, repeats: false) { [weak self] _ in
                MainActor.assumeIsolated { if let self, let url = self.url { self.webView.load(URLRequest(url: url)) } }
            }
        } else {
            show("Couldn't load \(url.absoluteString)\n\(error.localizedDescription)")
        }
    }

    // MARK: URL editor (⌘L)

    func editURL() {
        guard editor == nil else {
            window?.makeFirstResponder(editor)
            return
        }
        let field = NSTextField(string: url?.absoluteString ?? "")
        field.font = .systemFont(ofSize: 13)
        field.bezelStyle = .roundedBezel
        field.delegate = self
        addSubview(field)
        editor = field
        needsLayout = true
        layoutSubtreeIfNeeded()
        window?.makeFirstResponder(field)
        field.currentEditor()?.selectAll(nil)
    }

    func control(_ control: NSControl, textView: NSTextView, doCommandBy selector: Selector) -> Bool {
        switch selector {
        case #selector(NSResponder.insertNewline(_:)):
            let text = control.stringValue.trimmingCharacters(in: .whitespaces)
            closeEditor()
            if !text.isEmpty { onNavigate?(text) }
            return true
        case #selector(NSResponder.cancelOperation(_:)):
            closeEditor()
            return true
        default:
            return false
        }
    }

    func controlTextDidEndEditing(_ notification: Notification) { closeEditor() }

    private func closeEditor() {
        guard let field = editor else { return }
        editor = nil
        field.removeFromSuperview()
        window?.makeFirstResponder(webView)
    }
}

/// Reports when the page takes the keyboard, so kmux can focus its pane.
final class FocusReportingWebView: WKWebView {
    var onFocus: (() -> Void)?
    /// kmux's pane menu, added below WebKit's own items on a right-click.
    var contextMenu: (() -> NSMenu?)?

    override func willOpenMenu(_ menu: NSMenu, with event: NSEvent) {
        super.willOpenMenu(menu, with: event)
        window?.makeFirstResponder(self)
        guard let extra = contextMenu?() else { return }
        menu.addItem(.separator())
        for item in extra.items {
            extra.removeItem(item)
            menu.addItem(item)
        }
    }

    override func becomeFirstResponder() -> Bool {
        let result = super.becomeFirstResponder()
        if result { onFocus?() }
        return result
    }
}
