import AppKit

enum CodeTheme {
    static let background = color(dark: 0x14161a, light: 0xffffff)
    static let text = color(dark: 0xe6e8eb, light: 0x1d2026)
    static let number = color(dark: 0x4b5260, light: 0xb4bac4)
    static let currentNumber = color(dark: 0xc9ccd3, light: 0x1d2026)
    static let currentLine = color(dark: 0x1b1e24, light: 0xf4f5f7)
    static let flash = color(dark: 0x2a3a5c, light: 0xd6e2fa)

    static let fontSize: CGFloat = 12.5
    static let gutterPadding: CGFloat = 14

    static func font(size: CGFloat) -> NSFont { NSFont.monospacedSystemFont(ofSize: size, weight: .regular) }

    static func isDark(_ appearance: NSAppearance) -> Bool { appearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua }

    private static func color(dark: UInt32, light: UInt32) -> NSColor {
        NSColor(name: nil) { appearance in rgb(isDark(appearance) ? dark : light) }
    }

    private static func rgb(_ value: UInt32) -> NSColor {
        NSColor(srgbRed: CGFloat((value >> 16) & 0xff) / 255, green: CGFloat((value >> 8) & 0xff) / 255,
                blue: CGFloat(value & 0xff) / 255, alpha: 1)
    }
}
