"""Selection extraction, literal search and tracked references through live edits."""


def requests():
    def grid(action="observe", **options):
        return {"op": "grid", "grid": {"action": action, **options}}

    def point(x=0, y=0, tag="active"):
        return {"tag": tag, "x": x, "y": y}

    def case(name, operations, covers, **options):
        return ({"id": "grid/" + name, "cols": 8, "rows": 4,
                 "operations": [op if isinstance(op, dict) else {"op": "write", "data": op.hex()}
                                for op in operations], **options}, covers)

    observe = grid()
    texts = [b"", b"one two", b"  a  \r\n b   ", b"abcdefghijklmnopqr",
             "界aé🙂".encode(), "e\u0301 #\ufe0f!".encode()]
    bounds = [(point(), point(3)), (point(3), point()), (point(1), point(6, 1)),
              (point(6, 1), point(1)), (point(6), point(1, 1)),
              (point(1, 1), point(6)), (point(1), point(1)), (point(7), point(7, 3))]
    for text_index, text in enumerate(texts):
        for bound_index, (start, end) in enumerate(bounds):
            for rectangle in (False, True):
                yield case(f"selection/bounds/{text_index}/{bound_index}/{int(rectangle)}",
                           [text, grid("select", start=start, end=end, rectangle=rectangle),
                            grid("clear_selection"), observe], ["terminal.selection"])

    # Derived from native Screen.selectionString tests; CRLF reproduces the
    # testWriteString helper's hard line break through the real stream API.
    native_selections = [
        ("trim-space", b"1AB  \r\n2EFGH\r\n3IJKL", point(), point(2, 1)),
        ("internal-empty-line", b"1AB  \r\n\r\n2EFGH\r\n3IJKL", point(), point(2, 2)),
        ("internal-written-spaces", b"1AB\r\n     \r\n2EFGH", point(), point(2, 2)),
        ("unwritten-tail", b"A", point(), point(4, 3)),
        ("written-space-tail", b"A\r\n     ", point(), point(4, 3)),
        ("only-unwritten", b"", point(), point(4, 2)),
        ("only-written-spaces", b"     \r\n     ", point(), point(4, 2)),
        ("soft-wrap", b"1ABCD2EFGH3IJKL", point(0, 1), point(2, 2)),
        ("wide-end-tail", "1A⚡".encode(), point(), point(3)),
        ("wide-end-lead", "1A⚡".encode(), point(), point(2)),
        ("wide-only-tail", "1A⚡".encode(), point(3), point(3)),
        ("wide-end-head", "1ABC⚡".encode(), point(), point(4)),
        ("wide-start-head", "1ABC⚡Z".encode(), point(4), point(2, 1)),
        ("wide-only-head", "1ABC⚡".encode(), point(4), point(4)),
        ("wide-empty-soft-wrap", "👨      ".encode(), point(1), point(2)),
        ("partial-wrap-spaces", b"ab    cd", point(), point(3)),
        ("final-wrap-spaces", b"ab    cd", point(), point(4)),
        ("zwj", b"\x1b[?2027h" + "👨‍".encode(), point(), point(1)),
    ]
    for name, text, start, end in native_selections:
        for reverse in (False, True):
            for rectangle in (False, True):
                a, b = (end, start) if reverse else (start, end)
                yield case(f"selection/native/{name}/{int(reverse)}/{int(rectangle)}",
                           [text, grid("select", start=a, end=b, rectangle=rectangle)],
                           ["terminal.selection"], cols=5, rows=5)

    changes = {
        "erase-line": b"\x1b[1;1H\x1b[2K", "erase-display": b"\x1b[2J",
        "erase-history": b"\x1b[3J", "delete-line": b"\x1b[1;1H\x1b[M",
        "scroll": b"\r\nE\r\nF\r\nG\r\nH", "wrap": b"01234567890123456789",
        "narrow": {"op": "resize", "cols": 5, "rows": 4},
        "wide": {"op": "resize", "cols": 12, "rows": 4},
        "short": {"op": "resize", "cols": 8, "rows": 2},
        "tall": {"op": "resize", "cols": 8, "rows": 6},
        "reset": {"op": "terminal_reset"}, "ris": b"\x1bc",
        "alternate47": b"\x1b[?47hother\x1b[?47l",
        "alternate1049": b"\x1b[?1049hother\x1b[?1049l",
    }
    for name, change in changes.items():
        initial = b"abcdefghijklmnopqr\r\nlast"
        yield case("selection/mutation/" + name,
                   [initial, grid("select", start=point(1), end=point(4, 1)), change, observe],
                   ["terminal.selection", "terminal.resize", "terminal.tracked"])
        for tag in ("active", "viewport", "screen", "history"):
            yield case(f"tracked/mutation/{name}/{tag}",
                       [initial, grid("track", id=1, point=point(1, 0, tag)),
                        grid("track", id=2, point=point(4, 1, tag)), change, observe],
                       ["terminal.tracked", "terminal.resize"])

    history = b"zero\r\none\r\ntwo\r\nthree\r\nfour\r\nfive"
    for delta in (-100, -2, -1, 0, 1, 100):
        yield case(f"viewport/{delta}", [history, grid("viewport", delta=delta),
                   grid("track", id=1, point=point(1, 0, "viewport")),
                   grid("select", start=point(0, 0, "viewport"), end=point(2, 1, "viewport")),
                   b"\r\nsix", observe], ["terminal.selection", "terminal.tracked"])
    for reset in ({"op": "terminal_reset"}, {"op": "reset"}):
        yield case("tracked/reuse/" + reset["op"], [b"A", grid("track", id=1), reset,
                   b"BC", grid("track", id=2, point=point(1)), observe, grid("untrack", id=2)],
                   ["terminal.tracked", "terminal.reset"])
    yield case("tracked/untrack", [b"AB", grid("track", id=1), grid("untrack", id=1),
               grid("track", id=2, point=point(1)), observe, grid("untrack", id=2)], ["terminal.tracked"])
    yield case("tracked/untrack-inactive", [b"AB", grid("track", id=1), b"\x1b[?47h",
               grid("untrack", id=1)], ["terminal.tracked"])
    for limit in (0, 1, 4):
        yield case(f"tracked/prune/{limit}", [history, grid("track", id=1, point=point(tag="screen")),
                   grid("limits", lines=limit), b"\r\nmore\r\nrows", observe], ["terminal.tracked"])

    search_texts = [b"one two one", b"abcabcabc", b"abababa", b"abcdefghijklmnopqr",
                    b"one\r\none\r\none\r\none\r\none", b"a  b", "a界e\u0301🙂界".encode()]
    needles = [b"", b"one", b"abc", b"aba", b"hij", b" ", b"a.b", "界".encode(), "\u0301".encode()]
    for ti, text in enumerate(search_texts):
        for ni, needle in enumerate(needles):
            yield case(f"search/literal/{ti}/{ni}", [text, grid("search", needle=needle.hex())],
                       ["terminal.search"])
    for name, change in changes.items():
        yield case("search/mutation/" + name, [history, grid("search", needle=b"o".hex()), change,
                   grid("search", needle=b"o".hex())], ["terminal.search"])
    yield case("search/raw-needle", [b"abc", grid("search", needle="ff")], ["terminal.search"])
