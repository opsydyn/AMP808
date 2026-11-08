#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
output_path="${1:-$repo_root/target/apple-music-dev/AMP808-R3TR0.app}"
bundle_id="com.opsydyn.amp808r3tr0"
binary_name="amp808-r3tr0"
info_plist="$repo_root/apps/desktop/macos/Info.plist"
entitlements="$repo_root/apps/desktop/macos/AMP808-R3TR0.entitlements"
signing_identity="${AMP808_CODESIGN_IDENTITY:-}"
allow_adhoc="${AMP808_ALLOW_ADHOC_SIGNING:-0}"

fail() {
    echo "AMP808-R3TR0 development app build failed: $1" >&2
    exit 1
}

[[ "$(uname -s)" == "Darwin" ]] || fail "macOS is required"
[[ "$output_path" == *.app ]] || fail "output path must end in .app"

if [[ -z "$signing_identity" ]]; then
    if [[ "$allow_adhoc" == "1" ]]; then
        signing_identity="-"
    else
        fail "set AMP808_CODESIGN_IDENTITY to an Apple Development signing identity"
    fi
fi
if [[ "$signing_identity" == "-" && "$allow_adhoc" != "1" ]]; then
    fail "set AMP808_ALLOW_ADHOC_SIGNING=1 to build a non-qualifying ad-hoc test bundle"
fi

cd "$repo_root"
cargo build --locked -p amp808-r3tr0

target_dir="${CARGO_TARGET_DIR:-$repo_root/target}"
if [[ "$target_dir" != /* ]]; then
    target_dir="$repo_root/$target_dir"
fi
source_binary="$target_dir/debug/$binary_name"
[[ -x "$source_binary" ]] || fail "Cargo did not produce $source_binary"

output_parent="$(dirname "$output_path")"
mkdir -p "$output_parent"
staging_root="$(mktemp -d "$output_parent/.amp808-dev-app.XXXXXX")"
trap 'rm -rf "$staging_root"' EXIT
staging_app="$staging_root/AMP808-R3TR0.app"

mkdir -p "$staging_app/Contents/MacOS"
install -m 755 "$source_binary" "$staging_app/Contents/MacOS/$binary_name"
install -m 644 "$info_plist" "$staging_app/Contents/Info.plist"

codesign \
    --force \
    --sign "$signing_identity" \
    --identifier "$bundle_id" \
    --entitlements "$entitlements" \
    --timestamp=none \
    "$staging_app"

AMP808_ALLOW_ADHOC_SIGNING="$allow_adhoc" \
    "$repo_root/scripts/check-apple-music-dev-app.sh" "$staging_app"

if [[ -e "$output_path" ]]; then
    backup_path="$output_path.previous.$(date -u +%Y%m%dT%H%M%SZ)"
    mv "$output_path" "$backup_path"
    echo "Preserved previous bundle at $backup_path"
fi
mv "$staging_app" "$output_path"

echo "Built AMP808-R3TR0 development app at $output_path"
if [[ "$signing_identity" == "-" ]]; then
    echo "This bundle is ad-hoc signed for assembly testing and cannot qualify live MusicKit acceptance."
fi
