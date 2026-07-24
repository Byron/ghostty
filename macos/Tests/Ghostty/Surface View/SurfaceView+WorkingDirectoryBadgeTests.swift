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
}
