# Open Integrity Inception Ceremony

Establishes a cryptographic root of trust for the AgentZero repository using
the [Open Integrity](https://github.com/OpenIntegrityProject/core) framework.

## Prerequisites

- Git 2.34+ (SSH signing support)
- `ssh-keygen` (standard OpenSSH)

## Step 1: Generate the inception key

```bash
ssh-keygen -t ed25519 -C "inception@agentzero" -f ~/.ssh/agentzero_inception
```

Record the fingerprint:
```bash
ssh-keygen -E sha256 -lf ~/.ssh/agentzero_inception.pub
```

## Step 2: Configure Git for SSH signing

```bash
git config --local gpg.format ssh
git config --local user.signingkey ~/.ssh/agentzero_inception
git config --local commit.gpgsign true
git config --local gpg.ssh.allowedSignersFile .repo/config/verification/allowed_commit_signers
```

## Step 3: Create the inception commit

```bash
GIT_COMMITTER_NAME="$(ssh-keygen -E sha256 -lf ~/.ssh/agentzero_inception.pub | awk '{print $2}')" \
git commit --allow-empty -S \
  -m "Initialize cryptographic root of trust" \
  -m "This signed empty commit serves as the inception anchor for all future
commit verification. Only keys listed in
.repo/config/verification/allowed_commit_signers may authorize commits
after the trust transition." \
  --signoff
```

## Step 4: Trust transition

Add your operational signing key to `allowed_commit_signers`:

```bash
# Generate operational key (if not already using one)
ssh-keygen -t ed25519 -C "ari@agentzero.dev" -f ~/.ssh/agentzero_signing

# Add to allowed signers
echo "ari@agentzero.dev $(cat ~/.ssh/agentzero_signing.pub)" \
  >> .repo/config/verification/allowed_commit_signers

# Commit the transition (still signed by inception key)
git add .repo/config/verification/allowed_commit_signers
git commit -S -m "Trust transition: establish operational signing keys" \
  -m "Inception key retired. Delegated keys now govern this repository."

# Switch to operational key
git config --local user.signingkey ~/.ssh/agentzero_signing
```

## Step 5: Verify

```bash
git log --show-signature -2
git verify-commit HEAD
```

## Step 6: Enable signature verification hook

The `verify-signatures` hook is installed at `.githooks/verify-signatures`.
To enable it as a pre-push hook:

```bash
ln -sf ../../.githooks/verify-signatures .git/hooks/pre-push
```

Set `AGENTZERO_STRICT_SIGNING=1` to enforce (block unsigned pushes).

## Ceremony record

| Field           | Value |
|-----------------|-------|
| Date            | _TBD (fill during ceremony)_ |
| Inception hash  | _TBD_ |
| Key fingerprint | _TBD_ |
| Framework       | Open Integrity (Blockchain Commons) |
