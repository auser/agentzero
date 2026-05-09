---
name: secrets-scan
description: Deep-scan source code for hardcoded secrets using pattern matching and Shannon entropy analysis.
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

# Secrets Scan Skill

## Purpose

Deep-scan source files for hardcoded secrets, API keys, tokens, and high-entropy strings that may indicate embedded credentials. Extends beyond the repo-security-audit patterns with entropy-based detection.

## How It Works

1. Pattern matching against known secret formats (AWS keys, Stripe keys, JWT tokens, etc.)
2. Shannon entropy analysis: flags strings >20 chars with >4.5 bits/char entropy
3. Context-aware: skips comments, test fixtures, and known false-positive patterns

## Output Format

1. Summary of files scanned
2. Secrets found by category and severity
3. High-entropy strings flagged for review
4. Recommended actions
