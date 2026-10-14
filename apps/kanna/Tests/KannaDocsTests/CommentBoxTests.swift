import AppKit
import Testing
@testable import KannaDocs

@MainActor
struct CommentBoxTests {
    let long = String(repeating: "This comment runs on for a while so it wraps across many lines of the box. ", count: 12)

    func type(_ text: String, into field: BoxField) {
        field.setSelectedRange(NSRange(location: (field.string as NSString).length, length: 0))
        field.insertText(text, replacementRange: field.selectedRange())
    }

    func expectLaidOut(_ parts: [String: NSRect]) {
        let box = parts["box"]!, field = parts["field"]!
        #expect(box.insetBy(dx: 1, dy: 1).contains(field))
        for name in ["hint", "post"] {
            guard let part = parts[name] else { continue }
            #expect(box.insetBy(dx: 1, dy: 1).contains(part), "\(name) \(part) is outside the box \(box)")
            #expect(part.minY >= field.maxY, "\(name) \(part) overlaps the field \(field)")
        }
    }

    @Test func composerGrowsWithItsTextThenScrolls() {
        let box = CommentBox(composer: ())
        var resized = 0
        box.onResize = { _ in resized += 1 }
        let empty = box.parts
        #expect(empty["field"]!.height == 54)
        expectLaidOut(empty)
        type("Three\nshort\nlines\nand\na\nfifth", into: box.field)
        let five = box.parts
        #expect(five["field"]!.height > 54)
        #expect(five["box"]!.height > empty["box"]!.height)
        #expect(resized > 0)
        expectLaidOut(five)
        type(long, into: box.field)
        let full = box.parts
        #expect(full["field"]!.height == GrowField.most)
        #expect(full["text"]!.height > full["field"]!.height)
        expectLaidOut(full)
    }

    @Test func aNarrowComposerKeepsPostInside() {
        let box = CommentBox(composer: (), width: 200)
        type(long, into: box.field)
        #expect(box.parts["hint"] == nil)
        expectLaidOut(box.parts)
    }

    @Test func replyGrowsWithItsText() {
        let thread = DocThread(id: "c1", quote: "q", prefix: "", suffix: "", line: 1, state: "open", status: "sent",
                               messages: [DocMessage(who: "user", text: "First", at: "")], error: nil)
        let box = CommentBox(thread: thread, note: nil)
        let before = box.parts
        #expect(before["field"]!.height == 18)
        type(long, into: box.field)
        let after = box.parts
        #expect(after["field"]!.height == GrowField.most)
        #expect(after["box"]!.height - before["box"]!.height == GrowField.most - 18)
        expectLaidOut(after)
    }

    @Test func cardReplyGrowsTheCard() {
        let thread = DocThread(id: "c1", quote: "q", prefix: "", suffix: "", line: 1, state: "open", status: "sent",
                               messages: [DocMessage(who: "user", text: "First", at: "")], error: nil)
        let card = ThreadCard(thread: thread, active: true, note: nil, width: 260)
        let before = card.frame.height
        type(long, into: card.field)
        #expect(card.frame.height - before == GrowField.most - 18)
        #expect(card.bounds.contains(card.grow.frame))
    }
}
