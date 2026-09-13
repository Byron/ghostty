#!/usr/bin/env python3
"""Compare independent Rust and Zig processes; never link the Zig oracle into Rustty."""
import argparse
import hashlib
import json
from pathlib import Path
import queue
import random
import re
import subprocess
import sys
import threading

ROOT = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent
ARTIFACTS = ROOT / "target/parity"
MAX_RESPONSE = 128 * 1024 * 1024


class Peer:
    def __init__(self, name, command, timeout):
        self.name = name
        self.timeout = timeout
        self.queue = queue.Queue()
        self.log = (ARTIFACTS / f"{name}.log").open("wb")
        self.process = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                        stderr=self.log, cwd=ROOT)
        threading.Thread(target=self._read, daemon=True).start()

    def _read(self):
        try:
            while line := self.process.stdout.readline(MAX_RESPONSE + 1):
                if len(line) > MAX_RESPONSE or not line.endswith(b"\n"):
                    raise RuntimeError("response exceeded limit or was truncated")
                self.queue.put(json.loads(line))
            raise RuntimeError(f"oracle exited; inspect {self.name}.log")
        except Exception as error:
            self.queue.put(error)

    def request(self, request):
        self.process.stdin.write(json.dumps(request, separators=(",", ":")).encode() + b"\n")
        self.process.stdin.flush()
        try:
            response = self.queue.get(timeout=self.timeout)
        except queue.Empty as error:
            self.close()
            raise RuntimeError(f"{self.name}: timeout after {self.timeout}s") from error
        if isinstance(response, Exception):
            self.close()
            raise RuntimeError(f"{self.name}: {response}") from response
        return response

    def close(self):
        if self.process.poll() is None:
            self.process.terminate()
            try:
                self.process.wait(timeout=2)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait()
        self.log.close()


def difference(left, right, path="response"):
    if type(left) is not type(right):
        return f"{path}: Zig {left!r}, Rust {right!r}"
    if isinstance(left, dict):
        if left.keys() != right.keys():
            return f"{path}: different fields: {left.keys() ^ right.keys()}"
        for key in left:
            if diff := difference(left[key], right[key], f"{path}.{key}"):
                return diff
    elif isinstance(left, list):
        if len(left) != len(right):
            return f"{path}: Zig length {len(left)}, Rust length {len(right)}"
        for index, (a, b) in enumerate(zip(left, right)):
            if diff := difference(a, b, f"{path}[{index}]"):
                return diff
    elif left != right:
        return f"{path}: Zig {left!r}, Rust {right!r}"
    return None


def variants(request, exhaustive=False):
    """Delivery boundaries change; operation/observation ordering never changes."""
    yield request
    if request.get("kind", "terminal") != "terminal":
        return
    for name, chunks in (("scalar", [1]), ("chunks", [2, 7, 1, 13, 4])):
        operations = []
        for operation in request.get("operations", []):
            if operation["op"] != "write":
                operations.append(operation)
                continue
            data = bytes.fromhex(operation["data"])
            pos = 0
            index = 0
            while pos < len(data):
                size = chunks[index % len(chunks)]
                operations.append({"op": "write", "data": data[pos:pos + size].hex()})
                pos += size
                index += 1
        yield dict(request, id=f"{request['id']}/{name}", scalar=name == "scalar", operations=operations)
    if exhaustive:
        for index, operation in enumerate(request.get("operations", [])):
            if operation["op"] != "write":
                continue
            data = bytes.fromhex(operation["data"])
            if len(data) > 64:
                continue
            for split in range(1, len(data)):
                operations = request["operations"][:index] + [
                    {"op": "write", "data": data[:split].hex()},
                    {"op": "write", "data": data[split:].hex()},
                ] + request["operations"][index + 1:]
                yield dict(request, id=f"{request['id']}/split-{index}-{split}", operations=operations)


def corpus_requests():
    # stream.zig fuzz fixtures reserve byte zero for scalar/vector delivery.
    # The terminal bytes begin at offset one, in both reference paths.
    for directory in ("stream-initial", "stream-cmin"):
        paths = sorted((ROOT / "test/fuzz-libghostty/corpus" / directory).glob("*"))
        if not paths:
            raise RuntimeError(f"required corpus is missing: {directory}")
        for path in paths:
            if path.is_file():
                yield {"id": f"corpus/{directory}/{path.name}", "cols": 80, "rows": 24,
                       "operations": [{"op": "write", "data": path.read_bytes()[1:].hex()}]}


def generated_requests(seed, count):
    rng = random.Random(seed)
    writes = ["hello", "é界👩🏽‍💻", "\r", "\n", "\t", "\b", "\x1b[2J", "\x1b[K",
              "\x1b[1;1H", "\x1b[31m", "\x1b[0m", "\x1b[?1049h", "\x1b[?1049l",
              "\x1b[2;3r", "\x1b[r", "\x1b[4h", "\x1b[4l", "\x1b[2P", "\x1b[2@"]
    for case in range(count):
        operations = []
        for _ in range(30):
            if rng.randrange(8) == 0:
                operations.append({"op": "resize", "cols": rng.randint(3, 30), "rows": rng.randint(2, 8)})
            else:
                operations.append({"op": "write", "data": rng.choice(writes).encode().hex()})
            if rng.randrange(4) == 0:
                operations.append({"op": "observe"})
        yield {"id": f"generated/{seed}/{case}", "cols": 12, "rows": 4, "operations": operations}


def save_failure(request, left, right, reason):
    digest = hashlib.sha256(json.dumps(request, sort_keys=True).encode()).hexdigest()[:16]
    directory = ARTIFACTS / "failures" / digest
    directory.mkdir(parents=True, exist_ok=True)
    for name, value in (("request", request), ("zig", left), ("rust", right)):
        (directory / f"{name}.json").write_text(json.dumps(value, indent=2) + "\n")
    (directory / "difference.txt").write_text(reason + "\n")
    return directory.relative_to(ROOT)


def compare(peers, request):
    left, right = [peer.request(request) for peer in peers]
    # Capabilities are checked independently of observed behavior.
    comparable = [{k: v for k, v in response.items() if k != "capabilities"} for response in (left, right)]
    reason = difference(*comparable)
    if not left["ok"] or not right["ok"]:
        reason = reason or f"request failed: {left.get('err')} / {right.get('err')}"
    return left, right, reason


def coverage_gaps(manifest, capabilities, covered):
    missing = []
    for entry in manifest["requirements"]:
        if entry["status"] != "complete":
            missing.append(f"{entry['id']}: {entry['status']} — {entry['remaining']}")
        elif not all(entry["id"] in peer for peer in capabilities):
            missing.append(f"{entry['id']}: an oracle lacks this capability")
        elif entry["id"] not in covered:
            missing.append(f"{entry['id']}: no passing coverage case in this run")
    return missing


def minimize(peers, request):
    """Delta-debug operations, then bytes, retaining a successful state mismatch."""
    request = dict(request, id=request["id"] + "/minimized")
    attempts = 0
    original = compare(peers, request)[2]

    def signature(reason):
        return re.sub(r"\[\d+\]", "[]", reason.split(":", 1)[0]) if reason else None

    def still_fails(candidate):
        nonlocal attempts
        if attempts >= 2000:
            return False
        attempts += 1
        left, right, reason = compare(peers, candidate)
        return left["ok"] and right["ok"] and signature(reason) == signature(original)

    def reduce_sequence(sequence, candidate):
        partitions = 2
        while sequence and attempts < 2000:
            size = max(1, (len(sequence) + partitions - 1) // partitions)
            for start in range(0, len(sequence), size):
                reduced = sequence[:start] + sequence[start + size:]
                if still_fails(candidate(reduced)):
                    sequence = reduced
                    partitions = max(2, partitions - 1)
                    break
            else:
                if partitions >= len(sequence):
                    break
                partitions = min(len(sequence), partitions * 2)
                continue
        return sequence

    request["operations"] = reduce_sequence(request.get("operations", []),
                                             lambda ops: dict(request, operations=ops))
    for index, operation in enumerate(request["operations"]):
        if operation["op"] != "write":
            continue

        def replace(data):
            operations = request["operations"].copy()
            operations[index] = {"op": "write", "data": data.hex()}
            return dict(request, operations=operations)

        data = reduce_sequence(bytes.fromhex(operation["data"]), replace)
        request = replace(data)
    left, right, reason = compare(peers, request)
    return request, left, right, reason, attempts


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--thorough", action="store_true", help="run extended cases and require complete coverage")
    parser.add_argument("--no-build", action="store_true")
    parser.add_argument("--case", help="only fixture IDs containing this text")
    parser.add_argument("--replay", type=Path, help="replay a saved request")
    parser.add_argument("--minimize", action="store_true", help="reduce a replayed state mismatch")
    parser.add_argument("--timeout", type=float, default=15)
    parser.add_argument("--seed", type=int, default=0)
    parser.add_argument("--generated", type=int, default=0)
    parser.add_argument("--max-failures", type=int, default=20)
    args = parser.parse_args()
    if args.minimize and not args.replay:
        parser.error("--minimize requires --replay")
    ARTIFACTS.mkdir(parents=True, exist_ok=True)
    if not args.no_build:
        subprocess.run(["zig", "build", "vt-oracle", "-Demit-lib-vt=true", "-Demit-macos-app=false"], cwd=ROOT, check=True)
        subprocess.run(["cargo", "build", "--offline", "-p", "rustty-vt", "--example", "parity"], cwd=ROOT, check=True)
    peers = []
    failures = 0
    checked = 0
    aborted = False
    missing = []
    covered = set()
    try:
        for name, binary in (("zig", ROOT / "zig-out/bin/vt-oracle"), ("rust", ROOT / "target/debug/examples/parity")):
            peers.append(Peer(name, [str(binary)], args.timeout))
        capabilities = [set(peer.request({"kind": "capabilities"})["capabilities"]) for peer in peers]
        manifest = json.loads((HERE / "coverage.json").read_text())
        fixtures = json.loads((HERE / "smoke.json").read_text())
        requests = []
        if args.replay:
            requests.append((json.loads(args.replay.read_text()), []))
        else:
            for fixture in fixtures:
                if not args.case or args.case in fixture["request"]["id"]:
                    requests.append((fixture["request"], fixture["covers"]))
            if args.thorough:
                requests.extend((request, []) for request in corpus_requests())
            requests.extend((request, []) for request in generated_requests(args.seed, args.generated or (100 if args.thorough else 0)))
        for base, covers in requests:
            success = True
            baseline = None
            for request in ([base] if args.replay else variants(base, exhaustive=args.thorough)):
                left = right = None
                try:
                    left, right, reason = compare(peers, request)
                    normalized = [{k: v for k, v in value.items() if k not in ("id", "capabilities")}
                                  for value in (left, right)]
                    if baseline is None:
                        baseline = normalized
                    elif not reason:
                        for name, original, variant in zip(("zig", "rust"), baseline, normalized):
                            if diff := difference(original, variant, name + ".delivery"):
                                reason = diff
                                break
                except Exception as error:
                    reason = str(error)
                    aborted = True
                checked += 1
                if reason:
                    success = False
                    failures += 1
                    path = save_failure(request, left, right, reason)
                    print(f"FAIL {request['id']}: {reason}\n  {path}", flush=True)
                    if args.minimize and left and right and left["ok"] and right["ok"]:
                        reduced, lval, rval, diff, attempts = minimize(peers, request)
                        reduced_path = save_failure(reduced, lval, rval, diff)
                        print(f"  Minimized in {attempts} comparisons: {reduced_path}", flush=True)
                    if aborted or failures >= args.max_failures:
                        break
            if success:
                covered.update(covers)
            if aborted or failures >= args.max_failures:
                break
        if args.thorough:
            missing = coverage_gaps(manifest, capabilities, covered)
            for gap in missing:
                print(f"COVERAGE GAP {gap}")
    finally:
        for peer in peers:
            peer.close()
    summary = {"mode": "thorough" if args.thorough else "smoke", "checked": checked,
               "failures": failures, "coverage_gaps": missing, "covered": sorted(covered),
               "full_parity": args.thorough and not failures and not missing}
    (ARTIFACTS / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
    print(f"{checked} comparisons, {failures} failures, {len(missing)} coverage gaps; "
          + ("full parity verified" if summary["full_parity"] else "full parity not established"))
    return 1 if failures or missing or not checked else 0


if __name__ == "__main__":
    sys.exit(main())
