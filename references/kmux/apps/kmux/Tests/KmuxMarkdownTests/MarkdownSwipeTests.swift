import AppKit
import Testing
@testable import KmuxMarkdown

/// A two-finger swipe sideways navigates only once the page can't scroll that way.
struct MarkdownSwipeTests {
    @Test func swipesOnlyAtTheEdge() {
        let page = NSRect(x: 0, y: 0, width: 800, height: 2000)
        // Actual size: the whole width shows, so both ways navigate.
        #expect(MarkdownScrollView.swipes(back: true, visible: NSRect(x: 0, y: 0, width: 800, height: 600), page: page))
        #expect(MarkdownScrollView.swipes(back: false, visible: NSRect(x: 0, y: 0, width: 800, height: 600), page: page))
        // Magnified and panned to the middle: both ways pan instead.
        let middle = NSRect(x: 200, y: 0, width: 400, height: 300)
        #expect(!MarkdownScrollView.swipes(back: true, visible: middle, page: page))
        #expect(!MarkdownScrollView.swipes(back: false, visible: middle, page: page))
        // At the left edge: back navigates, forward pans.
        let left = NSRect(x: 0, y: 0, width: 400, height: 300)
        #expect(MarkdownScrollView.swipes(back: true, visible: left, page: page))
        #expect(!MarkdownScrollView.swipes(back: false, visible: left, page: page))
        // Zoomed out, the page sits in the middle of a wider view: both navigate.
        #expect(MarkdownScrollView.swipes(back: false, visible: NSRect(x: -200, y: 0, width: 1200, height: 900), page: page))
    }
}
