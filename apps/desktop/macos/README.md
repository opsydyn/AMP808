# AMP808-R3TR0 macOS configuration

The permanent desktop bundle identifier is `com.opsydyn.amp808r3tr0`.
`Info.plist` contains the media-library purpose string that macOS shows when the user explicitly
asks AMP808-R3TR0 to connect to Apple Music.

MusicKit automatic developer-token generation is a runtime App Service associated with the exact
bundle identifier. Before live acceptance, register an explicit App ID with this identifier in
Certificates, Identifiers & Profiles and enable MusicKit under **App Services**. This is an Apple
Developer account operation; it is not represented by inventing a local entitlement key.

The checked-in entitlements file continues to describe only capabilities already used by the
desktop app. MusicKit is an App Service associated with the explicit App ID rather than a reason to
invent an entitlement. Provisioning, notarisation, and release signing remain later delivery
gates. No authorization request may run at startup or in automated tests; the product UI must
initiate it from a visible user action.

The native bridge also checks the purpose string and exact bundle identifier before it calls
`MusicAuthorization.request()`. A binary launched directly by `cargo run` or a test runner is
therefore rejected without showing a consent prompt. Live authorization must be exercised from a
development-signed `.app` bundle assembled with this `Info.plist`; passing Rust tests does not
qualify the Apple account, MusicKit App Service, subscription, or consent dialog.

## Development app assembly

The live-acceptance bundle requires an installed Apple Development identity. Pass its exact
Keychain name at invocation time; do not store an identity, certificate, profile, key, or account
identifier in the repository:

```sh
AMP808_CODESIGN_IDENTITY="Apple Development: Developer Name" \
  scripts/build-apple-music-dev-app.sh

scripts/check-apple-music-dev-app.sh \
  target/apple-music-dev/AMP808-R3TR0.app
```

An ad-hoc build may verify local assembly, signature integrity, the bound `Info.plist`, and linked
MusicKit bridge symbols. It does not qualify MusicKit or native authorization acceptance:

```sh
AMP808_ALLOW_ADHOC_SIGNING=1 scripts/build-apple-music-dev-app.sh
AMP808_ALLOW_ADHOC_SIGNING=1 scripts/check-apple-music-dev-app.sh \
  target/apple-music-dev/AMP808-R3TR0.app
```

References:

- [Enable MusicKit for an App ID](https://developer.apple.com/help/account/services/musickit/)
- [Automatic developer-token generation](https://developer.apple.com/documentation/musickit/using-automatic-token-generation-for-apple-music-api)
- [`NSAppleMusicUsageDescription`](https://developer.apple.com/documentation/BundleResources/Information-Property-List/NSAppleMusicUsageDescription)
