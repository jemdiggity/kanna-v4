import Foundation
import Testing
@testable import KannaDrop

struct DropTextTests {
    @Test func plainPathsAreUnchanged() {
        #expect(DropText.escape("/Users/me/shot.png") == "/Users/me/shot.png")
    }

    @Test func shellCharactersAreEscapedAsGhosttyDoes() {
        #expect(DropText.escape("/tmp/Screen Shot (1).png") == ##"/tmp/Screen\ Shot\ \(1\).png"##)
        #expect(DropText.escape(##"/a/it's $HOME & "x"\y"##) == ##"/a/it\'s\ \$HOME\ \&\ \"x\"\\y"##)
        #expect(DropText.escape(##"/a/[b]{c}<d>`e`!f#g;h|i*j?k"##) == ##"/a/\[b\]\{c\}\<d\>\`e\`\!f\#g\;h\|i\*j\?k"##)
        #expect(DropText.escape("a\tb") == "a\\\tb")
    }

    @Test func severalPathsAreJoinedBySpaces() {
        #expect(DropText.paths(["/a b", "/c"]) == #"/a\ b /c"#)
    }

    @Test func droppedImagesGetTheirOwnFile() throws {
        let folder = try DropText.folder()
        let one = DropText.imageFile(in: folder)
        let two = DropText.imageFile(in: folder)
        #expect(one != two)
        #expect(one.pathExtension == "png")
    }
}
