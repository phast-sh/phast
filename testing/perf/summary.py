"""Summarize one or more benchmark runs as a single Markdown table.

Every run directory holds hyperfine's JSON per project and scenario. Given several, the
times each tool recorded for the same project and scenario are pooled, so the median is
taken over every run that went in, and the Runs column says how many that was.

Usage: python3 summary.py results/2026-10-05-1529 [results/2026-10-06-0910 ...]
"""

import json
import pathlib
import statistics
import sys
from collections import defaultdict

order = ["cold", "lock", "warm", "noop", "resolve"]
files = [path for run in sys.argv[1:] for path in pathlib.Path(run).glob("*.json")]

times = defaultdict(list)
for path in files:
    project, scenario = path.stem.rsplit("-", 1)
    for result in json.loads(path.read_text())["results"]:
        times[(project, scenario, result["command"])].extend(result["times"])

rows = []
for project, scenario in sorted({key[:2] for key in times}, key=lambda k: (k[0], order.index(k[1]))):
    composer = times[(project, scenario, "composer")]
    phast    = times[(project, scenario, "phast")]

    composer_median, phast_median = statistics.median(composer), statistics.median(phast)
    rows.append((
        project,
        scenario,
        f"{composer_median:.2f}s",
        f"{phast_median:.2f}s",
        f"{composer_median / phast_median:.1f}x",
        str(min(len(composer), len(phast))),
    ))

header = ("Project", "Scenario", "Composer", "Phast", "Faster", "Runs")
right  = (False, False, True, True, True, True)
widths = [max(len(cell) for cell in column) for column in zip(header, *rows)]
rule   = ["-" * w for w in widths]

for cells in [header, rule, *rows]:
    padded = (c.rjust(w) if r else c.ljust(w) for c, w, r in zip(cells, widths, right))
    print("| " + " | ".join(padded) + " |")
