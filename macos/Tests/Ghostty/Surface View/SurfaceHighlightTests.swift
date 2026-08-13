import Testing
@testable import Ghostty

@Suite
struct SurfaceHighlightTests {
    @Test func focusFrameIsHalfTheZoomFrameWidth() {
        #expect(Ghostty.OSSurfaceView.FiniteHighlight.focus.lineWidth == 1.5)
        #expect(Ghostty.OSSurfaceView.FiniteHighlight.zoom.lineWidth == 3)
    }
}
