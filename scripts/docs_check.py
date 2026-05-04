#!/usr/bin/env python3
from pathlib import Path
import sys

required = [
    "README.md",
    "AGENTS.md",
    "Justfile",
    "specs/project.md",
    "specs/security-model.md",
    "specs/SPRINT.md",
    "specs/adrs/README.md",
    "specs/plans/0001-bootstrap-agentzero.md",
    "specs/prompts/0001-bootstrap-rust-workspace.md",
]

missing = [p for p in required if not Path(p).exists()]
if missing:
    print("Missing required files:")
    for p in missing:
        print(f"- {p}")
    sys.exit(1)

print("docs-check passed")
