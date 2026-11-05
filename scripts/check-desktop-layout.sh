#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${repo_root}"

if [[ ! -f apps/desktop/Cargo.toml || ! -f apps/desktop/src/main.rs ]]; then
  echo "AMP808-R3TR0 package must live in apps/desktop" >&2
  exit 1
fi

cargo metadata --no-deps --format-version 1 | python3 -c '
import json
import pathlib
import sys

metadata = json.load(sys.stdin)
root = pathlib.Path(metadata["workspace_root"])
packages = {package["name"]: package for package in metadata["packages"]}

desktop = packages.get("amp808-r3tr0")
assert desktop is not None, "missing amp808-r3tr0 workspace package"
assert pathlib.Path(desktop["manifest_path"]) == root / "apps/desktop/Cargo.toml"
assert any(
    target["name"] == "amp808-r3tr0" and "bin" in target["kind"]
    for target in desktop["targets"]
), "amp808-r3tr0 binary target was not preserved"

tui = packages.get("amp808")
assert tui is not None, "existing amp808 TUI package is missing"
assert any(
    target["name"] == "amp808" and "bin" in target["kind"]
    for target in tui["targets"]
), "existing amp808 TUI binary was not preserved"

assert desktop["id"] not in set(metadata["workspace_default_members"]), (
    "macOS-first desktop package must not be a default workspace member yet"
)

dependencies = {dependency["name"]: dependency for dependency in desktop["dependencies"]}
kit = dependencies.get("gpui-kit")
assert kit is not None, "desktop must use gpui-kit as its GPUI facade"
assert kit["req"] == "=0.7.3", "gpui-kit must be pinned exactly to 0.7.3"
for forbidden in ("gpui", "gpui-pre", "gpui-platform", "gpui-unofficial"):
    assert forbidden not in dependencies, f"desktop must not directly depend on {forbidden}"
'

moon query projects | python3 -c '
import json
import sys

projects = json.load(sys.stdin)["projects"]
root = next(project for project in projects if project["id"] == "root")
tasks = root["tasks"]

expected = {
    "desktop-check": ["check", "--locked", "-p", "amp808-r3tr0"],
    "desktop-test": ["test", "--locked", "-p", "amp808-r3tr0"],
    "desktop-lint": [
        "clippy",
        "--locked",
        "-p",
        "amp808-r3tr0",
        "--tests",
        "--",
        "-D",
        "warnings",
    ],
    "desktop-build-macos": ["build", "--locked", "-p", "amp808-r3tr0"],
}

for task_id, args in expected.items():
    task = tasks.get(task_id)
    assert task is not None, f"missing Moon task root:{task_id}"
    assert task["command"] == "cargo", f"root:{task_id} must invoke cargo"
    assert task["args"] == args, f"root:{task_id} has unexpected arguments: {task['"'"'args'"'"']}"
    assert task["options"]["runInCI"] is False, (
        f"root:{task_id} must remain opt-in while desktop support is macOS-first"
    )
'
