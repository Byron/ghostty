"""Known literal-search differences across native storage-page boundaries."""


def requests():
    # Native allocation varies with column count. These dimensions cross page
    # boundaries both within the active area and between active/history pages.
    for cols, rows, count in ((80, 4, 1024), (80, 768, 768),
                              (215, 4, 512), (215, 256, 256),
                              (1024, 4, 256), (1024, 64, 64)):
        patterns = [
            ("hard", b"X\r\n" * (count - 1) + b"X", [b"X", b"X\nX", b"\n"]),
            ("blanks", b"X\r\n" + b"\r\n" * (count - 2) + b"X", [b"X", b"\n", b"\n\n"]),
            ("soft", (b"." * (cols - 1) + b"X") * count, [b"X.", b".X."]),
        ]
        for name, text, needles in patterns:
            request = {
                "id": f"grid/search/pages/{name}/{cols}/{rows}/{count}",
                "kind": "input", "cols": cols, "rows": rows,
                "operations": [{"op": "write", "data": text.hex()}] + [
                    {"op": "grid", "grid": {"action": "search", "needle": needle.hex()}}
                    for needle in needles
                ],
            }
            yield request, ["terminal.search"]


if __name__ == "__main__":
    import json
    import sys

    json.dump([{"request": request, "covers": covers} for request, covers in requests()],
              sys.stdout, indent=2)
    sys.stdout.write("\n")
