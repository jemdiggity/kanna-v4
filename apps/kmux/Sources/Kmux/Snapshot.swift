import AppKit
import GhosttyKit
import KmuxCore

/// `debug.snapshot`: captures a window as the window server composited it and
/// reports, per pane, the share of pixels that differ from the pane's
/// background. Checks that panes are actually drawn, not just present.
@MainActor
enum Snapshot {
    private typealias CreateImage = @convention(c) (CGRect, UInt32, UInt32, UInt32) -> Unmanaged<CGImage>?

    /// CGWindowListCreateImage is obsoleted in the macOS 15 SDK but still
    /// exported, and an app may capture its own windows without the screen
    /// recording permission that ScreenCaptureKit requires.
    private static let createImage: CreateImage? = {
        guard let symbol = dlsym(UnsafeMutableRawPointer(bitPattern: -2), "CGWindowListCreateImage") else { return nil }
        return unsafeBitCast(symbol, to: CreateImage.self)
    }()

    static func run(_ controller: WindowController, panes: [PaneView], path: String?, marker: String?) throws -> [String: JSON] {
        let window = controller.window
        // kCGWindowListOptionIncludingWindow; boundsIgnoreFraming | bestResolution.
        guard let image = createImage?(.null, 1 << 3, UInt32(window.windowNumber), 1 << 0 | 1 << 3)?.takeRetainedValue(),
              let data = image.dataProvider?.data, let bytes = CFDataGetBytePtr(data) else {
            throw KmuxError("bad_request", "window capture unavailable")
        }
        var out: [String: JSON] = ["window": .string(controller.id), "width": .number(Double(image.width)), "height": .number(Double(image.height))]
        if let path {
            let url = URL(fileURLWithPath: (path as NSString).expandingTildeInPath)
            try? FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
            try NSBitmapImageRep(cgImage: image).representation(using: .png, properties: [:])?.write(to: url)
            out["path"] = .string(url.path)
        }
        let scale = CGFloat(image.width) / window.frame.width
        let reader = Pixels(bytes: bytes, image: image)
        out["panes"] = .array(panes.map { view in
            let inWindow = view.convert(view.bounds, to: nil)
            let rect = CGRect(x: inWindow.minX * scale, y: (window.frame.height - inWindow.maxY) * scale, width: inWindow.width * scale, height: inWindow.height * scale)
            let background = reader.dominant(in: rect)
            var total = 0, inked = 0
            reader.forEach(in: rect) {
                total += 1
                if $0.distance(to: background) > 60 { inked += 1 }
            }
            var result: [String: JSON] = ["id": .string(view.id), "background": .string(background.description), "ink": .number(total == 0 ? 0 : (Double(inked) / Double(total) * 10000).rounded() / 10000)]
            if let marker, let terminal = view.content as? TerminalSurfaceView, let surface = terminal.surface {
                result["marker"] = markerCheck(marker, terminal, surface, rect: rect, scale: scale, reader: reader, background: background)
            }
            return .object(result)
        })
        return out
    }
}

/// Finds `marker` in the terminal's text and counts the marker's cells that
/// hold glyph pixels. A cell needs a few pixels clearly unlike the
/// background, so stale or uninitialised frames fail.
@MainActor
private func markerCheck(_ marker: String, _ terminal: TerminalSurfaceView, _ surface: ghostty_surface_t, rect: CGRect, scale: CGFloat,
                         reader: Pixels, background: RGB) -> JSON {
    let size = ghostty_surface_size(surface)
    // "*" checks the first word of the first line on screen, for panes whose
    // output isn't known in advance (a new shell).
    let text = terminal.viewportText()
    let marker = marker != "*" ? marker : text.split(separator: "\n").lazy.compactMap { $0.split(separator: " ").first.map(String.init) }.first ?? ""
    guard !marker.isEmpty, let (row, column) = locate(marker, in: text, columns: Int(size.columns)) else { return ["found": false] }
    let backing = terminal.window?.backingScaleFactor ?? 2
    let cell = CGSize(width: CGFloat(size.cell_width_px) * scale / backing, height: CGFloat(size.cell_height_px) * scale / backing)
    // Ghostty's default window padding is 2 points.
    let pad = 2 * scale
    var glyphs = 0, inked = 0
    for (offset, character) in marker.enumerated() where !character.isWhitespace {
        glyphs += 1
        let cellRect = CGRect(x: rect.minX + pad + CGFloat(column + offset) * cell.width, y: rect.minY + pad + CGFloat(row) * cell.height,
                              width: cell.width, height: cell.height).insetBy(dx: 1, dy: 1)
        var count = 0
        reader.forEach(in: cellRect) { if $0.distance(to: background) > 60 { count += 1 } }
        if count >= 4 { inked += 1 }
    }
    return ["found": true, "text": .string(marker), "row": .number(Double(row)), "column": .number(Double(column)), "glyphs": .number(Double(glyphs)), "inked": .number(Double(inked))]
}

/// Screen row and column of `marker`; read text joins soft-wrapped rows.
private func locate(_ marker: String, in text: String, columns: Int) -> (Int, Int)? {
    let columns = max(1, columns)
    var row = 0
    for line in text.components(separatedBy: "\n") {
        if let range = line.range(of: marker) {
            let offset = line.distance(from: line.startIndex, to: range.lowerBound)
            if offset % columns + marker.count <= columns { return (row + offset / columns, offset % columns) }
        }
        row += max(1, (line.count + columns - 1) / columns)
    }
    return nil
}

private struct RGB: Hashable, CustomStringConvertible {
    let r: Int, g: Int, b: Int
    func distance(to other: RGB) -> Int { abs(r - other.r) + abs(g - other.g) + abs(b - other.b) }
    var description: String { String(format: "#%02x%02x%02x", r, g, b) }
}

private struct Pixels {
    let bytes: UnsafePointer<UInt8>
    let image: CGImage
    let bgra: Bool

    init(bytes: UnsafePointer<UInt8>, image: CGImage) {
        self.bytes = bytes
        self.image = image
        bgra = image.bitmapInfo.intersection(.byteOrderMask) == .byteOrder32Little
    }

    func pixel(_ x: Int, _ y: Int) -> RGB {
        let p = bytes + y * image.bytesPerRow + x * (image.bitsPerPixel / 8)
        return bgra ? RGB(r: Int(p[2]), g: Int(p[1]), b: Int(p[0])) : RGB(r: Int(p[0]), g: Int(p[1]), b: Int(p[2]))
    }

    func forEach(in rect: CGRect, _ body: (RGB) -> Void) {
        let r = rect.integral.intersection(CGRect(x: 0, y: 0, width: image.width, height: image.height))
        guard !r.isNull, r.width > 0, r.height > 0 else { return }
        for y in Int(r.minY)..<Int(r.maxY) { for x in Int(r.minX)..<Int(r.maxX) { body(pixel(x, y)) } }
    }

    func dominant(in rect: CGRect) -> RGB {
        var counts: [RGB: Int] = [:]
        let r = rect.integral.intersection(CGRect(x: 0, y: 0, width: image.width, height: image.height))
        guard !r.isNull else { return RGB(r: 0, g: 0, b: 0) }
        let step = max(1, Int(min(r.width, r.height) / 60))
        for y in stride(from: Int(r.minY), to: Int(r.maxY), by: step) {
            for x in stride(from: Int(r.minX), to: Int(r.maxX), by: step) { counts[pixel(x, y), default: 0] += 1 }
        }
        return counts.max { $0.value < $1.value }?.key ?? RGB(r: 0, g: 0, b: 0)
    }
}
