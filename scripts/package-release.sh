#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd -- "$(dirname -- "$0")"/.. && pwd)"
cd "$ROOT_DIR"

TARGETS=(
  x86_64-apple-darwin
  aarch64-apple-darwin
  x86_64-unknown-linux-gnu
  aarch64-unknown-linux-gnu
)

mkdir -p dist

for TARGET in "${TARGETS[@]}"; do
  echo "==> Building for $TARGET"
  rustup target add "$TARGET" >/dev/null 2>&1 || true
  cargo build --release --target "$TARGET"
  BIN="target/$TARGET/release/awx"
  ART="awx-$TARGET"
  cp "$BIN" awx
  tar -czf "dist/${ART}.tar.gz" awx
  rm -f awx
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "dist/${ART}.tar.gz" > "dist/${ART}.tar.gz.sha256"
  elif command -v sha256sum >/dev/null 2>&1; then
    sha256sum "dist/${ART}.tar.gz" > "dist/${ART}.tar.gz.sha256"
  fi
done

echo "Artifacts in ./dist:"
ls -1 dist
