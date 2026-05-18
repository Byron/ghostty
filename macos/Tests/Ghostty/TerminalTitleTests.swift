import Testing
@testable import Ghostty

@Suite
struct TerminalTitleTests {
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
}
