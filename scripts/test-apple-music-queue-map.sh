#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
bridge_source="$repository_root/apps/desktop/macos/music-kit-bridge/Amp808MusicKitBridge.swift"
test_source="$repository_root/apps/desktop/macos/music-kit-bridge/QueueOccurrenceMapTests.swift"
test_directory="$(mktemp -d "${TMPDIR:-/tmp}/amp808-queue-map.XXXXXX")"
test_binary="$test_directory/queue-occurrence-map-tests"

cleanup() {
    rm -rf "$test_directory"
}
trap cleanup EXIT

xcrun swiftc \
    -swift-version 6 \
    -parse-as-library \
    -warnings-as-errors \
    -framework MusicKit \
    "$bridge_source" \
    "$test_source" \
    -o "$test_binary"

"$test_binary"
echo "Apple Music queue occurrence mapping tests passed"
