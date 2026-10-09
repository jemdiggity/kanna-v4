import Foundation
import Testing
@testable import KannaLinks

struct UntrustedURLTests {
    let folder: URL

    init() throws {
        folder = FileManager.default.temporaryDirectory.appending(path: "kanna-links-\(UUID().uuidString)", directoryHint: .isDirectory)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: false)
    }

    func file(_ name: String, _ text: String = "plain\n", mode: Int = 0o644) throws -> URL {
        let url = folder.appending(path: name)
        try FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        try text.write(to: url, atomically: true, encoding: .utf8)
        try FileManager.default.setAttributes([.posixPermissions: mode], ofItemAtPath: url.path)
        return url
    }

    func canonical(_ url: URL) -> URL { URL(filePath: url.standardizedFileURL.resolvingSymlinksInPath().path) }

    static let quiet: @Sendable (URL) -> String? = { _ in "com.apple.TextEdit" }

    @Test(arguments: ["http://example.com", "https://example.com/path?q=1#f", "HTTPS://Example.com/", "mailto:user@example.com",
                      "https://user@example.com/", "https://example.com/a%20b"])
    func webAndMailOpenTheURLThatWasChecked(_ value: String) {
        #expect(UntrustedURL(value).decision == .allow(URL(string: value)!))
    }

    @Test(arguments: ["https:relative", "http:///missing-host", "https:\\\\evil.example", "https://%20/", "http:"])
    func webLinksNeedAHost(_ value: String) {
        #expect(UntrustedURL(value).decision == .deny(.invalidWebURL))
    }

    @Test(arguments: ["vscode://file/tmp/example.swift", "ssh://example.com", "x-apple.systempreferences:com.apple.preference.security",
                      "javascript:alert(1)", "smb://server/share", "data:text/html,<script>1</script>", "x-man-page://ls", "ftp://example.com/",
                      "applescript://com.apple.scripteditor?action=new&script=1", "itms-services://?action=download-manifest", "vnc://host",
                      "afp://host/share", "telnet://host", "help:anchor=x", "shortcuts://run-shortcut?name=x"])
    func everyOtherSchemeIsRefused(_ value: String) {
        #expect(UntrustedURL(value).decision == .deny(.unsupportedScheme))
        #expect(UntrustedURL(clicked: value).decision == .deny(.unsupportedScheme))
    }

    @Test(arguments: ["/tmp/file.txt", "../file.txt", "payload.command", "", "mailto:", " https://example.com", "relative/file.txt"])
    func targetsWithoutAUsableSchemeAreRefused(_ value: String) {
        #expect(UntrustedURL(value).decision == .deny(.malformedURL) || UntrustedURL(value).decision == .deny(.unsafeCharacters))
    }

    @Test(arguments: ["\u{0085}", "\u{00A0}", "\u{2028}", "\u{2029}", "\u{202E}", "\u{2066}", "\u{200B}", "\u{2060}", "\u{FEFF}", "\u{3000}",
                      "\u{3164}", "\u{115F}", "\u{0007}", "\n", "\t"])
    func invisibleAndControlCharactersAreRefused(_ scalar: String) {
        #expect(UntrustedURL("https://example.com/before\(scalar)after").decision == .deny(.unsafeCharacters))
        #expect(UntrustedURL("https://exa\(scalar)mple.com/").decision == .deny(.unsafeCharacters))
        #expect(UntrustedURL(clicked: "/tmp/before\(scalar)after").decision == .deny(.unsafeCharacters))
    }

    @Test func plainDocumentsAndFoldersOpen() throws {
        let notes = try file("notes.txt")
        #expect(UntrustedURL(notes.absoluteString, handler: Self.quiet).decision == .allow(canonical(notes)))
        #expect(UntrustedURL("file://localhost\(notes.path)", handler: Self.quiet).decision == .allow(canonical(notes)))
        #expect(UntrustedURL(clicked: notes.path, handler: Self.quiet).decision == .allow(canonical(notes)))
        #expect(UntrustedURL(clicked: folder.path + "/./x/../notes.txt", handler: Self.quiet).decision == .allow(canonical(notes)))
        #expect(UntrustedURL(clicked: folder.path, handler: Self.quiet).decision == .allow(canonical(folder)))
    }

    @Test(arguments: ["payload.command", "payload.tool", "payload.terminal", "payload.webloc", "payload.inetloc", "payload.fileloc",
                      "payload.afploc", "payload.shortcut", "payload.workflow", "payload.jnlp", "payload.jar", "payload.pkg", "payload.mpkg",
                      "payload.mobileconfig", "payload.scpt", "payload.applescript", "payload.sh", "payload.zsh", "payload.py",
                      "payload.url", "payload.webarchive", "payload.prefPane", "PAYLOAD.COMMAND", "Payload.Terminal"])
    func filesThatRunCodeOrPointElsewhereAreRefused(_ name: String) throws {
        let payload = try file(name, "#!/bin/sh\n")
        #expect(UntrustedURL(payload.absoluteString, handler: Self.quiet).decision == .deny(.unsafeFile))
        #expect(UntrustedURL(clicked: payload.path, handler: Self.quiet).decision == .deny(.unsafeFile))
    }

    @Test func percentEncodingCannotHideAName() throws {
        let payload = try file("run.command", "#!/bin/sh\n")
        let encoded = "file://" + folder.path + "/run%2Ecommand"
        #expect(UntrustedURL(encoded, handler: Self.quiet).decision == .deny(.unsafeFile))
        #expect(UntrustedURL("file://" + folder.path + "/run%2ecommand", handler: Self.quiet).decision == .deny(.unsafeFile))
        #expect(UntrustedURL(payload.absoluteString.replacingOccurrences(of: "/run", with: "/%72un"), handler: Self.quiet).decision == .deny(.unsafeFile))
    }

    @Test func applicationsAndOtherPackagesAreRefused() throws {
        try FileManager.default.createDirectory(at: folder.appending(path: "Payload.app/Contents/MacOS"), withIntermediateDirectories: true)
        let inside = try file("Payload.app/Contents/Resources/readme.txt")
        try FileManager.default.createDirectory(at: folder.appending(path: "Notes.rtfd"), withIntermediateDirectories: true)
        try FileManager.default.createDirectory(at: folder.appending(path: "Hidden.APP"), withIntermediateDirectories: true)
        #expect(UntrustedURL(clicked: folder.appending(path: "Payload.app").path, handler: Self.quiet).decision == .deny(.unsafeFile))
        #expect(UntrustedURL(clicked: folder.appending(path: "Payload.app").path + "/", handler: Self.quiet).decision == .deny(.unsafeFile))
        #expect(UntrustedURL(clicked: folder.appending(path: "Hidden.APP").path, handler: Self.quiet).decision == .deny(.unsafeFile))
        #expect(UntrustedURL(clicked: inside.path, handler: Self.quiet).decision == .deny(.unsafeFile))
        #expect(UntrustedURL(clicked: folder.appending(path: "Notes.rtfd").path, handler: Self.quiet).decision == .deny(.unsafeFile))
        #expect(UntrustedURL("file:///System/Applications/Calculator.app", handler: Self.quiet).decision == .deny(.unsafeFile))
        #expect(UntrustedURL("file:///System/Applications/Calculator.app/Contents/MacOS/Calculator", handler: Self.quiet).decision == .deny(.unsafeFile))
    }

    @Test func executablesAreRefusedWhateverTheirName() throws {
        let disguised = try file("payload.txt", "#!/bin/sh\n", mode: 0o755)
        let bare = try file("payload", "#!/bin/sh\n", mode: 0o700)
        #expect(UntrustedURL(disguised.absoluteString, handler: Self.quiet).decision == .deny(.unsafeFile))
        #expect(UntrustedURL(clicked: bare.path, handler: Self.quiet).decision == .deny(.unsafeFile))
        #expect(UntrustedURL(clicked: "/bin/sh", handler: Self.quiet).decision == .deny(.unsafeFile))
    }

    @Test func linksAreJudgedByWhatTheyPointAt() throws {
        let payload = try file("payload.command", "#!/bin/sh\n")
        let link = folder.appending(path: "document.txt")
        try FileManager.default.createSymbolicLink(at: link, withDestinationURL: payload)
        try FileManager.default.createDirectory(at: folder.appending(path: "Real.app/Contents"), withIntermediateDirectories: true)
        let inner = try file("Real.app/Contents/info.txt")
        try FileManager.default.createSymbolicLink(at: folder.appending(path: "plain"), withDestinationURL: folder.appending(path: "Real.app"))
        #expect(UntrustedURL(link.absoluteString, handler: Self.quiet).decision == .deny(.unsafeFile))
        #expect(UntrustedURL(clicked: folder.appending(path: "plain").path, handler: Self.quiet).decision == .deny(.unsafeFile))
        #expect(UntrustedURL(clicked: folder.appending(path: "plain/Contents/info.txt").path, handler: Self.quiet).decision == .deny(.unsafeFile))
        _ = inner
    }

    @Test func finderAliasesAreRefused() throws {
        let target = try file("target.txt")
        let alias = folder.appending(path: "alias.txt")
        let data = try target.bookmarkData(options: .suitableForBookmarkFile, includingResourceValuesForKeys: nil, relativeTo: nil)
        try URL.writeBookmarkData(data, to: alias)
        #expect(UntrustedURL(clicked: alias.path, handler: Self.quiet).decision == .deny(.unsafeFile))
    }

    @Test func documentsThatACodeRunnerWouldOpenAreRefused() throws {
        let notes = try file("notes.txt")
        for app in ["com.apple.Terminal", "com.googlecode.iterm2", "com.apple.ScriptEditor2", "com.apple.Installer", "com.apple.shortcuts"] {
            #expect(UntrustedURL(notes.absoluteString, handler: { _ in app }).decision == .deny(.unsafeFile))
        }
        #expect(UntrustedURL(notes.absoluteString, handler: { _ in nil }).decision == .allow(canonical(notes)))
    }

    @Test func remoteAndOddFileLinksAreRefused() throws {
        let notes = try file("notes.txt")
        #expect(UntrustedURL("file://example.com" + notes.path).decision == .deny(.malformedURL))
        #expect(UntrustedURL("file://user@localhost" + notes.path).decision == .deny(.malformedURL))
        #expect(UntrustedURL("file://localhost:22" + notes.path).decision == .deny(.malformedURL))
        #expect(UntrustedURL(notes.absoluteString + "?x=1").decision == .deny(.malformedURL))
        #expect(UntrustedURL(notes.absoluteString + "#x").decision == .deny(.malformedURL))
        #expect(UntrustedURL("file:///no/such/file").decision == .deny(.inaccessibleFile))
        #expect(UntrustedURL("file:///dev/null").decision == .deny(.inaccessibleFile))
    }

    @Test func theShownTargetIsTheOneOpened() {
        #expect(UntrustedURL("/tmp/preview////../payload.command////").displayString == "/tmp/payload.command")
        #expect(UntrustedURL("https://example.com/a\u{202E}b").displayString == "https://example.com/a\\u{202E}b")
    }
}
