#!/usr/bin/env bash
# Install the `dit` CLI: download a release binary when one exists for this
# platform, otherwise build from source. Safe to re-run.

set -euo pipefail

repo="faridlab/dit-cli"
repo_url="https://github.com/${repo}"

say() { printf 'dit: %s\n' "$*"; }
die() { printf 'dit: error: %s\n' "$*" >&2; exit 1; }

# --- where the binary goes ----------------------------------------------------
# Cargo's bin dir when it exists (the usual case on a Rust machine), otherwise
# ~/.local/bin, the XDG conventional spot.

install_to() {
  local src="$1" dest
  if [ -n "${CARGO_HOME:-}" ] && [ -d "${CARGO_HOME}/bin" ]; then
    dest="${CARGO_HOME}/bin"
  elif [ -d "${HOME}/.cargo/bin" ]; then
    dest="${HOME}/.cargo/bin"
  else
    dest="${HOME}/.local/bin"
    mkdir -p "$dest"
  fi
  install -m 755 "$src" "${dest}/dit"
  say "installed ${dest}/dit"
  case ":${PATH}:" in
    *":${dest}:"*) ;;
    *) say "note: ${dest} is not on your PATH — add it to use 'dit'" ;;
  esac
}

# --- build from source ---------------------------------------------------------
# The fallback when no release matches. With npm available the web UI is built
# and embedded, giving a binary that serves the browser UI; without npm the
# binary is CLI-only until rebuilt with --features embed-ui.

build_from_source() {
  command -v git >/dev/null 2>&1 || die "git is required to build from source"
  command -v cargo >/dev/null 2>&1 || die "cargo is required to build from source (https://rustup.rs)"

  local src="$tmpdir/dit-src"
  git clone --depth 1 --quiet "$repo_url" "$src"

  local features=""
  if command -v npm >/dev/null 2>&1; then
    say "building the web UI (npm found)"
    (cd "$src" && npm ci --prefix apps/web && npm run build --prefix apps/web)
    features="--features embed-ui"
  else
    say "npm not found — building without the embedded UI ('dit ui' will not serve pages)"
  fi

  # shellcheck disable=SC2086
  (cd "$src" && cargo build --release --locked $features -p dit-cli)
  install_to "$src/target/release/dit"
}

# --- download a release --------------------------------------------------------

target=""
case "$(uname -s):$(uname -m)" in
  Darwin:arm64)  target="aarch64-apple-darwin" ;;
  Darwin:x86_64) target="x86_64-apple-darwin" ;;
  Linux:aarch64) target="aarch64-unknown-linux-gnu" ;;
  Linux:x86_64)  target="x86_64-unknown-linux-gnu" ;;
  *) say "no prebuilt release for $(uname -s) $(uname -m)" ;;
esac

tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT

if [ -n "$target" ]; then
  url="${repo_url}/releases/latest/download/dit-${target}.tar.gz"
  if command -v curl >/dev/null 2>&1; then
    fetch() { curl -fsSL "$1" -o "$2"; }
  elif command -v wget >/dev/null 2>&1; then
    fetch() { wget -q "$1" -O "$2"; }
  else
    say "neither curl nor wget found — building from source"
    fetch() { return 1; }
  fi

  if fetch "$url" "$tmpdir/dit.tar.gz"; then
    tar -xzf "$tmpdir/dit.tar.gz" -C "$tmpdir"
    [ -f "$tmpdir/dit" ] || die "the downloaded archive has no 'dit' binary"
    install_to "$tmpdir/dit"
    exit 0
  fi
  say "no release binary at ${url} — building from source"
fi

build_from_source
