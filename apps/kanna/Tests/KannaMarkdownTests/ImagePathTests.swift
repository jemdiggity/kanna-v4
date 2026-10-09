import Foundation
import Testing
@testable import KannaMarkdown

struct ImagePathTests {
    let folder: URL

    init() throws {
        folder = FileManager.default.temporaryDirectory.appendingPathComponent("kanna-md-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: folder.appendingPathComponent("img"), withIntermediateDirectories: true)
        try Data([0]).write(to: folder.appendingPathComponent("img/a.png"))
        try FileManager.default.createSymbolicLink(atPath: folder.appendingPathComponent("away").path, withDestinationPath: "/net/example.invalid/share")
        try FileManager.default.createSymbolicLink(atPath: folder.appendingPathComponent("near").path, withDestinationPath: "img")
    }

    func resolve(_ source: String) -> String? {
        MarkdownRenderer.localImagePath(source, folder: folder)
    }

    @Test func localFilesLoad() {
        let real = String(cString: realpath(folder.appendingPathComponent("img/a.png").path, nil))
        #expect(resolve("img/a.png") == real)
        #expect(resolve("img%2Fa.png") == real)
        #expect(resolve("near/a.png") == real)
        #expect(resolve(real) == real)
        #expect(resolve("file://" + real) == real)
        #expect(resolve("file://localhost" + real) == real)
    }

    @Test func remoteSourcesAreRefused() {
        for source in [
            "http://example.invalid/a.png", "HTTPS://example.invalid/a.png", "smb://example.invalid/a.png",
            "file://example.invalid/share/a.png", "FILE://example.invalid/share/a.png", "file:%2F%2Fexample.invalid/a.png",
            "/net/example.invalid/a.png", "//net/example.invalid/a.png", "/./net/example.invalid/a.png",
            "/private/net/example.invalid/a.png", "/System/Volumes/Data/net/example.invalid/a.png",
            "/Network/Servers/example.invalid/a.png", "/home/someone/a.png",
            "../../../../../../../../net/example.invalid/a.png", "away/a.png", "missing.png",
        ] {
            #expect(resolve(source) == nil, "\(source)")
        }
    }
}
