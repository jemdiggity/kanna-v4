import AppKit
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

    static func run(_ controller: WindowController, panes: [PaneView], path: String?) throws -> [String: JSON] {
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
            return ["id": .string(view.id), "background": .string(background.description), "ink": .number(total == 0 ? 0 : (Double(inked) / Double(total) * 10000).rounded() / 10000)]
        })
        return out
    }
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
