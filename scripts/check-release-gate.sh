#!/usr/bin/env bash
set -euo pipefail

cargo fmt --check
cargo nextest run --profile ci --locked

has_lib="$(cargo metadata --no-deps --format-version 1 | python3 -c 'import json, sys; meta=json.load(sys.stdin); print("true" if any("lib" in target.get("kind", []) for package in meta["packages"] for target in package.get("targets", [])) else "false")')"
if [[ "$has_lib" == "true" ]]; then
  cargo test --doc --quiet --locked
else
  echo "No library target present; skipping doctests."
fi

cargo clippy --all-targets --locked -- -D warnings
