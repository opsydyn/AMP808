#!/usr/bin/env bash
set -euo pipefail

info_plist="apps/desktop/macos/Info.plist"
entitlements="apps/desktop/macos/AMP808-R3TR0.entitlements"
expected_bundle_id="com.opsydyn.amp808r3tr0"
expected_usage="AMP808-R3TR0 uses your media library to browse playlists and metadata and play your selections through Apple Music."

plutil -lint "$info_plist" >/dev/null
plutil -lint "$entitlements" >/dev/null

actual_bundle_id="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$info_plist")"
actual_usage="$(/usr/libexec/PlistBuddy -c 'Print :NSAppleMusicUsageDescription' "$info_plist")"

if [[ "$actual_bundle_id" != "$expected_bundle_id" ]]; then
    echo "unexpected AMP808-R3TR0 bundle identifier: $actual_bundle_id" >&2
    exit 1
fi

if [[ "$actual_usage" != "$expected_usage" ]]; then
    echo "unexpected Apple Music usage description: $actual_usage" >&2
    exit 1
fi
