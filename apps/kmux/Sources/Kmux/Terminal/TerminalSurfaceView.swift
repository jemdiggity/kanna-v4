// Adapted from Ghostty macOS SurfaceView_AppKit.swift, Ghostty.Input.swift, and NSEvent+Extension.swift (MIT; Ghostty 4ec3122), via kanna v3.
import AppKit
import GhosttyKit
import QuartzCore

/// Draws one Ghostty surface and feeds it keyboard, mouse and IME input.
@MainActor
final class TerminalSurfaceView: NSView, @preconcurrency NSTextInputClient {
    var onFocus: (() -> Void)?
    var surface: ghostty_surface_t? { didSet { if surface != nil { syncSurfaceGeometry(); updateVisibility() } } }
    var isHandlingInputEvent = false
    /// Tests render hidden windows too (KMUX_IGNORE_OCCLUSION=1), so their
    /// pixel checks don't depend on what else is on screen.
    static let ignoresOcclusion = ProcessInfo.processInfo.environment["KMUX_IGNORE_OCCLUSION"] == "1"
    var address: UInt { UInt(bitPattern: Unmanaged.passUnretained(self).toOpaque()) }
    private var occlusionObserver: NSObjectProtocol?
    private var marked = NSMutableAttributedString()
    private var accumulating: [String]?
    private var suppressedKeyUps = Set<UInt16>()

    override var acceptsFirstResponder: Bool { true }

    // No backing layer of our own: Ghostty's renderer makes this view
    // layer-hosting with its IOSurface layer, as in Ghostty's app.
    override init(frame frameRect: NSRect) { super.init(frame: frameRect) }
    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    // Focus follows the first responder rather than key-window status, so a
    // pane keeps its focused cursor across reparenting.
    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        if let occlusionObserver { NotificationCenter.default.removeObserver(occlusionObserver) }
        occlusionObserver = window.map { window in
            NotificationCenter.default.addObserver(forName: NSWindow.didChangeOcclusionStateNotification, object: window, queue: .main) { [weak self] _ in
                MainActor.assumeIsolated { self?.updateVisibility() }
            }
        }
        guard let surface else { return }
        ghostty_surface_set_focus(surface, window?.firstResponder === self)
        syncSurfaceGeometry()
        updateVisibility()
    }

    override func viewDidChangeBackingProperties() {
        super.viewDidChangeBackingProperties()
        if let window {
            CATransaction.begin()
            CATransaction.setDisableActions(true)
            layer?.contentsScale = window.backingScaleFactor
            CATransaction.commit()
        }
        syncSurfaceGeometry()
    }

    /// ghostty_surface_set_occlusion takes *visibility*: false stops the
    /// renderer from updating at all.
    private func updateVisibility() {
        guard let surface else { return }
        let visible = window.map { Self.ignoresOcclusion || $0.occlusionState.contains(.visible) } ?? false
        ghostty_surface_set_occlusion(surface, visible)
    }

    /// Content scale and pixel size, also for views created at a zero frame
    /// and laid out or reparented later.
    private func syncSurfaceGeometry() {
        guard let surface, bounds.width > 0, bounds.height > 0 else { return }
        let backing = convertToBacking(bounds)
        ghostty_surface_set_content_scale(surface, backing.width / bounds.width, backing.height / bounds.height)
        ghostty_surface_set_size(surface, UInt32(max(1, backing.width)), UInt32(max(1, backing.height)))
    }

    override func setFrameSize(_ newSize: NSSize) {
        super.setFrameSize(newSize)
        syncSurfaceGeometry()
    }

    override func becomeFirstResponder() -> Bool {
        let result = super.becomeFirstResponder()
        if let surface { ghostty_surface_set_focus(surface, true) }
        onFocus?()
        return result
    }

    override func resignFirstResponder() -> Bool {
        let result = super.resignFirstResponder()
        if let surface { ghostty_surface_set_focus(surface, false) }
        return result
    }

    // MARK: Keyboard

    override func keyDown(with event: NSEvent) {
        guard let surface else {
            interpretKeyEvents([event])
            return
        }
        let mods = Self.mods(event.modifierFlags)
        let translated = ghostty_surface_key_translation_mods(surface, mods)
        let flags = Self.adjust(event.modifierFlags, basedOn: translated)
        let translatedEvent = flags == event.modifierFlags ? event : (NSEvent.keyEvent(with: .keyDown, location: event.locationInWindow, modifierFlags: flags, timestamp: event.timestamp, windowNumber: event.windowNumber, context: nil, characters: event.characters(byApplyingModifiers: flags) ?? "", charactersIgnoringModifiers: event.charactersIgnoringModifiers ?? "", isARepeat: event.isARepeat, keyCode: event.keyCode) ?? event)
        accumulating = []
        isHandlingInputEvent = true
        defer {
            accumulating = nil
            isHandlingInputEvent = false
        }
        interpretKeyEvents([translatedEvent])
        syncPreedit()
        if marked.length > 0 {
            suppressedKeyUps.insert(event.keyCode)
            return
        }
        // Committed text goes out as the key event's text, as Ghostty's app
        // does. ghostty_surface_text is the paste path and would wrap every
        // keystroke in bracketed-paste markers.
        if let texts = accumulating, !texts.isEmpty {
            for text in texts { sendKeyPress(event, mods: mods, text: text) }
            return
        }
        sendKeyPress(event, mods: mods, text: Self.keyEventText(translatedEvent))
    }

    override func keyUp(with event: NSEvent) {
        if suppressedKeyUps.remove(event.keyCode) != nil { return }
        guard let surface else { return }
        var input = ghostty_input_key_s()
        input.action = GHOSTTY_ACTION_RELEASE
        input.keycode = UInt32(event.keyCode)
        input.mods = Self.mods(event.modifierFlags)
        input.unshifted_codepoint = event.charactersIgnoringModifiers?.unicodeScalars.first?.value ?? 0
        _ = ghostty_surface_key(surface, input)
    }

    private func sendKeyPress(_ event: NSEvent, mods: ghostty_input_mods_e, text: String?) {
        guard let surface else { return }
        var input = ghostty_input_key_s()
        input.action = event.isARepeat ? GHOSTTY_ACTION_REPEAT : GHOSTTY_ACTION_PRESS
        input.keycode = UInt32(event.keyCode)
        input.mods = mods
        input.consumed_mods = ghostty_input_mods_e(rawValue: mods.rawValue & ~(GHOSTTY_MODS_CTRL.rawValue | GHOSTTY_MODS_SUPER.rawValue))
        input.unshifted_codepoint = event.charactersIgnoringModifiers?.unicodeScalars.first?.value ?? 0
        _ = Self.filteredText(text)?.withCString { input.text = $0; return ghostty_surface_key(surface, input) } ?? ghostty_surface_key(surface, input)
    }

    /// Mirrors Ghostty's NSEvent.ghosttyCharacters: a lone control character
    /// is reported without Control, and function-key private-use characters
    /// are not text.
    private static func keyEventText(_ event: NSEvent) -> String? {
        guard let characters = event.characters else { return nil }
        if characters.count == 1, let scalar = characters.unicodeScalars.first {
            if scalar.value < 0x20 { return event.characters(byApplyingModifiers: event.modifierFlags.subtracting(.control)) }
            if (0xF700...0xF8FF).contains(scalar.value) { return nil }
        }
        return characters
    }

    /// Mirrors Ghostty's String.keyEventText: text starting with an ASCII
    /// control character or DEL is dropped so the encoder works from the key
    /// code (Shift+Return reaches kitty-keyboard programs as CSI 13;2u).
    private static func filteredText(_ text: String?) -> String? {
        guard let text, let scalar = text.unicodeScalars.first else { return nil }
        return scalar.value < 0x20 || scalar.value == 0x7F ? nil : text
    }

    /// The text on screen, one line per row (soft-wrapped rows joined).
    func viewportText() -> String {
        guard let surface else { return "" }
        let selection = ghostty_selection_s(
            top_left: ghostty_point_s(tag: GHOSTTY_POINT_VIEWPORT, coord: GHOSTTY_POINT_COORD_TOP_LEFT, x: 0, y: 0),
            bottom_right: ghostty_point_s(tag: GHOSTTY_POINT_VIEWPORT, coord: GHOSTTY_POINT_COORD_BOTTOM_RIGHT, x: 0, y: 0), rectangle: false)
        var result = ghostty_text_s()
        guard ghostty_surface_read_text(surface, selection, &result) else { return "" }
        defer { ghostty_surface_free_text(surface, &result) }
        return result.text.map { String(cString: $0) } ?? ""
    }

    /// Types `text` as a paste, for the `send` command.
    func sendText(_ text: String) {
        guard let surface else { return }
        text.withCString { ghostty_surface_text(surface, $0, UInt(text.utf8.count)) }
    }

    // MARK: Mouse

    override func mouseDown(with event: NSEvent) {
        window?.makeFirstResponder(self)
        mouseButton(event, GHOSTTY_MOUSE_PRESS, GHOSTTY_MOUSE_LEFT)
    }
    override func mouseUp(with event: NSEvent) {
        mouseButton(event, GHOSTTY_MOUSE_RELEASE, GHOSTTY_MOUSE_LEFT)
        if let surface { ghostty_surface_mouse_pressure(surface, 0, 0) }
    }
    override func rightMouseDown(with event: NSEvent) {
        window?.makeFirstResponder(self)
        mouseButton(event, GHOSTTY_MOUSE_PRESS, GHOSTTY_MOUSE_RIGHT)
    }
    override func rightMouseUp(with event: NSEvent) { mouseButton(event, GHOSTTY_MOUSE_RELEASE, GHOSTTY_MOUSE_RIGHT) }
    override func otherMouseDown(with event: NSEvent) {
        window?.makeFirstResponder(self)
        mouseButton(event, GHOSTTY_MOUSE_PRESS, Self.mouseButton(for: event.buttonNumber))
    }
    override func otherMouseUp(with event: NSEvent) { mouseButton(event, GHOSTTY_MOUSE_RELEASE, Self.mouseButton(for: event.buttonNumber)) }
    override func mouseMoved(with event: NSEvent) { mousePosition(event) }
    override func mouseDragged(with event: NSEvent) { mousePosition(event) }
    override func rightMouseDragged(with event: NSEvent) { mousePosition(event) }
    override func otherMouseDragged(with event: NSEvent) { mousePosition(event) }

    override func scrollWheel(with event: NSEvent) {
        guard let surface else { return }
        let scale = event.hasPreciseScrollingDeltas ? 2.0 : 1.0
        let mods = (event.hasPreciseScrollingDeltas ? 1 : 0) | (Self.momentum(event.momentumPhase) << 1)
        ghostty_surface_mouse_scroll(surface, event.scrollingDeltaX * scale, event.scrollingDeltaY * scale, ghostty_input_scroll_mods_t(mods))
    }

    override func pressureChange(with event: NSEvent) {
        if let surface { ghostty_surface_mouse_pressure(surface, UInt32(event.stage), Double(event.pressure)) }
    }

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        addTrackingArea(NSTrackingArea(rect: bounds, options: [.mouseMoved, .activeInKeyWindow, .inVisibleRect], owner: self, userInfo: nil))
    }

    private func mouseButton(_ event: NSEvent, _ action: ghostty_input_mouse_state_e, _ button: ghostty_input_mouse_button_e) {
        guard let surface else { return }
        mousePosition(event)
        _ = ghostty_surface_mouse_button(surface, action, button, Self.mods(event.modifierFlags))
    }

    private func mousePosition(_ event: NSEvent) {
        guard let surface else { return }
        let point = convert(event.locationInWindow, from: nil)
        ghostty_surface_mouse_pos(surface, point.x, bounds.height - point.y, Self.mods(event.modifierFlags))
    }

    private static func mouseButton(for number: Int) -> ghostty_input_mouse_button_e {
        switch number {
        case 2: GHOSTTY_MOUSE_MIDDLE
        case 3: GHOSTTY_MOUSE_FOUR
        case 4: GHOSTTY_MOUSE_FIVE
        case 5: GHOSTTY_MOUSE_SIX
        case 6: GHOSTTY_MOUSE_SEVEN
        case 7: GHOSTTY_MOUSE_EIGHT
        case 8: GHOSTTY_MOUSE_NINE
        case 9: GHOSTTY_MOUSE_TEN
        case 10: GHOSTTY_MOUSE_ELEVEN
        default: GHOSTTY_MOUSE_UNKNOWN
        }
    }

    static func mods(_ flags: NSEvent.ModifierFlags) -> ghostty_input_mods_e {
        var value = GHOSTTY_MODS_NONE.rawValue
        if flags.contains(.shift) { value |= GHOSTTY_MODS_SHIFT.rawValue }
        if flags.contains(.control) { value |= GHOSTTY_MODS_CTRL.rawValue }
        if flags.contains(.option) { value |= GHOSTTY_MODS_ALT.rawValue }
        if flags.contains(.command) { value |= GHOSTTY_MODS_SUPER.rawValue }
        return ghostty_input_mods_e(rawValue: value)
    }

    private static func adjust(_ original: NSEvent.ModifierFlags, basedOn mods: ghostty_input_mods_e) -> NSEvent.ModifierFlags {
        var result = original
        for (flag, bit) in [(NSEvent.ModifierFlags.shift, GHOSTTY_MODS_SHIFT), (.control, GHOSTTY_MODS_CTRL), (.option, GHOSTTY_MODS_ALT), (.command, GHOSTTY_MODS_SUPER)] {
            if mods.rawValue & bit.rawValue != 0 { result.insert(flag) } else { result.remove(flag) }
        }
        return result
    }

    private static func momentum(_ phase: NSEvent.Phase) -> Int32 {
        if phase.contains(.mayBegin) { return 6 }
        if phase.contains(.began) { return 1 }
        if phase.contains(.stationary) { return 2 }
        if phase.contains(.changed) { return 3 }
        if phase.contains(.ended) { return 4 }
        if phase.contains(.cancelled) { return 5 }
        return 0
    }

    // MARK: NSTextInputClient

    func characterIndex(for point: NSPoint) -> Int { NSNotFound }
    func hasMarkedText() -> Bool { marked.length > 0 }
    func markedRange() -> NSRange { marked.length > 0 ? NSRange(location: 0, length: marked.length) : NSRange(location: NSNotFound, length: 0) }
    func validAttributesForMarkedText() -> [NSAttributedString.Key] { [] }

    func selectedRange() -> NSRange {
        guard let surface else { return NSRange(location: NSNotFound, length: 0) }
        var text = ghostty_text_s()
        guard ghostty_surface_read_selection(surface, &text) else { return NSRange(location: NSNotFound, length: 0) }
        defer { ghostty_surface_free_text(surface, &text) }
        return NSRange(location: Int(text.offset_start), length: Int(text.offset_len))
    }

    func setMarkedText(_ string: Any, selectedRange: NSRange, replacementRange: NSRange) {
        if let value = string as? NSAttributedString { marked = NSMutableAttributedString(attributedString: value) }
        else if let value = string as? String { marked = NSMutableAttributedString(string: value) }
        syncPreedit()
    }

    func unmarkText() {
        marked = NSMutableAttributedString()
        syncPreedit()
    }

    func attributedSubstring(forProposedRange range: NSRange, actualRange: NSRangePointer?) -> NSAttributedString? {
        guard let surface else { return nil }
        var text = ghostty_text_s()
        guard ghostty_surface_read_selection(surface, &text) else { return nil }
        defer { ghostty_surface_free_text(surface, &text) }
        guard let value = text.text else { return nil }
        actualRange?.pointee = NSRange(location: Int(text.offset_start), length: Int(text.offset_len))
        return NSAttributedString(string: String(cString: value))
    }

    func firstRect(forCharacterRange range: NSRange, actualRange: NSRangePointer?) -> NSRect {
        guard let surface else { return .zero }
        var x = 0.0, y = 0.0, width = 0.0, height = 0.0
        ghostty_surface_ime_point(surface, &x, &y, &width, &height)
        let rect = convert(NSRect(x: x, y: bounds.height - y, width: width, height: height), to: nil)
        return window?.convertToScreen(rect) ?? rect
    }

    func insertText(_ string: Any, replacementRange: NSRange) {
        let text = (string as? NSAttributedString)?.string ?? (string as? String) ?? ""
        unmarkText()
        if var accumulating {
            accumulating.append(text)
            self.accumulating = accumulating
        } else {
            sendText(text)
        }
    }

    override func doCommand(by selector: Selector) {}

    private func syncPreedit() {
        guard let surface else { return }
        if marked.length == 0 { ghostty_surface_preedit(surface, nil, 0) }
        else { marked.string.withCString { ghostty_surface_preedit(surface, $0, UInt(marked.string.utf8.count)) } }
    }
}
