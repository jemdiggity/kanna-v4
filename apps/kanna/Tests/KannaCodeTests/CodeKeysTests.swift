import Testing
@testable import KannaCode

struct CodeKeysTests {
    func moves(_ keys: [String], control: Bool = false) -> [CodeMove?] {
        var reader = CodeKeys()
        return keys.map { reader.press($0, control: control) }
    }

    @Test func lessKeysMoveByLinesHalfScreensAndScreens() {
        #expect(moves(["j", "k", "d", "u", "f", "b", " ", "G"]) == [.lines(1), .lines(-1), .pages(0.5), .pages(-0.5), .pages(1), .pages(-1), .pages(1), .bottom])
    }

    @Test func vimsControlKeysDoTheSame() {
        #expect(moves(["e", "y", "d", "u", "f", "b"], control: true) == [.lines(1), .lines(-1), .pages(0.5), .pages(-0.5), .pages(1), .pages(-1)])
    }

    @Test func gAndGgGoToTheTop() {
        #expect(moves(["g", "g"]) == [nil, .top])
        #expect(moves(["g", "j"]) == [nil, nil])
    }

    @Test func aCountRepeats() {
        #expect(moves(["5", "j"]) == [nil, .lines(5)])
        #expect(moves(["1", "2", "k"]) == [nil, nil, .lines(-12)])
        #expect(moves(["3", "}"]) == [nil, .blank(3)])
        #expect(moves(["2", "]", "]"]) == [nil, nil, .scope(2)])
    }

    @Test func scopesBlankLinesAndFinding() {
        #expect(moves(["]", "]", "[", "[", "}", "{"]) == [nil, .scope(1), nil, .scope(-1), .blank(1), .blank(-1)])
        #expect(moves(["/", "?", "n", "N", "\u{1b}"]) == [.find(forward: true), .find(forward: false), .match(forward: true), .match(forward: false), .stopFinding])
    }
}
