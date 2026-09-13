"""Compare both snapshot encodings in both decoders and with uninterrupted state."""


def write(data):
    return {"op": "write", "data": data.hex()}


def requests():
    def case(name, prefix, suffix=b"\x1b[0mZ", cols=12, rows=4):
        operations = [write(prefix)] if isinstance(prefix, bytes) else prefix
        return ({"id": "snapshot/" + name, "kind": "snapshot", "cols": cols, "rows": rows,
                 "operations": operations, "after": [write(suffix)]}, ["snapshot.cross-decode"])

    yield case("empty", b"")
    yield case("unicode", "é界👩🏽‍💻".encode())
    yield case("styles-links", b"\x1b[1;3;38:2::10:20:30mA\x1b]8;id=x;https://example.org\x1b\\B")
    yield case("saved-cursor", b"A\x1b[31m\x1b7\x1b[4;8HB", b"\x1b8C")
    yield case("alt-active", b"primary\x1b[?1049halternate", b"\x1b[?1049lZ")
    yield case("alt-inactive", b"primary\x1b[?47halternate\x1b[?47l", b"\x1b[?47hZ")
    yield case("pending-wrap", b"abcdefghijkl", b"M")
    yield case("history", b"".join(f"line {i}\r\n".encode() for i in range(40)))
    yield case("margins-tabs", b"\x1b[2;4r\x1b[?69h\x1b[3;10s\x1b[3g\x1b[1;5H\x1bH", b"\r\tX\nY")
    yield case("modes", b"\x1b[?7l\x1b[4h\x1b[?25l\x1b[2 q\x1b[?1h", b"abcdefghijklm\x1b[6n")
    yield case("kitty-ring", b"".join(f"\x1b[>{i}u".encode() for i in range(1, 11)),
               b"\x1b[<2u\x1b[?u\x1b[<1u\x1b[?u\x1b[=16;2u\x1b[?u\x1b[=3;3u\x1b[?u\x1b[<8u\x1b[?u")
    yield case("reflow", [write(b"abcdefghijklmnopqrstuv\r\nlast"), {"op": "resize", "cols": 7, "rows": 3}])

    # Every possible cut includes both fresh and partly populated parser state.
    continuations = {
        "utf8": "😄".encode(),
        "escape": b"\x1b(0q",
        "csi": b"\x1b[38:2::10:20:30mX",
        "osc": b"\x1b]2;restored title\x1b\\",
        "dcs": b"\x1bP$qm\x1b\\",
        "apc": b"\x1b_Ga=q,i=7,s=1,v=1,f=24;AAAA\x1b\\",
        "committed-control": b"\x1b[3\x071mX",
    }
    for name, data in continuations.items():
        for cut in range(1, len(data)):
            yield case(f"continuation/{name}/{cut}", b"A" + data[:cut], data[cut:] + b"Z")


def compare(peers, request, difference):
    """Wire page grouping may differ; preserve both encodings in artifacts."""
    def semantic(response):
        return {k: v for k, v in response.items() if k not in ("id", "capabilities", "snapshots")}

    args = {k: v for k, v in request.items() if k not in ("kind", "after")}
    args["kind"] = "terminal"
    prefix = request.get("operations", [])
    suffix = [{"op": "observe"}] + request.get("after", [])
    encoded = [peer.request(dict(args, operations=prefix + [{"op": "snapshot"}])) for peer in peers]
    results = [{"id": request["id"], "ok": response["ok"], "err": response["err"],
                "source": semantic(response)} for response in encoded]
    if not all(response["ok"] for response in encoded):
        return *results, difference(*results) or "snapshot encoding failed"
    if any(len(response["snapshots"]) != 1 for response in encoded):
        raise RuntimeError("snapshot encoder did not return exactly one payload")
    payloads = dict(zip(("zig", "rust"), (response["snapshots"][0] for response in encoded)))
    own_difference = None
    for peer, result in zip(peers, results):
        result["encodings"] = payloads
        live = peer.request(dict(args, operations=prefix + [{"op": "checkpoint"}] + suffix))
        result["live"] = semantic(live)
        result["restored"] = {}
        for source, data in payloads.items():
            restored = peer.request(dict(args, operations=[{"op": "restore", "data": data}] + suffix))
            value = semantic(restored)
            result["restored"][source] = value
            result["ok"] &= live["ok"] and restored["ok"]
            if own_difference is None:
                own_difference = difference(result["live"], value, f"{peer.name}.restore-{source}")
    compared = [{k: v for k, v in result.items() if k != "encodings"} for result in results]
    failure = None if all(result["ok"] for result in results) else "snapshot phase failed"
    return *results, difference(*compared) or own_difference or failure
