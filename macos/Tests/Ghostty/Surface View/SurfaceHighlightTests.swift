import Testing
@testable import Ghostty

@Suite
struct SurfaceHighlightTests {
    @Test func focusFrameIsHalfTheZoomFrameWidth() {
        #expect(Ghostty.OSSurfaceView.FiniteHighlight.focus.lineWidth == 1.5)
        #expect(Ghostty.OSSurfaceView.FiniteHighlight.zoom.lineWidth == 3)
    }

    @MainActor @Test func notificationAttentionRequiresAnUnfocusedSurface() {
        let surface = Ghostty.OSSurfaceView(id: nil, frame: .zero)

        surface.requestNotificationAttention(isFocused: true)
        #expect(!surface.notificationAttention)

        surface.requestNotificationAttention(isFocused: false)
        #expect(surface.notificationAttention)

        surface.clearNotificationAttention()
        #expect(!surface.notificationAttention)
    }
}
