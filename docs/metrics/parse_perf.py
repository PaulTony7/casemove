#!/usr/bin/env python3
"""Parse PERF| lines from a legacy app run into baseline metrics JSON.

Usage:
  ELECTRON_ENABLE_LOGGING=1 npm start 2>&1 | tee run.log
  python3 docs/metrics/parse_perf.py run.log > docs/metrics/baseline-legacy.json

Emits one JSON object: S1/S2/S3/S5 durations (ms), S4 filter p50/p95 (ms),
S6 listener snapshots (leak detection: counts must be stable across logins).
"""
import json
import re
import sys
from collections import defaultdict

perf_re = re.compile(r"PERF\|([^\s|]+)\|([^\s|]+)\|([^\s|]+)(?:\|(.*))?")


def stats(values):
    if not values:
        return None
    values = sorted(values)
    n = len(values)

    def pct(p):
        return values[min(n - 1, int(round(p * (n - 1))))]

    return {
        "n": n,
        "min": values[0],
        "p50": pct(0.50),
        "p95": pct(0.95),
        "max": values[-1],
    }


def main():
    if len(sys.argv) < 2:
        print(__doc__, file=sys.stderr)
        sys.exit(1)
    durations = defaultdict(list)  # (scenario, name) -> [ms]
    extras = defaultdict(list)     # (scenario, name) -> [extra str]
    listeners = defaultdict(list)  # name -> [json str]

    with open(sys.argv[1], encoding="utf-8", errors="replace") as f:
        for line in f:
            m = perf_re.search(line)
            if not m:
                continue
            scenario, name, value, extra = m.groups()
            if scenario == "S6":
                listeners[name].append(extra or value)
                continue
            try:
                durations[(scenario, name)].append(float(value))
            except ValueError:
                continue
            if extra:
                extras[(scenario, name)].append(extra)

    out = {"scenarios": {}, "s6_listeners": {}}
    for (scenario, name), vals in sorted(durations.items()):
        out["scenarios"].setdefault(scenario, {})[name] = stats(vals)
        ex = extras.get((scenario, name))
        if ex:
            out["scenarios"][scenario][name]["samples"] = ex[-5:]

    for name, snaps in listeners.items():
        out["s6_listeners"][name] = snaps
        if name == "listeners_after_login" and len(snaps) > 1:
            out["s6_listener_leak"] = snaps[-1] != snaps[0]

    json.dump(out, sys.stdout, indent=2)
    print()


if __name__ == "__main__":
    main()
