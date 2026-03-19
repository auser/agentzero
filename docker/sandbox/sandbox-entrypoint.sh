#!/usr/bin/env bash
set -euo pipefail

POLICY_FILE=""

# Look for security-policy.yaml in workspace first, then /data
if [ -f /workspace/.agentzero/security-policy.yaml ]; then
    POLICY_FILE="/workspace/.agentzero/security-policy.yaml"
elif [ -f /data/security-policy.yaml ]; then
    POLICY_FILE="/data/security-policy.yaml"
fi

if [ -n "$POLICY_FILE" ]; then
    echo "[sandbox] Found security policy: $POLICY_FILE"

    # Generate iptables rules from the YAML policy
    RULES_FILE=$(mktemp /tmp/iptables-rules.XXXXXX)
    python3 /usr/local/bin/policy-to-iptables.py "$POLICY_FILE" > "$RULES_FILE"

    echo "[sandbox] Applying iptables rules..."
    # Apply each rule (must run as root before dropping privileges)
    while IFS= read -r rule; do
        [ -z "$rule" ] && continue
        [[ "$rule" == \#* ]] && continue
        eval "iptables $rule" || echo "[sandbox] Warning: failed to apply rule: $rule"
    done < "$RULES_FILE"

    rm -f "$RULES_FILE"
    echo "[sandbox] Network policy applied."
else
    echo "[sandbox] No security-policy.yaml found; running with default network access."
fi

# Drop to non-root user and exec the gateway binary
echo "[sandbox] Starting agentzero as non-root user..."
exec su -s /bin/sh agentzero -c "exec agentzero $*"
