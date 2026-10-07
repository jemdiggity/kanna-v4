import AppKit
import GhosttyKit

/// A menu key equivalent.
struct Shortcut: Equatable {
    let key: String
    let modifiers: NSEvent.ModifierFlags

    static func cmd(_ key: String) -> Shortcut { Shortcut(key: key, modifiers: .command) }
    static func shiftCmd(_ key: String) -> Shortcut { Shortcut(key: key, modifiers: [.command, .shift]) }

    /// The shortcut the user's Ghostty config binds to `action` (e.g.
    /// "new_split:right"), if it can be shown as a menu key equivalent.
    /// Physical keys are read as on a US layout.
    init?(_ trigger: ghostty_input_trigger_s) {
        let key: String?
        switch trigger.tag {
        case GHOSTTY_TRIGGER_UNICODE: key = UnicodeScalar(trigger.key.unicode).map { String(Character($0)).lowercased() }
        case GHOSTTY_TRIGGER_PHYSICAL: key = Self.physical[trigger.key.physical]
        default: key = nil
        }
        guard let key, !key.isEmpty else { return nil }
        var modifiers: NSEvent.ModifierFlags = []
        let mods = trigger.mods.rawValue
        if mods & GHOSTTY_MODS_SHIFT.rawValue != 0 { modifiers.insert(.shift) }
        if mods & GHOSTTY_MODS_CTRL.rawValue != 0 { modifiers.insert(.control) }
        if mods & GHOSTTY_MODS_ALT.rawValue != 0 { modifiers.insert(.option) }
        if mods & GHOSTTY_MODS_SUPER.rawValue != 0 { modifiers.insert(.command) }
        self.init(key: key, modifiers: modifiers)
    }

    init(key: String, modifiers: NSEvent.ModifierFlags) {
        self.key = key
        self.modifiers = modifiers
    }

    private static let physical: [ghostty_input_key_e: String] = {
        var table: [ghostty_input_key_e: String] = [
            GHOSTTY_KEY_BACKQUOTE: "`", GHOSTTY_KEY_BACKSLASH: "\\", GHOSTTY_KEY_BRACKET_LEFT: "[", GHOSTTY_KEY_BRACKET_RIGHT: "]",
            GHOSTTY_KEY_COMMA: ",", GHOSTTY_KEY_EQUAL: "=", GHOSTTY_KEY_MINUS: "-", GHOSTTY_KEY_PERIOD: ".", GHOSTTY_KEY_QUOTE: "'",
            GHOSTTY_KEY_SEMICOLON: ";", GHOSTTY_KEY_SLASH: "/", GHOSTTY_KEY_ENTER: "\r",
            GHOSTTY_KEY_ARROW_UP: String(UnicodeScalar(NSUpArrowFunctionKey)!), GHOSTTY_KEY_ARROW_DOWN: String(UnicodeScalar(NSDownArrowFunctionKey)!),
            GHOSTTY_KEY_ARROW_LEFT: String(UnicodeScalar(NSLeftArrowFunctionKey)!), GHOSTTY_KEY_ARROW_RIGHT: String(UnicodeScalar(NSRightArrowFunctionKey)!),
        ]
        for (offset, letter) in "abcdefghijklmnopqrstuvwxyz".enumerated() {
            table[ghostty_input_key_e(rawValue: GHOSTTY_KEY_A.rawValue + UInt32(offset))] = String(letter)
        }
        for digit in 0...9 { table[ghostty_input_key_e(rawValue: GHOSTTY_KEY_DIGIT_0.rawValue + UInt32(digit))] = String(digit) }
        return table
    }()
}

/// Key presses for `debug.key`, built from a virtual key code the way the
/// system builds hardware events, so AppKit derives the characters from the
/// current keyboard layout (hand-built NSEvents match menus differently).
enum SyntheticKey {
    private static let codes: [String: CGKeyCode] = [
        "a": 0, "s": 1, "d": 2, "f": 3, "h": 4, "g": 5, "z": 6, "x": 7, "c": 8, "v": 9, "b": 11, "q": 12, "w": 13, "e": 14,
        "r": 15, "y": 16, "t": 17, "o": 31, "u": 32, "i": 34, "p": 35, "l": 37, "j": 38, "k": 40, "n": 45, "m": 46,
        "]": 30, "[": 33, "`": 50, "return": 36, "escape": 53, "space": 49,
    ]

    /// Parses "cmd+shift+[" style names into a key-down event for `window`.
    static func event(_ name: String, window: NSWindow?) -> NSEvent? {
        var parts = name.lowercased().split(separator: "+").map(String.init)
        guard let key = parts.popLast(), let code = codes[key == "enter" ? "return" : key] else { return nil }
        var flags: CGEventFlags = []
        for part in parts {
            switch part {
            case "cmd", "super": flags.insert(.maskCommand)
            case "shift": flags.insert(.maskShift)
            case "opt", "alt", "option": flags.insert(.maskAlternate)
            case "ctrl", "control": flags.insert(.maskControl)
            default: return nil
            }
        }
        guard let cgEvent = CGEvent(keyboardEventSource: CGEventSource(stateID: .hidSystemState), virtualKey: code, keyDown: true) else { return nil }
        cgEvent.flags = flags
        guard let event = NSEvent(cgEvent: cgEvent) else { return nil }
        // Address the event to the key window, as the window server would.
        return NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: event.modifierFlags, timestamp: event.timestamp,
                                windowNumber: window?.windowNumber ?? 0, context: nil, characters: event.characters ?? "",
                                charactersIgnoringModifiers: event.charactersIgnoringModifiers ?? "", isARepeat: false, keyCode: event.keyCode)
    }
}
