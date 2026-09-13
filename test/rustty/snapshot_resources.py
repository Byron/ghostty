"""Native PAGE resource exhaustion and grapheme continuation boundaries."""
import struct

import snapshots


def requests(reference):
    encoded = reference.request({"id": "snapshot/resources/source", "cols": 80, "rows": 1,
                                 "operations": [{"op": "snapshot"}]})
    if not encoded["ok"] or len(encoded["snapshots"]) != 1:
        raise RuntimeError("reference could not encode resource fixture")
    template = snapshots.records(bytes.fromhex(encoded["snapshots"][0]))

    def case(name, capacity, entries):
        # Eight-bit bare codepoints, one active row, no styles or hyperlinks.
        page = bytearray(struct.pack("<HHHHHHII", 80, 1, 0, 0, 0, 0, capacity, 0))
        page.extend(struct.pack("<BH", 0, 80) + b"A" * 80)
        page.extend(struct.pack("<I", len(entries)))
        for row, col, suffix in entries:
            page.extend(struct.pack("<HHH", row, col, len(suffix)))
            page.extend(struct.pack(f"<{len(suffix)}I", *suffix))
        parts = [(tag, page if tag == 3 else data) for tag, data in template]
        return ({"id": "snapshot/resources/graphemes/" + name, "operations": [
            {"op": "restore", "data": snapshots.frame(parts).hex()},
            {"op": "observe"},
        ]}, ["snapshot.cross-decode", "terminal.cells"])

    for capacity in (0, 1, 16, 17, 32, 48, 64, 128, 256, 512, 1024, 2048, 8192):
        for length in (1, 4, 5, 8, 9, 16, 17, 32, 33, 64, 65, 128):
            entries = [(0, col, [0x300 + i for i in range(length)]) for col in range(4)]
            yield case(f"capacity/{capacity}/{length}", capacity, entries)
        yield case(f"map/{capacity}", capacity, [(0, col, [0x301]) for col in range(80)])
    yield case("mid-cluster", 512, [(0, col, list(range(0x300, 0x300 + count)))
                                    for col, count in enumerate((64, 64, 8, 61))])
    yield case("retry-failed", 512, [(0, col, [0x301] * count)
                                     for col, count in ((0, 64), (1, 64), (2, 8),
                                                        (3, 61), (3, 1), (3, 4))])
    yield case("invalid-and-duplicate", 64, [
        (1, 0, [0x301]), (0, 80, [0x301]),
        (0, 0, [0, 0xD800, 0x110000]), (0, 0, [0x301, 0, 0xD800, 0x302]),
        (0, 0, [0x303]), (0, 1, [0x301] * 65),
    ])
