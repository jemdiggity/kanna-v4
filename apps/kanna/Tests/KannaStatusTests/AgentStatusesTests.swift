import Testing
@testable import KannaStatus

struct AgentStatusesTests {
    @Test func finishingWhileYouLookElsewhereIsUnreadUntilYouLook() {
        var book = AgentStatuses()
        book.set("p1", "working", looking: false)
        book.set("p1", "idle", looking: false)
        #expect(book.statuses["p1"] == "idle")
        #expect(book.unread == ["p1"])
        book.looked(at: ["p2"])
        #expect(book.unread == ["p1"])
        book.looked(at: ["p1"])
        #expect(book.unread.isEmpty)
        #expect(book.statuses["p1"] == "idle")
    }

    @Test func finishingInFrontOfYouIsRead() {
        var book = AgentStatuses()
        book.set("p1", "working", looking: true)
        book.set("p1", "idle", looking: true)
        #expect(book.unread.isEmpty)
    }

    @Test func onlyWorkingToIdleMakesUnread() {
        var book = AgentStatuses()
        book.set("p1", "needs-you", looking: false)
        book.set("p1", "idle", looking: false)
        book.set("p2", "idle", looking: false)
        #expect(book.unread.isEmpty)
    }

    @Test func leavingIdleClearsUnread() {
        var book = AgentStatuses()
        book.set("p1", "working", looking: false)
        book.set("p1", "idle", looking: false)
        book.set("p1", "working", looking: false)
        #expect(book.unread.isEmpty)
        book.set("p1", "idle", looking: false)
        book.set("p1", "exited", looking: false)
        #expect(book.unread.isEmpty)
        #expect(book.statuses["p1"] == "exited")
    }

    @Test func closedPanesAreForgotten() {
        var book = AgentStatuses()
        book.set("p1", "working", looking: false)
        book.set("p1", "idle", looking: false)
        book.set("p2", "working", looking: false)
        book.keep(["p2"])
        #expect(book.statuses == ["p2": "working"])
        #expect(book.unread.isEmpty)
        book.set("p2", nil, looking: false)
        #expect(book.statuses["p2"] == nil)
    }

    @Test func theTaskWaitingLongestIsTheOneThatAskedFirst() {
        var book = AgentStatuses()
        book.set("p1", "needs-you", looking: false, at: 30)
        book.set("p2", "needs-you", looking: false, at: 10)
        book.set("p3", "working", looking: false, at: 5)
        book.set("p2", "needs-you", looking: false, at: 40)
        #expect(book.waitingLongest(among: ["p1", "p2", "p3"]) == "p2")
        #expect(book.waitingLongest(among: ["p1", "p3"]) == "p1")
        book.set("p2", "working", looking: false, at: 50)
        book.set("p2", "needs-you", looking: false, at: 60)
        #expect(book.waitingLongest(among: ["p1", "p2"]) == "p1")
        #expect(book.waitingLongest(among: ["p3"]) == nil)
    }
}
