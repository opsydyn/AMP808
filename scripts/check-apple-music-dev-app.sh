#!/usr/bin/env bash
set -euo pipefail

bundle_path="${1:-}"
expected_bundle_id="com.opsydyn.amp808r3tr0"
expected_executable="amp808-r3tr0"
expected_usage="AMP808-R3TR0 uses your media library to browse playlists and metadata and play your selections through Apple Music."

fail() {
    echo "AMP808-R3TR0 development app check failed: $1" >&2
    exit 1
}

[[ -n "$bundle_path" ]] || fail "pass the .app bundle path as the only argument"
[[ -d "$bundle_path" ]] || fail "bundle does not exist: $bundle_path"

info_plist="$bundle_path/Contents/Info.plist"
executable="$bundle_path/Contents/MacOS/$expected_executable"

[[ -f "$info_plist" ]] || fail "missing Contents/Info.plist"
[[ -x "$executable" ]] || fail "missing executable Contents/MacOS/$expected_executable"

plutil -lint "$info_plist" >/dev/null || fail "Info.plist is invalid"

actual_bundle_id="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$info_plist")"
actual_executable="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$info_plist")"
actual_usage="$(/usr/libexec/PlistBuddy -c 'Print :NSAppleMusicUsageDescription' "$info_plist")"

[[ "$actual_bundle_id" == "$expected_bundle_id" ]] || fail "unexpected bundle identifier"
[[ "$actual_executable" == "$expected_executable" ]] || fail "unexpected executable name"
[[ "$actual_usage" == "$expected_usage" ]] || fail "unexpected Apple Music usage description"

codesign --verify --strict "$bundle_path" 2>/dev/null || fail "bundle signature is invalid"

signature_details="$(codesign --display --verbose=4 "$bundle_path" 2>&1)"
if [[ "${AMP808_ALLOW_ADHOC_SIGNING:-0}" != "1" ]] && grep -q '^Signature=adhoc$' <<<"$signature_details"; then
    fail "ad-hoc signing cannot qualify live MusicKit acceptance"
fi

grep -q "^Identifier=$expected_bundle_id$" <<<"$signature_details" || fail "signature identifier does not match the app"
nm "$executable" | grep '_amp808_music_kit_request_connection' >/dev/null || fail "MusicKit bridge is not linked into the executable"

echo "AMP808-R3TR0 development app check passed"
