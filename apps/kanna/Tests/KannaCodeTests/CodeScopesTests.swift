import Testing
@testable import KannaCode

struct CodeScopesTests {
    let scopes = [CodeScope(first: 5, last: 13), CodeScope(first: 7, last: 12), CodeScope(first: 8, last: 10), CodeScope(first: 15, last: 19), CodeScope(first: 16, last: 18)]

    @Test func aLineIsInTheScopesOpenedAboveItThatCloseBelowIt() {
        #expect(CodeScopes.open(at: 9, in: scopes) == [5, 7, 8])
        #expect(CodeScopes.open(at: 11, in: scopes) == [5, 7])
        #expect(CodeScopes.open(at: 14, in: scopes) == [])
        #expect(CodeScopes.open(at: 5, in: scopes) == [])
    }

    @Test func aCommentedLineTakesTheScopeItOpensOrTheSmallestItSitsIn() {
        let nested = scopes + [CodeScope(first: 5, last: 6)]
        #expect(CodeScopes.snap(line: 5, last: nil, in: nested, lineCount: 30) == CodeScope(first: 5, last: 13))
        #expect(CodeScopes.snap(line: 9, last: nil, in: scopes, lineCount: 30) == CodeScope(first: 8, last: 10))
        #expect(CodeScopes.snap(line: 11, last: nil, in: scopes, lineCount: 30) == CodeScope(first: 7, last: 12))
        #expect(CodeScopes.snap(line: 14, last: nil, in: scopes, lineCount: 30) == CodeScope(first: 14, last: 14))
        #expect(CodeScopes.snap(line: 9, last: 9, in: scopes, lineCount: 30) == CodeScope(first: 9, last: 9))
        #expect(CodeScopes.snap(line: 28, last: 40, in: scopes, lineCount: 30) == CodeScope(first: 28, last: 30))
    }

    @Test func pinnedRowsAreTheScopesOfTheFirstLineBelowThem() {
        #expect(CodeScopes.pinned(top: 1, in: scopes) == [])
        #expect(CodeScopes.pinned(top: 6, in: scopes) == [5])
        #expect(CodeScopes.pinned(top: 9, in: scopes) == [5, 7])
        #expect(CodeScopes.pinned(top: 8, in: scopes) == [5, 7])
        #expect(CodeScopes.pinned(top: 10, in: scopes) == [5, 7])
        #expect(CodeScopes.pinned(top: 13, in: scopes) == [])
        #expect(CodeScopes.pinned(top: 17, in: scopes) == [15])
        #expect(CodeScopes.pinned(top: 16, in: scopes) == [15, 16])
    }

    @Test func aPickedLineLandsJustBelowItsPinnedScopes() {
        let long = [CodeScope(first: 1, last: 63), CodeScope(first: 2, last: 62), CodeScope(first: 3, last: 61)]
        #expect(CodeScopes.top(showing: 40, in: long) == 37)
        #expect(CodeScopes.pinned(top: 37, in: long) == [1, 2, 3])
        #expect(CodeScopes.top(showing: 2, in: long) == 1)
        #expect(CodeScopes.top(showing: 9, in: scopes) == 6)
    }

    @Test func noMoreThanFiveRows() {
        let deep = (1...8).map { CodeScope(first: $0, last: 40 - $0) }
        #expect(CodeScopes.pinned(top: 20, in: deep) == [1, 2, 3, 4, 5])
    }

    @Test func blankLinesTakeTheNextLinesIndent() {
        let file = CodeFile(text: "fn a() {\n    if x {\n\n        y\n    }\n}\n")
        #expect(CodeScopes.indents(file) == [0, 4, 8, 8, 4, 0])
        #expect(CodeScopes.unit(CodeScopes.indents(file)) == 4)
        #expect(CodeScopes.indents(CodeFile(text: "a:\n\tb\n")) == [0, 4])
        #expect(CodeScopes.unit([0, 2, 4, 2, 0]) == 2)
    }
}
