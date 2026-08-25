import Testing
@testable import Ghostty

@Suite
struct TerminalTitleTests {
    private final class Surface {
        let running: Bool

        init(running: Bool) {
            self.running = running
        }
    }

    @Test func tabOverrideComposesWithTerminalTitle() {
        #expect(BaseTerminalController.composeTitle(
            tabOverride: "Tab",
            terminalTitle: "Terminal") == "Tab - Terminal")
    }

    @Test func tabOverrideAloneWhenTerminalTitleEmpty() {
        #expect(BaseTerminalController.composeTitle(
            tabOverride: "Tab",
            terminalTitle: "") == "Tab")
    }

    @Test func terminalTitleAloneWithoutTabOverride() {
        #expect(BaseTerminalController.composeTitle(
            tabOverride: nil,
            terminalTitle: "Terminal") == "Terminal")
    }

    @Test func busyTitlesAreCombined() {
        #expect(BaseTerminalController.combineTitles(["one", "two"]) == "one, two")
    }

    @Test func focusedRunningSurfaceComesFirst() {
        let first = Surface(running: true)
        let focused = Surface(running: true)
        let last = Surface(running: true)

        let result = BaseTerminalController.selectTitleSurfaces(
            active: focused,
            ordered: [first, focused, last],
            isRunning: \.running)

        #expect(result.count == 3)
        #expect(result[0] === focused)
        #expect(result[1] === first)
        #expect(result[2] === last)
    }

    @Test func runningSurfacesKeepLayoutOrderWhenFocusedSurfaceIsIdle() {
        let focused = Surface(running: false)
        let first = Surface(running: true)
        let second = Surface(running: true)

        let result = BaseTerminalController.selectTitleSurfaces(
            active: focused,
            ordered: [focused, first, second],
            isRunning: \.running)

        #expect(result.count == 2)
        #expect(result[0] === first)
        #expect(result[1] === second)
    }

    @Test func focusedSurfaceWinsWhenAllSurfacesAreIdle() {
        let focused = Surface(running: false)
        let other = Surface(running: false)

        let result = BaseTerminalController.selectTitleSurfaces(
            active: focused,
            ordered: [other, focused],
            isRunning: \.running)

        #expect(result.count == 1)
        #expect(result[0] === focused)
    }

    @Test func activityStopIsOnlyAnActiveToIdleTransition() {
        var transition = BaseTerminalController.ActivityTransition()

        let initialIdle = transition.stopped(false)
        let started = transition.stopped(true)
        let remainedActive = transition.stopped(true)
        let stopped = transition.stopped(false)
        let remainedIdle = transition.stopped(false)

        #expect(!initialIdle)
        #expect(!started)
        #expect(!remainedActive)
        #expect(stopped)
        #expect(!remainedIdle)
    }

    @Test func activityCanStopMoreThanOnce() {
        var transition = BaseTerminalController.ActivityTransition()

        let firstStart = transition.stopped(true)
        let firstStop = transition.stopped(false)
        let secondStart = transition.stopped(true)
        let secondStop = transition.stopped(false)

        #expect(!firstStart)
        #expect(firstStop)
        #expect(!secondStart)
        #expect(secondStop)
    }

    @Test func activityStopsAreTrackedPerSurface() {
        var first = BaseTerminalController.ActivityTransition()
        var second = BaseTerminalController.ActivityTransition()

        let firstStarted = first.stopped(true)
        let secondStarted = second.stopped(true)
        let firstStopped = first.stopped(false)
        let secondRemainedActive = second.stopped(true)
        let secondStopped = second.stopped(false)

        #expect(!firstStarted)
        #expect(!secondStarted)
        #expect(firstStopped)
        #expect(!secondRemainedActive)
        #expect(secondStopped)
    }
}
