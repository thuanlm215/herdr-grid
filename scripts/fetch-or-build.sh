#!/bin/sh
# Install a released, SHA-256-verified binary when one matches this checkout.
# If no usable release asset exists, preserve the source-build behavior.
set -u

repo="thuanlm215/herdr-grid"
script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo_root="${HERDR_GRID_REPO_ROOT:-$script_dir/..}"
cargo_toml="${HERDR_GRID_CARGO_TOML:-$repo_root/Cargo.toml}"
out="${HERDR_GRID_OUT:-$repo_root/target/release/herdr-grid}"
base_url="${HERDR_GRID_RELEASE_BASE_URL:-https://github.com/$repo/releases/download}"
tmpdir=""
install_tmp=""

have() {
  command -v "$1" >/dev/null 2>&1
}

cleanup() {
  if [ -n "$install_tmp" ]; then
    rm -f -- "$install_tmp"
  fi
  if [ -n "$tmpdir" ]; then
    rm -f -- "$tmpdir/binary" "$tmpdir/SHA256SUMS"
    rmdir -- "$tmpdir" 2>/dev/null || true
  fi
}

build_from_source() {
  cleanup
  if [ -n "${HOME:-}" ] && [ -f "$HOME/.cargo/env" ]; then
    # rustup may not be on the PATH inherited by the Herdr server.
    . "$HOME/.cargo/env"
  fi
  if ! have cargo; then
    echo "herdr-grid: no prebuilt binary was available and cargo was not found." >&2
    echo "Install Rust from https://rustup.rs, then re-run: herdr plugin install $repo" >&2
    exit 1
  fi
  cd "$repo_root" || exit 1
  exec cargo build --release --locked
}

fallback() {
  echo "herdr-grid: $1; building from source instead." >&2
  build_from_source
}

download() {
  if have curl; then
    curl -fsSL -o "$2" "$1"
  elif have wget; then
    wget -q -O "$2" "$1"
  else
    return 127
  fi
}

sha256_of() {
  if have sha256sum; then
    sha256sum "$1" | awk '{print $1}'
  elif have shasum; then
    shasum -a 256 "$1" | awk '{print $1}'
  else
    return 127
  fi
}

os=$(uname -s 2>/dev/null || echo unknown)
arch=$(uname -m 2>/dev/null || echo unknown)
triple=""

case "$os/$arch" in
  Darwin/arm64|Darwin/aarch64) triple="aarch64-apple-darwin" ;;
  Darwin/x86_64|Darwin/amd64) triple="x86_64-apple-darwin" ;;
  Linux/aarch64|Linux/arm64) triple="aarch64-unknown-linux-musl" ;;
  Linux/x86_64|Linux/amd64) triple="x86_64-unknown-linux-musl" ;;
esac

[ -n "$triple" ] || fallback "no prebuilt binary for $os/$arch"

version=$(grep -E '^version *= *"' "$cargo_toml" 2>/dev/null | head -n 1 | sed -E 's/^version *= *"([^"]+)".*/\1/')
[ -n "$version" ] || fallback "could not read the package version"

asset="herdr-grid-$triple"
tmpdir=$(mktemp -d 2>/dev/null) || fallback "could not create a temporary directory"
trap cleanup EXIT HUP INT TERM

download "$base_url/v$version/$asset" "$tmpdir/binary" || fallback "prebuilt v$version asset $asset is unavailable"
download "$base_url/v$version/SHA256SUMS" "$tmpdir/SHA256SUMS" || fallback "v$version checksums are unavailable"

expected=$(grep -E "^[0-9a-fA-F]{64} [ *]$asset\$" "$tmpdir/SHA256SUMS" 2>/dev/null | awk '{print tolower($1)}' | head -n 1)
[ -n "$expected" ] || fallback "no checksum is listed for $asset"

actual=$(sha256_of "$tmpdir/binary") || fallback "no SHA-256 tool is available"
actual=$(printf '%s' "$actual" | tr '[:upper:]' '[:lower:]')
[ "$actual" = "$expected" ] || fallback "checksum mismatch for $asset"

mkdir -p "$(dirname -- "$out")" || fallback "could not create the binary directory"
install_tmp=$(mktemp "${out}.tmp.XXXXXX" 2>/dev/null) || fallback "could not stage the verified binary"
cp "$tmpdir/binary" "$install_tmp" || fallback "could not stage the verified binary"
chmod +x "$install_tmp" || fallback "could not make the verified binary executable"
mv -f -- "$install_tmp" "$out" || fallback "could not install the verified binary"
install_tmp=""

echo "herdr-grid: installed prebuilt v$version ($triple), verified SHA-256."
exit 0
