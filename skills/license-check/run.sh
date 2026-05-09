#!/bin/bash
# License Check Skill — host-supervised entrypoint
# Extracts dependency license info and checks for compliance issues.

set -euo pipefail

FLAGGED_LICENSES=("GPL-2.0" "GPL-3.0" "AGPL-3.0" "LGPL-2.0" "LGPL-2.1" "LGPL-3.0" "SSPL-1.0" "EUPL-1.2" "CC-BY-SA" "CC-BY-NC")

echo "=== License Check ==="
echo ""

# Rust project
if [ -f "Cargo.toml" ]; then
    echo "Detected: Rust project (Cargo.toml)"

    if ! command -v cargo &> /dev/null; then
        echo "error: cargo not found — install Rust to use license-check on this project"
        exit 1
    fi

    echo "Running: cargo metadata --format-version 1"
    METADATA=$(cargo metadata --format-version 1 --no-deps 2>/dev/null || true)

    if [ -z "$METADATA" ]; then
        echo "warning: cargo metadata returned no output"
        exit 1
    fi

    echo ""
    echo "Dependencies and licenses:"
    echo "$METADATA" | python3 -c "
import json, sys
data = json.load(sys.stdin)
packages = data.get('packages', [])
flagged = set(${FLAGGED_LICENSES[@]/#/\"} )
missing = []
flagged_pkgs = []
for pkg in packages:
    license = pkg.get('license', 'UNKNOWN')
    if license is None:
        license = 'UNKNOWN'
    name = pkg.get('name', '?')
    version = pkg.get('version', '?')
    print(f'  {name} {version}: {license}')
    if license == 'UNKNOWN':
        missing.append(f'{name} {version}')
    for flag in ['GPL', 'AGPL', 'LGPL', 'SSPL', 'EUPL']:
        if flag in license.upper():
            flagged_pkgs.append(f'{name} {version}: {license}')
            break
print()
if flagged_pkgs:
    print('FLAGGED (copyleft/restrictive):')
    for p in flagged_pkgs:
        print(f'  !! {p}')
    print()
if missing:
    print('MISSING LICENSE:')
    for p in missing:
        print(f'  ?? {p}')
    print()
if not flagged_pkgs and not missing:
    print('All licenses look clean.')
" 2>/dev/null || echo "warning: python3 not available for JSON parsing"

    exit 0
fi

# Node project
if [ -f "package.json" ]; then
    echo "Detected: Node project (package.json)"

    if [ -d "node_modules" ]; then
        echo "Scanning node_modules for license fields..."
        echo ""
        find node_modules -maxdepth 2 -name "package.json" -exec sh -c '
            name=$(cat "$1" 2>/dev/null | python3 -c "import json,sys; d=json.load(sys.stdin); print(d.get(\"name\",\"?\"))" 2>/dev/null || echo "?")
            license=$(cat "$1" 2>/dev/null | python3 -c "import json,sys; d=json.load(sys.stdin); print(d.get(\"license\",\"UNKNOWN\"))" 2>/dev/null || echo "UNKNOWN")
            echo "  $name: $license"
        ' _ {} \;
    else
        echo "warning: node_modules not found — run npm install first"
        exit 1
    fi

    exit 0
fi

echo "error: no supported project found (Cargo.toml or package.json)"
exit 1
