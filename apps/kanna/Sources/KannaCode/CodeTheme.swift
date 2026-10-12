import AppKit

enum CodeTheme {
    static let background = color(dark: 0x14161a, light: 0xffffff)
    static let text = color(dark: 0xe6e8eb, light: 0x1d2026)
    static let number = color(dark: 0x4b5260, light: 0xb4bac4)
    static let currentNumber = color(dark: 0xc9ccd3, light: 0x1d2026)
    static let currentLine = color(dark: 0x1b1e24, light: 0xf4f5f7)
    static let flash = color(dark: 0x2a3a5c, light: 0xd6e2fa)

    static let keyword = color(dark: 0xc792ea, light: 0x8839c4)
    static let function = color(dark: 0x7aa2ff, light: 0x2f63d8)
    static let type = color(dark: 0xe5c07b, light: 0xa15c00)
    static let string = color(dark: 0xa5d6a7, light: 0x2d7d3a)
    static let constant = color(dark: 0xf78c6c, light: 0xc2410c)
    static let comment = color(dark: 0x6b7280, light: 0x8a919c)
    static let macro = color(dark: 0x56b6c2, light: 0x0e7490)
    static let attribute = color(dark: 0x8b93a0, light: 0x6b7280)
    static let property = color(dark: 0xc9d1e0, light: 0x374151)
    static let operatorColor = color(dark: 0x89ddff, light: 0x0369a1)
    static let punctuation = color(dark: 0x8b93a0, light: 0x6b7280)

    static func color(capture: String) -> NSColor? {
        switch capture {
        case "keyword", "tag", "variable.builtin": keyword
        case "function.macro": macro
        case "function", "function.builtin", "function.method": function
        case "type", "type.builtin", "constructor": type
        case "string", "string.special": string
        case "constant", "constant.builtin", "number", "escape": constant
        case "comment": comment
        case "attribute", "label": attribute
        case "property": property
        case "operator": operatorColor
        case "punctuation", "punctuation.bracket", "punctuation.delimiter", "punctuation.special": punctuation
        default: nil
        }
    }

    static let fontSize: CGFloat = 12.5
    static let gutterPadding: CGFloat = 14

    static func font(size: CGFloat) -> NSFont { NSFont.monospacedSystemFont(ofSize: size, weight: .regular) }
    static func italic(size: CGFloat) -> NSFont { NSFontManager.shared.convert(font(size: size), toHaveTrait: .italicFontMask) }

    static func isDark(_ appearance: NSAppearance) -> Bool { appearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua }

    private static func color(dark: UInt32, light: UInt32) -> NSColor {
        NSColor(name: nil) { appearance in rgb(isDark(appearance) ? dark : light) }
    }

    private static func rgb(_ value: UInt32) -> NSColor {
        NSColor(srgbRed: CGFloat((value >> 16) & 0xff) / 255, green: CGFloat((value >> 8) & 0xff) / 255,
                blue: CGFloat(value & 0xff) / 255, alpha: 1)
    }
}
