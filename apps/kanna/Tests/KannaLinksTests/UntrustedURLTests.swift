import Foundation
import Testing
@testable import KannaLinks

struct UntrustedURLTests {
    @Test(arguments: ["http://example.com", "https://example.com/path", "mailto:user@example.com"])
    func safeSchemesOpen(_ value: String) {
        #expect(UntrustedURL(value).decision == .allow(URL(string: value)!))
    }

    @Test(arguments: ["https:relative", "http:///missing-host"])
    func webLinksNeedAHost(_ value: String) {
        #expect(UntrustedURL(value).decision == .deny(.invalidWebURL))
    }

    @Test(arguments: ["/tmp/file.txt", "../file.txt", "payload.command", "", "mailto:"])
    func targetsWithoutASchemeAreRefused(_ value: String) {
        #expect(UntrustedURL(value).decision == .deny(.malformedURL))
    }

    @Test(arguments: ["vscode://file/tmp/example.swift", "ssh://example.com", "x-apple.systempreferences:com.apple.preference.security",
                      "javascript:alert(1)", "smb://server/share"])
    func otherSchemesAsk(_ value: String) {
        #expect(UntrustedURL(value).decision == .confirm(URL(string: value)!))
    }

    @Test(arguments: ["\u{0085}", "\u{2028}", "\u{2029}", "\u{202E}", "\u{2066}", "\u{200B}"])
    func invisibleAndLineBreakingCharactersAreRefused(_ scalar: String) {
        #expect(UntrustedURL("https://example.com/before\(scalar)after").decision == .deny(.unsafeCharacters))
        #expect(UntrustedURL(clicked: "/tmp/before\(scalar)after").decision == .deny(.unsafeCharacters))
    }

    @Test func plainLocalFilesOpen() throws {
        let folder = try Self.folder()
        defer { try? FileManager.default.removeItem(at: folder) }
        let file = folder.appending(path: "document.txt")
        try "safe".write(to: file, atomically: true, encoding: .utf8)
        let canonical = file.standardizedFileURL.resolvingSymlinksInPath()
        #expect(UntrustedURL(file.absoluteString).decision == .allow(canonical))
        #expect(UntrustedURL(clicked: file.path).decision == .allow(canonical))
        guard case .allow(let opened) = UntrustedURL(clicked: folder.path).decision else { Issue.record("a folder should open"); return }
        #expect(opened.path == folder.standardizedFileURL.resolvingSymlinksInPath().path)
    }

    @Test(arguments: ["payload.command", "payload.tool", "payload.app", "payload.workflow", "payload.terminal", "payload.webloc"])
    func filesThatRunCodeAreRefused(_ name: String) throws {
        let folder = try Self.folder()
        defer { try? FileManager.default.removeItem(at: folder) }
        let file = folder.appending(path: name)
        try "#!/bin/sh\n".write(to: file, atomically: true, encoding: .utf8)
        #expect(UntrustedURL(file.absoluteString).decision == .deny(.unsafeFile))
        #expect(UntrustedURL(clicked: file.path).decision == .deny(.unsafeFile))
    }

    @Test func applicationBundlesAreRefused() throws {
        let folder = try Self.folder()
        defer { try? FileManager.default.removeItem(at: folder) }
        let bundle = folder.appending(path: "Payload.app/Contents", directoryHint: .isDirectory)
        try FileManager.default.createDirectory(at: bundle, withIntermediateDirectories: true)
        #expect(UntrustedURL(clicked: folder.appending(path: "Payload.app").path).decision == .deny(.unsafeFile))
        #expect(UntrustedURL("file:///System/Applications/Calculator.app").decision == .deny(.unsafeFile))
    }

    @Test func scriptsAndExecutablesAreRefusedWhateverTheirName() throws {
        let folder = try Self.folder()
        defer { try? FileManager.default.removeItem(at: folder) }
        let script = folder.appending(path: "payload.sh")
        try "#!/bin/sh\n".write(to: script, atomically: true, encoding: .utf8)
        let disguised = folder.appending(path: "payload.txt")
        try "#!/bin/sh\n".write(to: disguised, atomically: true, encoding: .utf8)
        try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: disguised.path)
        #expect(UntrustedURL(script.absoluteString).decision == .deny(.unsafeFile))
        #expect(UntrustedURL(disguised.absoluteString).decision == .deny(.unsafeFile))
        #expect(UntrustedURL(clicked: "/bin/sh").decision == .deny(.unsafeFile))
    }

    @Test func symlinksAreJudgedByWhatTheyPointAt() throws {
        let folder = try Self.folder()
        defer { try? FileManager.default.removeItem(at: folder) }
        let payload = folder.appending(path: "payload.command")
        let link = folder.appending(path: "document.txt")
        try "#!/bin/sh\n".write(to: payload, atomically: true, encoding: .utf8)
        try FileManager.default.createSymbolicLink(at: link, withDestinationURL: payload)
        #expect(UntrustedURL(link.absoluteString).decision == .deny(.unsafeFile))
    }

    @Test func remoteAndOddFileLinksAreRefused() {
        #expect(UntrustedURL("file://example.com/etc/hosts").decision == .deny(.malformedURL))
        #expect(UntrustedURL("file:///etc/hosts?x=1").decision == .deny(.malformedURL))
        #expect(UntrustedURL("file:///no/such/file").decision == .deny(.inaccessibleFile))
        #expect(UntrustedURL("file:///dev/null").decision == .deny(.inaccessibleFile))
        #expect(UntrustedURL(clicked: "relative/file.txt").decision == .deny(.malformedURL))
    }

    @Test func theShownTargetIsTheOneOpened() {
        #expect(UntrustedURL("/tmp/preview////../payload.command////").displayString == "/tmp/payload.command")
        #expect(UntrustedURL("https://example.com/a\u{202E}b").displayString == "https://example.com/a\\u{202E}b")
    }

    static func folder() throws -> URL {
        let folder = FileManager.default.temporaryDirectory.appending(path: "kanna-links-\(UUID().uuidString)", directoryHint: .isDirectory)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: false)
        return folder
    }
}
