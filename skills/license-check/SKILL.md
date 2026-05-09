---
name: license-check
description: Check project dependencies for license compliance using cargo metadata or package.json.
version: 0.1.0
runtime: host_supervised
entrypoint: run.sh
permissions:
  filesystem:
    read: ["."]
  shell:
    default: require_approval
- shell
- read
---

# License Check Skill

## Purpose

Scan project dependencies for license compliance. Identifies copyleft licenses (GPL, AGPL), unknown licenses, and packages with no declared license.

## How It Works

For Rust projects, runs `cargo metadata --format-version 1` to extract dependency license information. For Node projects, reads `package.json` and `node_modules/*/package.json`.

Requires host-supervised runtime since it executes shell commands.

## Output Format

1. Summary of dependencies checked
2. License distribution (MIT, Apache-2.0, etc.)
3. Flagged licenses (copyleft, unknown, missing)
4. Recommended actions
