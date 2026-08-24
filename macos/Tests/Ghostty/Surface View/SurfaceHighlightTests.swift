import AppKit
import SwiftUI
import Testing
@testable import Ghostty

@Suite
struct SurfaceHighlightTests {
    @Test func focusFrameIsHalfTheZoomFrameWidth() {
        #expect(Ghostty.OSSurfaceView.FiniteHighlight.focus.lineWidth == 1.5)
        #expect(Ghostty.OSSurfaceView.FiniteHighlight.zoom.lineWidth == 3)
    }

    @MainActor @Test func focusFrameUsesProvidedAccentColor() throws {
        let expected = NSColor(srgbRed: 0.12, green: 0.73, blue: 0.41, alpha: 1)
        let view = NSHostingView(rootView:
            Ghostty.FiniteHighlightOverlay(highlight: .zoom)
                .ghosttyAccentColor(Color(nsColor: expected)))
        view.frame = NSRect(x: 0, y: 0, width: 20, height: 20)
        view.layoutSubtreeIfNeeded()

        let image = try #require(view.bitmapImageRepForCachingDisplay(in: view.bounds))
        view.cacheDisplay(in: view.bounds, to: image)
        let actual = try #require(image.colorAt(x: 1, y: 10)?.usingColorSpace(.sRGB))

        #expect(abs(actual.redComponent - expected.redComponent) < 0.005)
        #expect(abs(actual.greenComponent - expected.greenComponent) < 0.005)
        #expect(abs(actual.blueComponent - expected.blueComponent) < 0.005)
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
