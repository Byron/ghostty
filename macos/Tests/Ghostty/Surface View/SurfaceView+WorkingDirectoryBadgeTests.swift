import Testing
@testable import Ghostty

@MainActor struct SurfaceView_WorkingDirectoryBadgeTests {
    typealias Badge = Ghostty.WorkingDirectoryBadge

    @Test(arguments: [
        ("/Users/ghostty/project", "project"),
        ("/Users/ghostty/project/", "project"),
        ("/", "/"),
    ])
    func basename(pwd: String, expected: String) {
        let presentation = Badge.presentation(
            pwd: pwd,
            isFocusedSurface: false,
            windowFocus: true
        )

        #expect(presentation?.name == expected)
    }

    @Test(arguments: [nil, ""])
    func missingPwdIsHidden(pwd: String?) {
        #expect(Badge.presentation(
            pwd: pwd,
            isFocusedSurface: false,
            windowFocus: true
        ) == nil)
    }

    @Test func focusedSurfaceInFocusedWindowIsHidden() {
        #expect(Badge.presentation(
            pwd: "/project",
            isFocusedSurface: true,
            windowFocus: true
        ) == nil)
    }

    @Test func focusedSurfaceInFocusedWindowIsShownProminentlyWhenRequested() {
        #expect(Badge.presentation(
            pwd: "/project",
            isFocusedSurface: true,
            windowFocus: true,
            showFocusedSurface: true
        )?.style == .focused)
    }

    @Test func unfocusedSurfaceInFocusedWindowUsesNormalStyle() {
        #expect(Badge.presentation(
            pwd: "/project",
            isFocusedSurface: false,
            windowFocus: true
        )?.style == .normal)
    }

    @Test func focusedSurfaceInUnfocusedWindowUsesFocusedStyle() {
        #expect(Badge.presentation(
            pwd: "/project",
            isFocusedSurface: true,
            windowFocus: false
        )?.style == .focused)
    }

    @Test func unfocusedSurfaceInUnfocusedWindowUsesNormalStyle() {
        #expect(Badge.presentation(
            pwd: "/project",
            isFocusedSurface: false,
            windowFocus: false
        )?.style == .normal)
    }

    @Test func activityIndicatorOnlyShowsForActiveUnfocusedSurface() {
        let inactive = Badge.Presentation(name: "project", style: .normal)
        let focused = Badge.Presentation(name: "project", style: .focused)

        #expect(Badge.showsActivityIndicator(isActive: true, presentation: inactive))
        #expect(!Badge.showsActivityIndicator(isActive: false, presentation: inactive))
        #expect(!Badge.showsActivityIndicator(isActive: true, presentation: focused))
    }

    @Test func badgeIsFiveCellsToTheRightOfTheCursor() {
        #expect(Badge.cursorAdjacentX(
            containerWidth: 300,
            cursorCenterX: 50,
            labelWidth: 100,
            cellWidth: 10
        ) == 155)
    }

    @Test func badgeMovesLeftWhenItDoesNotFitToTheRight() {
        #expect(Badge.cursorAdjacentX(
            containerWidth: 200,
            cursorCenterX: 150,
            labelWidth: 60,
            cellWidth: 10,
        ) == 65)
    }

    @Test func detectsCursorMovement() async throws {
        #expect(try await Badge.cursorMoved(
            from: .zero,
            currentPosition: { CGPoint(x: 1, y: 0) }
        ))
    }
}
