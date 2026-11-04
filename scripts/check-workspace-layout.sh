#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${repo_root}"

if grep -Eq '^\[package\][[:space:]]*$' Cargo.toml; then
  echo "root Cargo.toml must be a virtual workspace manifest" >&2
  exit 1
fi

if [[ ! -f apps/tui/Cargo.toml || ! -f apps/tui/src/main.rs ]]; then
  echo "native amp808 package must live in apps/tui" >&2
  exit 1
fi

if [[ ! -d apps/tui/tests/fixtures ]]; then
  echo "native amp808 test fixtures must move with the package" >&2
  exit 1
fi

cargo metadata --no-deps --format-version 1 | python3 -c '
import json
import pathlib
import sys

metadata = json.load(sys.stdin)
root = pathlib.Path(metadata["workspace_root"])
packages = {package["name"]: package for package in metadata["packages"]}

required = {"amp808", "amp808-core", "amp808_web"}
missing = required - packages.keys()
assert not missing, f"missing workspace packages: {sorted(missing)}"

amp808 = packages["amp808"]
assert pathlib.Path(amp808["manifest_path"]) == root / "apps/tui/Cargo.toml"
assert any(
    target["name"] == "amp808" and "bin" in target["kind"]
    for target in amp808["targets"]
), "amp808 binary target was not preserved"

default_ids = set(metadata["workspace_default_members"])
assert amp808["id"] in default_ids, "amp808 must remain a default workspace member"
assert packages["amp808-core"]["id"] in default_ids, (
    "amp808-core must remain a default workspace member"
)
'
