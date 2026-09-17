#!/usr/bin/env python3
"""Verify that the expected version of each crate is live on crates.io.

Usage: verify_crates_publish.py --expected 0.1.0 lanok-core lanok ...

A green publish job is not proof: `cargo publish` returns once the upload is
accepted, and the version only becomes resolvable when the index catches up. A
release that reports success while half its crates are unresolvable is the
failure this guards against, so the check polls until every crate reports the
version or the timeout expires.

Reads the sparse index rather than the API: the index has no User-Agent policy
and is the thing cargo itself resolves against.
"""

import argparse
import json
import sys
import time
import urllib.error
import urllib.request


def index_path(crate: str) -> str:
    """Where the sparse index files a crate, which depends on its name length."""
    n = len(crate)
    if n == 1:
        return f"1/{crate}"
    if n == 2:
        return f"2/{crate}"
    if n == 3:
        return f"3/{crate[0]}/{crate}"
    return f"{crate[:2]}/{crate[2:4]}/{crate}"


def published_versions(crate: str) -> set[str]:
    url = f"https://index.crates.io/{index_path(crate)}"
    req = urllib.request.Request(url, headers={"User-Agent": "lanok-release-check"})
    try:
        with urllib.request.urlopen(req, timeout=20) as resp:
            body = resp.read().decode("utf-8")
    except urllib.error.HTTPError as e:
        # 404 means the crate has never been published, which is a legitimate
        # state to be waiting on during a first release.
        if e.code == 404:
            return set()
        raise
    return {json.loads(line)["vers"] for line in body.splitlines() if line.strip()}


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--expected", required=True)
    ap.add_argument("--timeout", type=int, default=600)
    ap.add_argument("crates", nargs="+")
    args = ap.parse_args()

    pending = set(args.crates)
    deadline = time.monotonic() + args.timeout

    while pending:
        for crate in sorted(pending):
            try:
                if args.expected in published_versions(crate):
                    print(f"ok    {crate} {args.expected}")
                    pending.discard(crate)
            except Exception as e:  # network blip: keep waiting, not failing
                print(f"warn  {crate}: {e}")
        if not pending:
            break
        if time.monotonic() >= deadline:
            for crate in sorted(pending):
                print(f"FAIL  {crate} is not serving {args.expected}")
            return 1
        time.sleep(10)

    print(f"all {len(args.crates)} crates serving {args.expected}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
