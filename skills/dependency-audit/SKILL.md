---
name: dependency-audit
description: Scan dependency lockfiles for known-vulnerable packages across Rust, Node, Ruby, Go, and Python ecosystems.
version: 0.1.0
runtime: none
permissions:
  filesystem:
    read: ["."]
  network:
    default: deny
  shell:
    default: deny
---

# Dependency Audit Skill

## Purpose

Scan dependency lockfiles (`Cargo.lock`, `package-lock.json`, `yarn.lock`, `Gemfile.lock`, `go.sum`, `requirements.txt`) for packages with known security issues.

## How It Works

Matches package names and versions against patterns defined in `patterns.toml`. Each pattern entry specifies an ecosystem, package name, affected versions, and severity.

## Output Format

1. Summary of lockfiles found
2. Vulnerable packages by severity
3. Recommended actions (upgrade, replace, or remove)
