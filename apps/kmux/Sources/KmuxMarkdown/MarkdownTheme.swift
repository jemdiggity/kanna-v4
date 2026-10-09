import AppKit

/// Colours and fonts for markdown panes: Kanna's palette (as in kanna-v3's
/// doc view), each colour following the system appearance.
enum MarkdownTheme {
    static let background = color(dark: 0x14161a, light: 0xffffff)
    static let panel = color(dark: 0x1b1e24, light: 0xf4f5f7)
    static let panel2 = color(dark: 0x22262d, light: 0xeaecf0)
    static let line = color(dark: 0x2c313a, light: 0xd8dce2)
    static let text = color(dark: 0xe6e8eb, light: 0x1d2026)
    static let muted = color(dark: 0x8b93a0, light: 0x6b7280)
    static let accent = color(dark: 0x7aa2ff, light: 0x2f63d8)
    static let bad = color(dark: 0xef6b6b, light: 0xcf3f3f)

    static let bodySize: CGFloat = 14
    static let monoSize: CGFloat = 12.5
    static let headingSizes: [CGFloat] = [26, 21, 17.5, 15.5, 14.5, 14]
    /// The widest the text gets (at zoom 1); wider panes centre it.
    static let maxTextWidth: CGFloat = 860

    static func isDark(_ appearance: NSAppearance) -> Bool { appearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua }

    private static func color(dark: UInt32, light: UInt32) -> NSColor {
        NSColor(name: nil) { appearance in rgb(isDark(appearance) ? dark : light) }
    }

    private static func rgb(_ value: UInt32) -> NSColor {
        NSColor(srgbRed: CGFloat((value >> 16) & 0xff) / 255, green: CGFloat((value >> 8) & 0xff) / 255,
                blue: CGFloat(value & 0xff) / 255, alpha: 1)
    }
}
