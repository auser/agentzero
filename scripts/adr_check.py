#!/usr/bin/env python3
from pathlib import Path
import re
import sys

adr_dir = Path("specs/adrs")
adrs = sorted(p for p in adr_dir.glob("*.md") if p.name != "README.md")
if not adrs:
    print("No ADRs found")
    sys.exit(1)

bad = []
for adr in adrs:
    if not re.match(r"^\d{4}-[a-z0-9-]+\.md$", adr.name):
        bad.append(f"Bad ADR filename: {adr}")
    text = adr.read_text()
    for heading in ["# ADR", "## Status", "## Context", "## Decision", "## Consequences"]:
        if heading not in text:
            bad.append(f"{adr}: missing {heading}")

if bad:
    print("ADR check failed:")
    for item in bad:
        print(f"- {item}")
    sys.exit(1)

print(f"adr-check passed ({len(adrs)} ADRs)")
