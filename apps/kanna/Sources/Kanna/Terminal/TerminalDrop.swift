import AppKit
import KannaDrop

extension TerminalSurfaceView {
    static let dropTypes: [NSPasteboard.PasteboardType] =
        [.fileURL, .png, .tiff, .string] + NSFilePromiseReceiver.readableDraggedTypes.map { NSPasteboard.PasteboardType($0) }

    override func draggingEntered(_ sender: any NSDraggingInfo) -> NSDragOperation {
        guard let types = sender.draggingPasteboard.types, !Set(types).isDisjoint(with: Self.dropTypes) else { return [] }
        return .copy
    }

    override func performDragOperation(_ sender: any NSDraggingInfo) -> Bool {
        drop(sender.draggingPasteboard)
    }

    @discardableResult
    func drop(_ pasteboard: NSPasteboard) -> Bool {
        let files = pasteboard.readObjects(forClasses: [NSURL.self], options: [.urlReadingFileURLsOnly: true]) as? [URL] ?? []
        if !files.isEmpty {
            insertDropped(DropText.paths(files.map(\.path)))
            return true
        }
        let promises = pasteboard.readObjects(forClasses: [NSFilePromiseReceiver.self]) as? [NSFilePromiseReceiver] ?? []
        if !promises.isEmpty, let folder = try? DropText.folder() {
            let joined = Joined()
            for promise in promises {
                promise.receivePromisedFiles(atDestination: folder, options: [:], operationQueue: .main) { [weak self] url, error in
                    guard error == nil else { return }
                    MainActor.assumeIsolated {
                        self?.insertDropped(joined.next() + DropText.escape(url.path))
                    }
                }
            }
            return true
        }
        if let image = Self.png(pasteboard), let folder = try? DropText.folder() {
            let url = DropText.imageFile(in: folder)
            guard (try? image.write(to: url)) != nil else { return false }
            insertDropped(DropText.escape(url.path))
            return true
        }
        if let text = pasteboard.string(forType: .string), !text.isEmpty {
            insertDropped(text)
            return true
        }
        return false
    }

    private static func png(_ pasteboard: NSPasteboard) -> Data? {
        if let data = pasteboard.data(forType: .png) { return data }
        guard let data = pasteboard.data(forType: .tiff), let bitmap = NSBitmapImageRep(data: data) else { return nil }
        return bitmap.representation(using: .png, properties: [:])
    }
}

@MainActor
private final class Joined {
    private var started = false
    func next() -> String {
        defer { started = true }
        return started ? " " : ""
    }
}
