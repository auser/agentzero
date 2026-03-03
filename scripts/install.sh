#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────────────────────
# AgentZero Installer
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/auser/agentzero/main/scripts/install.sh | bash
#   curl -fsSL ... | bash -s -- --version 0.2.0 --dir /usr/local/bin
#
# Zero external dependencies. Pure bash. Supports Linux, macOS, and WSL.
# ──────────────────────────────────────────────────────────────────────────────
set -euo pipefail

SCRIPT_VERSION="0.1.0"
REPO="auser/agentzero"
BINARY_NAME="agentzero"
GITHUB_API="https://api.github.com/repos/${REPO}/releases"
GITHUB_RELEASE="https://github.com/${REPO}/releases/download"

# ──────────────────────────────────────────────────────────────────────────────
# Defaults (overridden by flags)
# ──────────────────────────────────────────────────────────────────────────────
VERSION="latest"
INSTALL_DIR=""
CHANNEL="stable"
FORCE=0
QUIET=0
VERBOSE=0
DRY_RUN=0
NO_VERIFY=0
NO_COLOR=0
FROM_SOURCE=0
UNINSTALL=0
COMPLETIONS_SHELL=""
GITHUB_TOKEN="${GITHUB_TOKEN:-}"

# ──────────────────────────────────────────────────────────────────────────────
# Color system — honors NO_COLOR env, --no-color flag, and non-TTY stderr
# ──────────────────────────────────────────────────────────────────────────────
setup_colors() {
  if [[ -t 2 ]] && [[ "${NO_COLOR}" -eq 0 ]] && [[ -z "${NO_COLOR_ENV:-}" ]]; then
    BOLD='\033[1m'
    DIM='\033[2m'
    ITALIC='\033[3m'
    UNDERLINE='\033[4m'
    RED='\033[0;31m'
    GREEN='\033[0;32m'
    YELLOW='\033[0;33m'
    BLUE='\033[0;34m'
    MAGENTA='\033[0;35m'
    CYAN='\033[0;36m'
    WHITE='\033[0;37m'
    BOLD_RED='\033[1;31m'
    BOLD_GREEN='\033[1;32m'
    BOLD_YELLOW='\033[1;33m'
    BOLD_BLUE='\033[1;34m'
    BOLD_MAGENTA='\033[1;35m'
    BOLD_CYAN='\033[1;36m'
    NC='\033[0m'
  else
    BOLD='' DIM='' ITALIC='' UNDERLINE=''
    RED='' GREEN='' YELLOW='' BLUE='' MAGENTA='' CYAN='' WHITE=''
    BOLD_RED='' BOLD_GREEN='' BOLD_YELLOW='' BOLD_BLUE=''
    BOLD_MAGENTA='' BOLD_CYAN='' NC=''
  fi
}

# Capture NO_COLOR env before flag parsing overwrites it
NO_COLOR_ENV="${NO_COLOR:-}"

# ──────────────────────────────────────────────────────────────────────────────
# Messaging helpers — all output goes to stderr
# ──────────────────────────────────────────────────────────────────────────────
info() {
  [[ "${QUIET}" -eq 1 ]] && return
  printf "${CYAN}  ▸${NC} %s\n" "$*" >&2
}

success() {
  [[ "${QUIET}" -eq 1 ]] && return
  printf "${GREEN}  ✓${NC} %s\n" "$*" >&2
}

warn() {
  printf "${YELLOW}  ⚠${NC} %s\n" "$*" >&2
}

error() {
  printf "${BOLD_RED}  ✗ Error:${NC} %s\n" "$*" >&2
  exit 1
}

error_noexit() {
  printf "${BOLD_RED}  ✗ Error:${NC} %s\n" "$*" >&2
}

debug() {
  [[ "${VERBOSE}" -eq 0 ]] && return
  printf "${DIM}  … %s${NC}\n" "$*" >&2
}

step() {
  [[ "${QUIET}" -eq 1 ]] && return
  local num="$1"; shift
  printf "\n${BOLD}${BLUE}  [%s]${NC} ${BOLD}%s${NC}\n" "$num" "$*" >&2
}

# ──────────────────────────────────────────────────────────────────────────────
# Banner
# ──────────────────────────────────────────────────────────────────────────────
show_banner() {
  [[ "${QUIET}" -eq 1 ]] && return
  cat >&2 <<EOF

${BOLD_CYAN}   █████╗  ██████╗ ███████╗███╗   ██╗████████╗${BOLD_BLUE}███████╗███████╗██████╗  ██████╗${NC}
${BOLD_CYAN}  ██╔══██╗██╔════╝ ██╔════╝████╗  ██║╚══██╔══╝${BOLD_BLUE}╚══███╔╝██╔════╝██╔══██╗██╔═══██╗${NC}
${BOLD_CYAN}  ███████║██║  ███╗█████╗  ██╔██╗ ██║   ██║   ${BOLD_BLUE}  ███╔╝ █████╗  ██████╔╝██║   ██║${NC}
${BOLD_CYAN}  ██╔══██║██║   ██║██╔══╝  ██║╚██╗██║   ██║   ${BOLD_BLUE} ███╔╝  ██╔══╝  ██╔══██╗██║   ██║${NC}
${BOLD_CYAN}  ██║  ██║╚██████╔╝███████╗██║ ╚████║   ██║   ${BOLD_BLUE}███████╗███████╗██║  ██║╚██████╔╝${NC}
${BOLD_CYAN}  ╚═╝  ╚═╝ ╚═════╝ ╚══════╝╚═╝  ╚═══╝   ╚═╝   ${BOLD_BLUE}╚══════╝╚══════╝╚═╝  ╚═╝ ╚═════╝${NC}

${DIM}  Lightweight, Rust-first agent runtime${NC}
${DIM}  Installer v${SCRIPT_VERSION}${NC}

EOF
}

# ──────────────────────────────────────────────────────────────────────────────
# Usage / Help
# ──────────────────────────────────────────────────────────────────────────────
usage() {
  cat >&2 <<EOF
${BOLD}AgentZero Installer${NC} ${DIM}v${SCRIPT_VERSION}${NC}

${BOLD}USAGE${NC}
    install.sh [options]
    install.sh --uninstall [options]
    curl -fsSL .../install.sh | bash -s -- [options]

${BOLD}OPTIONS${NC}
    ${GREEN}-v${NC}, ${GREEN}--version${NC} ${UNDERLINE}VERSION${NC}     Install specific version ${DIM}[default: latest]${NC}
    ${GREEN}-d${NC}, ${GREEN}--dir${NC} ${UNDERLINE}DIR${NC}             Install directory ${DIM}[default: ~/.local/bin]${NC}
    ${GREEN}-c${NC}, ${GREEN}--channel${NC} ${UNDERLINE}CHANNEL${NC}     Release channel: stable, nightly ${DIM}[default: stable]${NC}
    ${GREEN}-f${NC}, ${GREEN}--force${NC}                Force reinstall even if already installed
    ${GREEN}-q${NC}, ${GREEN}--quiet${NC}                Suppress non-essential output
    ${GREEN}-V${NC}, ${GREEN}--verbose${NC}              Enable debug output
    ${GREEN}-n${NC}, ${GREEN}--dry-run${NC}              Show what would happen without doing it
        ${GREEN}--no-color${NC}             Disable colored output
        ${GREEN}--no-verify${NC}            Skip SHA-256 checksum verification
        ${GREEN}--completions${NC} ${UNDERLINE}SHELL${NC}    Install shell completions (bash, zsh, fish)
        ${GREEN}--from-source${NC}          Build from source instead of downloading binary
        ${GREEN}--uninstall${NC}            Remove agentzero and its data
        ${GREEN}--token${NC} ${UNDERLINE}TOKEN${NC}          GitHub API token (avoids rate limits in CI/Docker)
    ${GREEN}-h${NC}, ${GREEN}--help${NC}                 Show this help message

${BOLD}ENVIRONMENT${NC}
    ${CYAN}AGENTZERO_INSTALL_DIR${NC}    Override install directory
    ${CYAN}AGENTZERO_VERSION${NC}        Override version to install
    ${CYAN}GITHUB_TOKEN${NC}             GitHub API token (avoids rate limits in CI/Docker)
    ${CYAN}NO_COLOR${NC}                 Disable colored output (standard)

${BOLD}EXAMPLES${NC}
    ${DIM}# Install latest version${NC}
    curl -fsSL https://raw.githubusercontent.com/${REPO}/main/scripts/install.sh | bash

    ${DIM}# Install specific version to custom directory${NC}
    curl -fsSL https://raw.githubusercontent.com/${REPO}/main/scripts/install.sh | bash -s -- -v 0.2.0 -d /usr/local/bin

    ${DIM}# Install with shell completions${NC}
    curl -fsSL https://raw.githubusercontent.com/${REPO}/main/scripts/install.sh | bash -s -- --completions zsh

    ${DIM}# Build from source${NC}
    curl -fsSL https://raw.githubusercontent.com/${REPO}/main/scripts/install.sh | bash -s -- --from-source

    ${DIM}# Dry run (see what would happen)${NC}
    curl -fsSL https://raw.githubusercontent.com/${REPO}/main/scripts/install.sh | bash -s -- --dry-run --verbose

    ${DIM}# Uninstall${NC}
    curl -fsSL https://raw.githubusercontent.com/${REPO}/main/scripts/install.sh | bash -s -- --uninstall
EOF
}

# ──────────────────────────────────────────────────────────────────────────────
# Argument parsing — supports short, long, combined short flags, and = syntax
# ──────────────────────────────────────────────────────────────────────────────
require_arg() {
  local flag="$1"
  local next="${2:-}"
  if [[ -z "$next" ]] || [[ "$next" == -* ]]; then
    error "Option '${flag}' requires an argument. See --help."
  fi
}

parse_args() {
  while [[ $# -gt 0 ]]; do
    case "$1" in
      -h|--help)
        usage
        exit 0
        ;;
      -v|--version)
        require_arg "$1" "${2:-}"
        VERSION="$2"
        shift 2
        ;;
      --version=*)
        VERSION="${1#*=}"
        shift
        ;;
      -d|--dir)
        require_arg "$1" "${2:-}"
        INSTALL_DIR="$2"
        shift 2
        ;;
      --dir=*)
        INSTALL_DIR="${1#*=}"
        shift
        ;;
      -c|--channel)
        require_arg "$1" "${2:-}"
        CHANNEL="$2"
        shift 2
        ;;
      --channel=*)
        CHANNEL="${1#*=}"
        shift
        ;;
      --completions)
        require_arg "$1" "${2:-}"
        COMPLETIONS_SHELL="$2"
        shift 2
        ;;
      --completions=*)
        COMPLETIONS_SHELL="${1#*=}"
        shift
        ;;
      -f|--force)     FORCE=1;       shift ;;
      -q|--quiet)     QUIET=1;       shift ;;
      -V|--verbose)   VERBOSE=1;     shift ;;
      -n|--dry-run)   DRY_RUN=1;     shift ;;
      --no-color)     NO_COLOR=1;    shift ;;
      --no-verify)    NO_VERIFY=1;   shift ;;
      --from-source)  FROM_SOURCE=1; shift ;;
      --uninstall)    UNINSTALL=1;   shift ;;
      --token|--github-token)
        GITHUB_TOKEN="$2"; shift 2 ;;
      # Combined short flags: -fqV, -fnV, etc.
      -[fqVn]*)
        local flags="${1#-}"
        shift
        local i
        for (( i=0; i<${#flags}; i++ )); do
          local flag="${flags:$i:1}"
          case "$flag" in
            f) FORCE=1 ;;
            q) QUIET=1 ;;
            V) VERBOSE=1 ;;
            n) DRY_RUN=1 ;;
            *) error "Unknown flag '-${flag}' in combined flags. See --help." ;;
          esac
        done
        ;;
      -*)
        error "Unknown option: $1. See --help."
        ;;
      *)
        error "Unexpected argument: $1. See --help."
        ;;
    esac
  done

  # Apply environment variable overrides (flags take precedence)
  if [[ "$VERSION" == "latest" ]] && [[ -n "${AGENTZERO_VERSION:-}" ]]; then
    VERSION="${AGENTZERO_VERSION}"
  fi
  if [[ -z "$INSTALL_DIR" ]] && [[ -n "${AGENTZERO_INSTALL_DIR:-}" ]]; then
    INSTALL_DIR="${AGENTZERO_INSTALL_DIR}"
  fi

  # Validate completions shell
  if [[ -n "$COMPLETIONS_SHELL" ]]; then
    case "$COMPLETIONS_SHELL" in
      bash|zsh|fish) ;;
      *) error "Unsupported completions shell: ${COMPLETIONS_SHELL}. Supported: bash, zsh, fish" ;;
    esac
  fi

  # Validate channel
  case "$CHANNEL" in
    stable|nightly|canary) ;;
    *) error "Unknown channel: ${CHANNEL}. Supported: stable, nightly, canary" ;;
  esac
}

# ──────────────────────────────────────────────────────────────────────────────
# Prerequisite checks
# ──────────────────────────────────────────────────────────────────────────────
has_cmd() {
  command -v "$1" >/dev/null 2>&1
}

need_cmd() {
  if ! has_cmd "$1"; then
    error "Required command not found: ${BOLD}$1${NC}"
  fi
}

DOWNLOAD_CMD=""

detect_downloader() {
  if has_cmd curl; then
    DOWNLOAD_CMD="curl"
  elif has_cmd wget; then
    DOWNLOAD_CMD="wget"
  else
    error "Either ${BOLD}curl${NC} or ${BOLD}wget${NC} is required to download files."
  fi
  debug "Using downloader: ${DOWNLOAD_CMD}"
}

# Download a URL to a file path. Shows progress on TTY, silent otherwise.
download_to() {
  local url="$1"
  local dest="$2"

  debug "Downloading: ${url}"
  debug "         To: ${dest}"

  if [[ "$DOWNLOAD_CMD" == "curl" ]]; then
    if [[ -t 2 ]] && [[ "$QUIET" -eq 0 ]]; then
      curl -fSL --progress-bar "$url" -o "$dest"
    else
      curl -fsSL "$url" -o "$dest"
    fi
  else
    if [[ -t 2 ]] && [[ "$QUIET" -eq 0 ]]; then
      wget --show-progress -q "$url" -O "$dest"
    else
      wget -q "$url" -O "$dest"
    fi
  fi
}

# Download a URL to stdout (for API calls).
download_stdout() {
  local url="$1"

  if [[ "$DOWNLOAD_CMD" == "curl" ]]; then
    curl -fsSL "$url"
  else
    wget -qO- "$url"
  fi
}

# Fetch a GitHub API URL with optional token auth; provides actionable errors on 403/404.
github_api_get() {
  local url="$1"
  local tmp http_code response

  if [[ "$DOWNLOAD_CMD" == "curl" ]]; then
    tmp="$(mktemp)"
    if [[ -n "$GITHUB_TOKEN" ]]; then
      http_code="$(curl -sL -w "%{http_code}" \
        -H "Authorization: Bearer ${GITHUB_TOKEN}" \
        -H "Accept: application/vnd.github+json" \
        -H "X-GitHub-Api-Version: 2022-11-28" \
        -o "$tmp" "$url" 2>/dev/null)" || http_code="000"
    else
      http_code="$(curl -sL -w "%{http_code}" \
        -H "Accept: application/vnd.github+json" \
        -H "X-GitHub-Api-Version: 2022-11-28" \
        -o "$tmp" "$url" 2>/dev/null)" || http_code="000"
    fi
    response="$(cat "$tmp")"
    rm -f "$tmp"
  else
    if [[ -n "$GITHUB_TOKEN" ]]; then
      response="$(wget -qO- \
        --header="Authorization: Bearer ${GITHUB_TOKEN}" \
        --header="Accept: application/vnd.github+json" \
        "$url" 2>/dev/null)" || response=""
    else
      response="$(wget -qO- \
        --header="Accept: application/vnd.github+json" \
        "$url" 2>/dev/null)" || response=""
    fi
    http_code="200"
  fi

  case "$http_code" in
    403)
      error "GitHub API rate limit exceeded (HTTP 403). Set GITHUB_TOKEN env var or pass --token <token> to authenticate and raise the limit to 5,000 req/hour."
      ;;
    404)
      error "No releases found on GitHub (HTTP 404). The project may not have published a release yet. Use --version to specify a version manually."
      ;;
    000|"")
      error "Failed to reach GitHub API (no response). Check your network connection or specify a version with --version."
      ;;
    2*)
      : # success range
      ;;
    *)
      error "GitHub API returned HTTP ${http_code}. Use --version to bypass API resolution."
      ;;
  esac

  if [[ -z "$response" ]]; then
    error "GitHub API returned an empty response. Use --version to bypass API resolution."
  fi

  echo "$response"
}

detect_sha_cmd() {
  if has_cmd sha256sum; then
    SHA_CMD="sha256sum"
  elif has_cmd shasum; then
    SHA_CMD="shasum -a 256"
  else
    SHA_CMD=""
  fi
  debug "SHA-256 command: ${SHA_CMD:-none}"
}

check_dependencies() {
  detect_downloader
  detect_sha_cmd

  need_cmd uname
  need_cmd mktemp

  if [[ "$FROM_SOURCE" -eq 1 ]]; then
    need_cmd cargo
    need_cmd rustc
    need_cmd git

    # Check Rust version >= 1.80
    local rust_version
    rust_version="$(rustc --version | grep -oE '[0-9]+\.[0-9]+\.[0-9]+')"
    local rust_major rust_minor
    rust_major="$(echo "$rust_version" | cut -d. -f1)"
    rust_minor="$(echo "$rust_version" | cut -d. -f2)"

    if [[ "$rust_major" -lt 1 ]] || { [[ "$rust_major" -eq 1 ]] && [[ "$rust_minor" -lt 80 ]]; }; then
      error "Rust 1.80+ is required (found ${rust_version}). Update via: ${BOLD}rustup update stable${NC}"
    fi
    debug "Rust version: ${rust_version}"
  fi
}

# ──────────────────────────────────────────────────────────────────────────────
# Platform & architecture detection
# ──────────────────────────────────────────────────────────────────────────────
PLATFORM=""
ARCH=""

detect_platform() {
  local os
  os="$(uname -s)"

  case "$os" in
    Linux*)   PLATFORM="linux" ;;
    Darwin*)  PLATFORM="macos" ;;
    MINGW*|MSYS*|CYGWIN*)
      PLATFORM="windows"
      warn "Windows detected via ${os}. Consider using WSL for best experience."
      ;;
    FreeBSD*) PLATFORM="freebsd" ;;
    *)
      error "Unsupported operating system: ${os}"
      ;;
  esac

  debug "Detected platform: ${PLATFORM}"
  success "Platform: ${BOLD}${PLATFORM}${NC}"
}

detect_arch() {
  local machine
  machine="$(uname -m)"

  case "$machine" in
    x86_64|amd64)
      ARCH="x86_64"
      ;;
    i686|i386|i586)
      ARCH="x86"
      warn "32-bit x86 detected. Pre-built binaries may not be available."
      warn "Consider using --from-source if download fails."
      ;;
    aarch64|arm64)
      ARCH="aarch64"
      ;;
    armv7l|armv7)
      ARCH="armv7"
      ;;
    *)
      error "Unsupported architecture: ${machine}. Supported: x86_64, aarch64, arm64, armv7, i686"
      ;;
  esac

  debug "Detected architecture: ${ARCH} (raw: ${machine})"
  success "Architecture: ${BOLD}${ARCH}${NC}"
}

# Build the artifact name matching the release workflow convention
build_artifact_name() {
  local version="$1"
  local ext=""
  if [[ "$PLATFORM" == "windows" ]]; then
    ext=".exe"
  fi
  echo "${BINARY_NAME}-v${version}-${PLATFORM}-${ARCH}${ext}"
}

# ──────────────────────────────────────────────────────────────────────────────
# Install directory resolution
# ──────────────────────────────────────────────────────────────────────────────
resolve_install_dir() {
  if [[ -n "$INSTALL_DIR" ]]; then
    debug "Using user-specified install dir: ${INSTALL_DIR}"
    return
  fi

  # Check well-known writable paths
  if [[ -d "$HOME/.cargo/bin" ]] && [[ -w "$HOME/.cargo/bin" ]]; then
    INSTALL_DIR="$HOME/.cargo/bin"
  elif [[ -d "$HOME/.local/bin" ]] && [[ -w "$HOME/.local/bin" ]]; then
    INSTALL_DIR="$HOME/.local/bin"
  elif [[ -d "$HOME/.local/bin" ]]; then
    INSTALL_DIR="$HOME/.local/bin"
  else
    INSTALL_DIR="$HOME/.local/bin"
  fi

  debug "Resolved install dir: ${INSTALL_DIR}"
}

# ──────────────────────────────────────────────────────────────────────────────
# Privilege escalation
# ──────────────────────────────────────────────────────────────────────────────
needs_sudo() {
  local dir="$1"
  # Check if dir exists and we can write to it
  if [[ -d "$dir" ]] && [[ -w "$dir" ]]; then
    return 1
  fi
  # Check if we can create the dir
  local parent="$dir"
  while [[ ! -d "$parent" ]]; do
    parent="$(dirname "$parent")"
  done
  if [[ -w "$parent" ]]; then
    return 1
  fi
  return 0
}

run_privileged() {
  if [[ "$(id -u)" -eq 0 ]]; then
    "$@"
  elif has_cmd sudo; then
    info "Requesting elevated permissions to install to ${INSTALL_DIR}"
    sudo "$@"
  elif has_cmd doas; then
    info "Requesting elevated permissions to install to ${INSTALL_DIR}"
    doas "$@"
  else
    error "Root permissions are required to install to ${INSTALL_DIR}. Install sudo or use --dir to choose a writable location."
  fi
}

# ──────────────────────────────────────────────────────────────────────────────
# Version resolution
# ──────────────────────────────────────────────────────────────────────────────
resolve_version() {
  # Strip leading 'v' if present
  VERSION="${VERSION#v}"

  if [[ "$VERSION" != "latest" ]]; then
    debug "Using specified version: ${VERSION}"
    success "Version: ${BOLD}v${VERSION}${NC}"
    return
  fi

  info "Fetching latest release from GitHub..."

  local api_url="${GITHUB_API}/latest"
  local response
  response="$(github_api_get "$api_url")"

  # Parse "tag_name": "vX.Y.Z" without jq
  VERSION="$(echo "$response" | grep -oE '"tag_name"\s*:\s*"[^"]+"' | head -1 | grep -oE 'v[0-9]+\.[0-9]+\.[0-9]+[^"]*' | head -1)"
  VERSION="${VERSION#v}"

  if [[ -z "$VERSION" ]]; then
    error "Could not determine latest version from GitHub API response."
  fi

  debug "Resolved latest version: ${VERSION}"
  success "Version: ${BOLD}v${VERSION}${NC} ${DIM}(latest)${NC}"
}

# ──────────────────────────────────────────────────────────────────────────────
# Existing install check
# ──────────────────────────────────────────────────────────────────────────────
check_existing_install() {
  local existing_path
  existing_path="$(command -v "$BINARY_NAME" 2>/dev/null || true)"

  if [[ -z "$existing_path" ]]; then
    debug "No existing installation found."
    return
  fi

  local existing_version
  existing_version="$("$existing_path" --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' || echo "unknown")"

  if [[ "$FORCE" -eq 1 ]]; then
    warn "Existing installation found at ${existing_path} (v${existing_version}). Overwriting due to --force."
    return
  fi

  if [[ "$existing_version" == "$VERSION" ]]; then
    success "AgentZero v${VERSION} is already installed at ${existing_path}"
    info "Use ${BOLD}--force${NC} to reinstall."
    exit 0
  fi

  info "Upgrading from v${existing_version} → v${VERSION}"
}

# ──────────────────────────────────────────────────────────────────────────────
# Temp directory & cleanup
# ──────────────────────────────────────────────────────────────────────────────
TMP_DIR=""

create_tmp_dir() {
  TMP_DIR="$(mktemp -d 2>/dev/null || mktemp -d -t agentzero-install)"
  debug "Created temp directory: ${TMP_DIR}"
}

cleanup() {
  if [[ -n "${TMP_DIR}" ]] && [[ -d "${TMP_DIR}" ]]; then
    rm -rf "${TMP_DIR}"
    debug "Cleaned up temp directory"
  fi
}

trap cleanup EXIT INT TERM HUP

# ──────────────────────────────────────────────────────────────────────────────
# Download binary
# ──────────────────────────────────────────────────────────────────────────────
ARTIFACT_NAME=""
ARTIFACT_PATH=""

download_binary() {
  create_tmp_dir

  ARTIFACT_NAME="$(build_artifact_name "$VERSION")"
  ARTIFACT_PATH="${TMP_DIR}/${ARTIFACT_NAME}"

  local download_url="${GITHUB_RELEASE}/v${VERSION}/${ARTIFACT_NAME}"

  if [[ "$DRY_RUN" -eq 1 ]]; then
    info "${DIM}[dry-run]${NC} Would download: ${download_url}"
    info "${DIM}[dry-run]${NC} To: ${ARTIFACT_PATH}"
    return
  fi

  info "Downloading ${BOLD}${ARTIFACT_NAME}${NC}..."
  debug "URL: ${download_url}"

  if ! download_to "$download_url" "$ARTIFACT_PATH"; then
    error_noexit "Download failed for ${ARTIFACT_NAME}."
    printf "\n" >&2
    info "Possible causes:"
    info "  • Version v${VERSION} may not exist"
    info "  • No pre-built binary for ${PLATFORM}-${ARCH}"
    info "  • Network connectivity issue"
    printf "\n" >&2
    info "Try: ${BOLD}--from-source${NC} to build from source"
    info "  or ${BOLD}--version VERSION${NC} to specify a different version"
    exit 1
  fi

  success "Downloaded ${ARTIFACT_NAME}"
}

# ──────────────────────────────────────────────────────────────────────────────
# Checksum verification
# ──────────────────────────────────────────────────────────────────────────────
verify_checksum() {
  if [[ "$NO_VERIFY" -eq 1 ]]; then
    warn "Skipping checksum verification (--no-verify)"
    return
  fi

  if [[ "$DRY_RUN" -eq 1 ]]; then
    info "${DIM}[dry-run]${NC} Would verify SHA-256 checksum"
    return
  fi

  if [[ -z "$SHA_CMD" ]]; then
    warn "No SHA-256 tool found (sha256sum or shasum). Skipping verification."
    warn "Install sha256sum or use --no-verify to suppress this warning."
    return
  fi

  local checksums_url="${GITHUB_RELEASE}/v${VERSION}/SHA256SUMS"
  local checksums_path="${TMP_DIR}/SHA256SUMS"

  info "Verifying checksum..."

  if ! download_to "$checksums_url" "$checksums_path" 2>/dev/null; then
    warn "SHA256SUMS file not found for this release. Skipping verification."
    warn "Use --no-verify to suppress this warning."
    return
  fi

  local expected_hash
  expected_hash="$(grep "${ARTIFACT_NAME}" "$checksums_path" | awk '{print $1}')"

  if [[ -z "$expected_hash" ]]; then
    warn "No checksum found for ${ARTIFACT_NAME} in SHA256SUMS. Skipping verification."
    return
  fi

  local actual_hash
  actual_hash="$(${SHA_CMD} "$ARTIFACT_PATH" | awk '{print $1}')"

  debug "Expected: ${expected_hash}"
  debug "Actual:   ${actual_hash}"

  if [[ "$expected_hash" != "$actual_hash" ]]; then
    error "Checksum verification failed!

  Expected: ${expected_hash}
  Actual:   ${actual_hash}

  The downloaded file may be corrupted or tampered with.
  Use --no-verify to skip this check (not recommended)."
  fi

  success "Checksum verified ${DIM}(SHA-256)${NC}"
}

# ──────────────────────────────────────────────────────────────────────────────
# Install binary to target directory
# ──────────────────────────────────────────────────────────────────────────────
install_binary() {
  resolve_install_dir

  local bin_path="${INSTALL_DIR}/${BINARY_NAME}"

  if [[ "$DRY_RUN" -eq 1 ]]; then
    info "${DIM}[dry-run]${NC} Would install to: ${bin_path}"
    if needs_sudo "$INSTALL_DIR"; then
      info "${DIM}[dry-run]${NC} Would require elevated permissions"
    fi
    return
  fi

  if needs_sudo "$INSTALL_DIR"; then
    run_privileged mkdir -p "$INSTALL_DIR"
    run_privileged install -m 0755 "$ARTIFACT_PATH" "$bin_path"
  else
    mkdir -p "$INSTALL_DIR"
    install -m 0755 "$ARTIFACT_PATH" "$bin_path"
  fi

  success "Installed to ${BOLD}${bin_path}${NC}"
}

# ──────────────────────────────────────────────────────────────────────────────
# Shell completions
# ──────────────────────────────────────────────────────────────────────────────
install_completions() {
  local bin_path="${INSTALL_DIR}/${BINARY_NAME}"
  local comp_dir=""
  local comp_file=""

  case "$COMPLETIONS_SHELL" in
    bash)
      comp_dir="$HOME/.local/share/bash-completion/completions"
      comp_file="${comp_dir}/${BINARY_NAME}"
      ;;
    zsh)
      comp_dir="$HOME/.zfunc"
      comp_file="${comp_dir}/_${BINARY_NAME}"
      ;;
    fish)
      comp_dir="$HOME/.config/fish/completions"
      comp_file="${comp_dir}/${BINARY_NAME}.fish"
      ;;
  esac

  if [[ "$DRY_RUN" -eq 1 ]]; then
    info "${DIM}[dry-run]${NC} Would generate ${COMPLETIONS_SHELL} completions to ${comp_file}"
    return
  fi

  mkdir -p "$comp_dir"

  if "$bin_path" completions --shell "$COMPLETIONS_SHELL" > "$comp_file" 2>/dev/null; then
    success "Installed ${COMPLETIONS_SHELL} completions to ${comp_file}"
    if [[ "$COMPLETIONS_SHELL" == "zsh" ]]; then
      info "Ensure ${BOLD}fpath+=(~/.zfunc)${NC} is in your .zshrc before compinit"
    fi
  else
    warn "Could not generate ${COMPLETIONS_SHELL} completions (command may not support it yet)"
  fi
}

# ──────────────────────────────────────────────────────────────────────────────
# Build from source
# ──────────────────────────────────────────────────────────────────────────────
build_from_source() {
  create_tmp_dir

  local src_dir="${TMP_DIR}/agentzero-src"

  if [[ "$DRY_RUN" -eq 1 ]]; then
    info "${DIM}[dry-run]${NC} Would clone ${REPO} and build from source"
    info "${DIM}[dry-run]${NC} cargo build -p ${BINARY_NAME} --release"
    ARTIFACT_PATH="${TMP_DIR}/placeholder"
    return
  fi

  local clone_ref="main"
  if [[ "$VERSION" != "latest" ]]; then
    clone_ref="v${VERSION}"
  fi

  info "Cloning repository (${clone_ref})..."
  if ! git clone --depth 1 --branch "$clone_ref" "https://github.com/${REPO}.git" "$src_dir" 2>&1; then
    error "Failed to clone repository. Check that the version tag exists."
  fi

  info "Building from source... ${DIM}(this may take a few minutes)${NC}"
  if ! (cd "$src_dir" && cargo build -p "$BINARY_NAME" --release 2>&1); then
    error "Build failed. Check the output above for errors."
  fi

  ARTIFACT_PATH="${src_dir}/target/release/${BINARY_NAME}"
  if [[ "$PLATFORM" == "windows" ]]; then
    ARTIFACT_PATH="${ARTIFACT_PATH}.exe"
  fi

  if [[ ! -f "$ARTIFACT_PATH" ]]; then
    error "Build completed but binary not found at expected path: ${ARTIFACT_PATH}"
  fi

  success "Built from source"
}

# ──────────────────────────────────────────────────────────────────────────────
# Uninstall
# ──────────────────────────────────────────────────────────────────────────────
uninstall() {
  resolve_install_dir

  local bin_path="${INSTALL_DIR}/${BINARY_NAME}"
  local data_dir="$HOME/.agentzero"

  step 1 "Removing agentzero"

  if [[ ! -f "$bin_path" ]]; then
    # Try to find it in PATH
    local found_path
    found_path="$(command -v "$BINARY_NAME" 2>/dev/null || true)"
    if [[ -n "$found_path" ]]; then
      bin_path="$found_path"
      info "Found installation at ${bin_path}"
    else
      warn "No agentzero binary found at ${bin_path} or in PATH"
    fi
  fi

  if [[ -f "$bin_path" ]]; then
    if [[ "$DRY_RUN" -eq 1 ]]; then
      info "${DIM}[dry-run]${NC} Would remove: ${bin_path}"
    else
      if needs_sudo "$(dirname "$bin_path")"; then
        run_privileged rm -f "$bin_path"
      else
        rm -f "$bin_path"
      fi
      success "Removed ${bin_path}"
    fi
  fi

  # Remove shell completions
  local comp_files=(
    "$HOME/.local/share/bash-completion/completions/${BINARY_NAME}"
    "$HOME/.zfunc/_${BINARY_NAME}"
    "$HOME/.config/fish/completions/${BINARY_NAME}.fish"
  )

  for comp_file in "${comp_files[@]}"; do
    if [[ -f "$comp_file" ]]; then
      if [[ "$DRY_RUN" -eq 1 ]]; then
        info "${DIM}[dry-run]${NC} Would remove: ${comp_file}"
      else
        rm -f "$comp_file"
        success "Removed ${comp_file}"
      fi
    fi
  done

  # Offer to remove data directory
  if [[ -d "$data_dir" ]]; then
    step 2 "Data directory"

    if [[ "$DRY_RUN" -eq 1 ]]; then
      info "${DIM}[dry-run]${NC} Would ask about removing: ${data_dir}"
    elif [[ -t 0 ]] && [[ "$FORCE" -eq 0 ]]; then
      printf "\n" >&2
      printf "  ${YELLOW}Remove data directory ${data_dir}?${NC}\n" >&2
      printf "  This includes configuration, memory database, and plugins.\n" >&2
      printf "  ${DIM}[y/N]${NC} " >&2
      local answer
      read -r answer
      if [[ "$answer" =~ ^[Yy]$ ]]; then
        rm -rf "$data_dir"
        success "Removed ${data_dir}"
      else
        info "Kept ${data_dir}"
      fi
    elif [[ "$FORCE" -eq 1 ]]; then
      rm -rf "$data_dir"
      success "Removed ${data_dir}"
    else
      info "Data directory preserved: ${data_dir}"
      info "Remove manually with: ${BOLD}rm -rf ${data_dir}${NC}"
    fi
  fi

  printf "\n" >&2
  success "${BOLD}AgentZero has been uninstalled.${NC}"
}

# ──────────────────────────────────────────────────────────────────────────────
# Post-install
# ──────────────────────────────────────────────────────────────────────────────
post_install() {
  if [[ "$DRY_RUN" -eq 1 ]]; then
    printf "\n" >&2
    success "${BOLD}Dry run complete.${NC} No changes were made."
    return
  fi

  local bin_path="${INSTALL_DIR}/${BINARY_NAME}"

  printf "\n" >&2

  # Verify installation
  if [[ -x "$bin_path" ]]; then
    local installed_version
    installed_version="$("$bin_path" --version 2>/dev/null || echo "installed")"
    success "${BOLD}${installed_version}${NC}"
  fi

  # Check PATH
  case ":${PATH}:" in
    *":${INSTALL_DIR}:"*)
      debug "Install directory is already in PATH"
      ;;
    *)
      local shell_name
      shell_name="$(basename "${SHELL:-/bin/bash}")"

      if [[ "$shell_name" == "fish" ]]; then
        local fish_config="$HOME/.config/fish/config.fish"
        mkdir -p "$(dirname "$fish_config")"
        if ! grep -qF "fish_add_path ${INSTALL_DIR}" "$fish_config" 2>/dev/null; then
          printf "\nfish_add_path %s\n" "$INSTALL_DIR" >> "$fish_config"
          success "Added ${INSTALL_DIR} to PATH in ${fish_config}"
        else
          debug "${INSTALL_DIR} already in ${fish_config}"
        fi
      else
        local rc_file
        case "$shell_name" in
          zsh) rc_file="${HOME}/.zshrc" ;;
          *)   rc_file="${HOME}/.bashrc" ;;
        esac
        local export_line="export PATH=\"${INSTALL_DIR}:\$PATH\""
        if ! grep -qF "$export_line" "$rc_file" 2>/dev/null; then
          printf "\n%s\n" "$export_line" >> "$rc_file"
          success "Added ${INSTALL_DIR} to PATH in ${rc_file}"
          # Reflect in the current process so the success banner is accurate
          export PATH="${INSTALL_DIR}:${PATH}"
        else
          debug "${INSTALL_DIR} already in ${rc_file}"
        fi
      fi
      ;;
  esac

  # Success banner
  printf "  ${BOLD_GREEN}╭────────────────────────────────────────────╮${NC}\n" >&2
  printf "  ${BOLD_GREEN}│${NC}                                            ${BOLD_GREEN}│${NC}\n" >&2
  printf "  ${BOLD_GREEN}│${NC}   ${BOLD}Installation complete!${NC}                    ${BOLD_GREEN}│${NC}\n" >&2
  printf "  ${BOLD_GREEN}│${NC}                                            ${BOLD_GREEN}│${NC}\n" >&2
  printf "  ${BOLD_GREEN}│${NC}   Get started:                             ${BOLD_GREEN}│${NC}\n" >&2
  printf "  ${BOLD_GREEN}│${NC}     ${CYAN}$ agentzero onboard${NC}                    ${BOLD_GREEN}│${NC}\n" >&2
  printf "  ${BOLD_GREEN}│${NC}     ${CYAN}$ agentzero --help${NC}                     ${BOLD_GREEN}│${NC}\n" >&2
  printf "  ${BOLD_GREEN}│${NC}                                            ${BOLD_GREEN}│${NC}\n" >&2
  printf "  ${BOLD_GREEN}╰────────────────────────────────────────────╯${NC}\n" >&2
  printf "\n" >&2
}

# ──────────────────────────────────────────────────────────────────────────────
# Main
# ──────────────────────────────────────────────────────────────────────────────
main() {
  # Initialize colors early with defaults, then re-init after parsing --no-color
  setup_colors
  parse_args "$@"
  setup_colors
  show_banner

  if [[ "$UNINSTALL" -eq 1 ]]; then
    uninstall
    exit 0
  fi

  step 1 "Detecting platform"
  detect_platform
  detect_arch

  step 2 "Checking dependencies"
  check_dependencies

  step 3 "Resolving version"
  resolve_version

  check_existing_install

  if [[ "$FROM_SOURCE" -eq 1 ]]; then
    step 4 "Building from source"
    build_from_source
  else
    step 4 "Downloading agentzero v${VERSION}"
    download_binary

    step 5 "Verifying checksum"
    verify_checksum
  fi

  local install_step=6
  if [[ "$FROM_SOURCE" -eq 1 ]]; then
    install_step=5
  fi

  step "$install_step" "Installing to ${INSTALL_DIR:-<auto>}"
  install_binary

  if [[ -n "$COMPLETIONS_SHELL" ]]; then
    local comp_step=$((install_step + 1))
    step "$comp_step" "Installing ${COMPLETIONS_SHELL} completions"
    install_completions
  fi

  post_install
}

main "$@"
