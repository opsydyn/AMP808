---
status: accepted
date: 2026-11-07
decision-makers: alan
consulted: Apple Music API and MusicKit documentation, cliamp provider prior art
informed: AMP808 contributors
---

# Ship Apple Music as the first desktop media adapter

## Context and Problem Statement

AMP808-R3TR0 is a macOS-first GPUI desktop player with local playback, a persistent local
catalogue, playlists, MilkDrop visualisation, and a cancellable `yt-dlp` integration. Apple Music
is required for the first provider phase and is a product differentiator rather than another PCM
source. Apple owns playback and does not expose decoded samples to AMP808, while authorised
MusicKit clients can browse a subscriber's library, playlists, catalogue, recommendations, and
history and can ask a MusicKit player to play selected items.

During the first playback-handoff compile spike on 2026-11-08, Xcode 26.5's macOS MusicKit module
proved that `SystemMusicPlayer` is explicitly unavailable on macOS. The same module exposes
`ApplicationMusicPlayer` on macOS 14 and later, including queue, prepare, play, pause, stop, seek,
and skip operations. The accepted decision is therefore amended to use
`ApplicationMusicPlayer` for the desktop product. Music.app automation remains separate TUI prior
art rather than a dependency of this GUI.

The Ratatui application already contains useful Apple prior art: ADR-0012 controls `Music.app`,
ADR-0013 introduced a metadata client, and ADR-0020 reopened read-only Apple Music work. That code
must not be imported into the decoupled desktop product or changed during this delivery. The GUI
must first prove its own product and adapter contract. TUI parity is a required follow-up after the
GUI delivery is complete; only then should the project decide whether the proven Apple boundary
belongs in a shared crate.

How should AMP808-R3TR0 add authorised Apple Music metadata, playlist browsing, and deterministic
playback selection without leaking Swift, MusicKit, tokens, Apple DTOs, or GPUI types across the
application's domain boundaries?

## Decision Drivers

- Apple Music is required for the first provider phase and is AMP808-R3TR0's initial provider USP.
- The shipping app must obtain informed user consent instead of requiring users to paste tokens.
- The user's Apple Developer Program membership permits an explicit App ID with the MusicKit App
  Service and automatic developer-token generation.
- Selecting a song or playlist must have a defined playback result, not end at a metadata-only
  browser.
- Apple playback remains external to AMP808's CPAL/Symphonia engine; no false PCM, BPM, EQ, or
  MilkDrop claims are allowed.
- Provider DTOs, authentication, platform FFI, application state, and GPUI presentation must have
  explicit owners.
- The first implementation must not create a speculative shared crate or modify `apps/tui`.
- Production publication, notarisation, and release signing remain deferred, while a
  development-signed bundle is required for live MusicKit acceptance.

## Considered Options

- Add a desktop-private Apple adapter backed by a thin native MusicKit bridge and
  `ApplicationMusicPlayer`.
- Copy the TUI Apple Music REST client and require manually supplied developer and user tokens.
- Import the TUI `PlaybackBackend`, Apple API client, and UI-facing provider model into the desktop.
- Create a shared Apple Music crate before the desktop contract is proven.
- Keep Apple metadata in Rust but continue using only `osascript` for transport.

## Decision Outcome

Chosen option: **add a desktop-private Apple adapter backed by a thin native MusicKit bridge and
`ApplicationMusicPlayer`**, because it gives the desktop product native consent, automatically
managed tokens, typed metadata, and a deterministic playlist-to-playback handoff while keeping
Apple and Swift details outside GPUI and the local audio engine.

1. Add a desktop-owned Apple Music bounded context under `apps/desktop`.
   - Domain values describe authorisation, subscription availability, Apple media references,
     playlists, tracks, paging, artwork descriptors, and playback state.
   - Application state is an explicit enum covering unavailable, unauthorised, authorising,
     loading, loaded, empty, playing, failed, and access-revoked outcomes. Request identity rejects
     stale async responses.
   - IDs and URLs are validated newtypes. Absence uses `Option`; no empty-string or integer
     sentinels represent missing data.
2. Define a narrow, consumer-owned Apple port in Rust. It exposes only the use cases the desktop
   needs: inspect capability, request authorisation, load library playlists, load playlist tracks,
   set an app-scoped MusicKit queue, and issue supported transport commands.
3. Implement the port with a small in-process Swift/MusicKit bridge.
   - Swift and MusicKit types do not cross the boundary.
   - The bridge returns versioned, bounded UTF-8 response envelopes through a C-compatible ABI;
     Rust validates and maps them into domain values.
   - Allocation and release occur on the same side of the boundary. Rust owns a safe RAII wrapper
     around any bridge-owned buffer or handle.
   - Every exported operation has a request ID and exactly one terminal result. Cancellation or
     supersession prevents a late result from replacing current UI state.
   - The bridge never returns developer tokens, music-user tokens, private keys, raw request
     headers, or unredacted Apple response bodies.
4. Use `MusicAuthorization` for consent and MusicKit automatic developer and user-token handling.
   AMP808 does not embed a Media Services private key, generate JWTs, persist a music-user token,
   or ask a user to paste credentials.
5. Use MusicKit's macOS-supported `ApplicationMusicPlayer` for selected Apple songs and playlists.
   This preserves ADR-0012's core boundary—Apple owns the protected playback path rather than
   AMP808's local decoder—while replacing brittle desktop AppleScript selection with an official
   MusicKit queue and transport API. Unlike `SystemMusicPlayer` on Apple mobile platforms, this
   player is app-scoped and does not control Music.app.
6. Keep Apple playback outside `audio.rs`, `playback.rs`, `analysis.rs`, and `milkdrop.rs`.
   - Apple mode exposes no PCM samples.
   - BPM is unavailable.
   - EQ and sample-derived MilkDrop are unavailable.
   - The cosmic underlay may remain ambient but must not claim Apple audio reactivity.
7. Present Apple Music as a distinct desktop library mode with keyboard and VoiceOver access.
   The UI includes consent, unavailable/subscription, loading, empty, loaded, failed, and revoked
   states; playlist and track selection; metadata; artwork where available; and honest capability
   labels.
8. Keep the implementation private to `apps/desktop` through GUI delivery and native acceptance.
   After completion, perform a separate boundary review to decide whether pure Rust domain values,
   the Apple port, and/or the native bridge should move to a shared crate before TUI parity work.
   No shared-crate extraction is pre-approved by this ADR.

### Domain and Boundary Review

- **Apple Music domain** owns Apple media identity, authorisation and subscription state,
  collection navigation, and the meaning of an Apple playback request.
- **Application coordinator** owns request lifecycle, stale-response rejection, retry, selection,
  and conversion from adapter results into desktop state.
- **MusicKit bridge** owns Swift concurrency, MusicKit requests, automatic token use, Apple DTO
  decoding, `ApplicationMusicPlayer`, and platform error translation.
- **Desktop UI** owns presentation, focus, accessible labels, and user commands. It never imports
  Swift, raw FFI, tokens, or Apple response DTOs.
- **Local playback** continues to own CPAL, Symphonia, PCM analysis, BPM, EQ, and projectM. It never
  imports Apple Music domain or bridge types.
- **Local library** owns local files and local playlists. Apple resources are remote references and
  are not silently inserted into SQLite-backed local playlists.
- **Ratatui application** remains unchanged until the desktop delivery is accepted.

Allowed dependency direction:

```text
desktop UI -> Apple application coordinator -> Apple port/domain <- MusicKit bridge
desktop UI -> local playback (existing, separate)
```

The verdict is **safe with noted risk**. The native bridge and development-signing requirements
are new build and runtime boundaries, so the first implementation slice must prove bridge loading,
memory ownership, error redaction, and MusicKit capability before product UI expansion.

### Consequences

- Good, because users authorise Apple access through the native system consent flow.
- Good, because Apple playlists can become playable through the official macOS MusicKit player.
- Good, because automatic token management keeps private keys and pasted tokens out of the app.
- Good, because a typed anti-corruption layer prevents Apple DTOs and Swift types from becoming the
  future Spotify, SoundCloud, or YouTube domain model.
- Good, because the desktop can prove the adapter before the TUI or a shared crate depends on it.
- Bad, because the repository gains a Swift build artifact and an FFI boundary alongside Cargo.
- Bad, because live acceptance requires a MusicKit-enabled App ID, an Apple Music subscription,
  user consent, and a development-signed bundle.
- Bad, because Apple playback cannot drive AMP808's PCM-derived EQ, BPM, or MilkDrop path.
- Neutral, because production release signing and publication remain separate work.

## Non-goals

- No changes to `apps/tui` during the desktop implementation.
- No shared Apple Music crate during GUI delivery.
- No Spotify, SoundCloud, YouTube, or generic multi-provider registry in this implementation.
- No Apple private key, embedded manually generated developer token, pasted music-user token, or
  token-issuing backend.
- No direct Apple stream URL, audio decoding, PCM capture, BPM inference, EQ, or audio-reactive
  projectM input.
- No Apple library mutations, playlist creation, favourites, ratings, or destructive actions in
  the first read-only slice.
- No silent import of Apple items into the local catalogue, queue, or persisted local playlists.
- No production signing, notarisation, distribution packaging, or App Store submission.

## Implementation Plan

### Affected paths

- `apps/desktop/src/apple_music.rs`: private domain values, validated identifiers, state machine,
  port contract, coordinator, typed commands/events, and pure tests.
- `apps/desktop/src/apple_music/macos.rs`: safe Rust wrapper around the native bridge and error
  mapping. This is the only Rust module allowed to contain bridge FFI declarations.
- `apps/desktop/macos/music-kit-bridge/`: Swift source and C-compatible bridge contract for
  authorisation, library playlists, playlist tracks, `ApplicationMusicPlayer`, and transport state.
- `apps/desktop/build.rs`: macOS-only deterministic bridge compilation/linking into Cargo's
  `OUT_DIR`; non-macOS builds expose an unavailable adapter without invoking Swift tools.
- `apps/desktop/Cargo.toml`: register `build.rs`; add `serde = "1"`, `serde_json = "1"`, and
  `thiserror = "2"` if the first failing tests prove the versioned response-envelope boundary.
- `apps/desktop/src/main.rs`: register the private module and construct the production adapter only
  on macOS.
- `apps/desktop/src/app.rs`: add provider-neutral desktop commands and state transitions without
  importing MusicKit or Swift types.
- `apps/desktop/src/ui.rs`: add Apple library mode, consent and recovery UI, playlist/track
  navigation, metadata/artwork presentation, keyboard behavior, and accessibility semantics.
- `apps/desktop/macos/Info.plist`: declare the stable bundle identifier and a concise
  `NSAppleMusicUsageDescription` explaining playlist, metadata, and playback access.
- `apps/desktop/macos/AMP808-R3TR0.entitlements`: add only MusicKit-related capability material
  proven necessary by the development profile; do not enable App Sandbox implicitly.
- `.moon/tasks.yml` or the existing desktop Moon task configuration: make bridge checks explicit
  without replacing Cargo as build authority.
- `adr/README.md`: index this decision and record acceptance evidence after delivery.

### Dependencies and configuration

- Use Apple's installed Swift toolchain and MusicKit framework; do not add a third-party Apple Music
  SDK or FFI generator in the first slice.
- Add Rust serialization/error dependencies only at the adapter boundary and keep them out of
  `amp808-core`.
- Register an explicit App ID matching AMP808-R3TR0's bundle identifier and enable the MusicKit App
  Service in Certificates, Identifiers & Profiles.
- Development acceptance uses that App ID and a development signing identity. Production identity,
  notarisation, and publication remain deferred.

### TDD delivery slices

1. **Pure Apple domain contract**
   - Write failing tests for legal authorisation/loading/playback states, stale request rejection,
     validated IDs, pagination, empty results, denial, revocation, and retry.
   - Implement the smallest desktop-private Rust domain and fake port that passes them.
2. **Bridge contract spike**
   - Write failing ABI contract tests for version negotiation, bounded UTF-8 envelopes, exactly-once
     completion, invalid payloads, memory release, redacted errors, and unavailable capability.
   - Compile a minimal Swift bridge and prove an unauthorised status request in a development bundle
     before adding broader MusicKit calls.
3. **Authorisation and subscription**
   - Add the usage description and MusicKit-enabled development identity.
   - Cover not-determined, denied, restricted, authorised, unsubscribed, revoked, and bridge-failure
     states without blocking GPUI.
4. **Read-only playlists and metadata**
   - Fetch all library playlist pages and selected playlist-track pages.
   - Map identifiers, names, artists, albums, durations, genres, artwork descriptors, catalog
     equivalence, editability, and missing fields into Rust domain values.
   - Reject repeated/off-contract pagination and never report a partial page sequence as complete.
5. **MusicKit playback handoff**
   - Set `ApplicationMusicPlayer` queues from a selected song or playlist and expose prepare, play,
     pause, next, previous, stop, repeat, shuffle, current entry, duration, position, and failure.
   - Keep local playback and Apple playback mutually explicit; switching modes tears down only the
     outgoing coordinator and never fabricates PCM state.
6. **GPUI product surface**
   - Add accessible library navigation, metadata, artwork, loading/progress, denial/recovery, and
     transport controls using the existing AMP808-R3TR0 skin.
   - Preserve all local-library, queue, visualiser, keyboard, and VoiceOver behavior.
7. **Native acceptance and handoff**
   - Validate a normal authorised account and denied/revoked/error paths in the same
     development-signed bundle.
   - Record observed metadata, playlist paging, selection playback, relaunch, focus, keyboard,
     VoiceOver, and non-PCM capability labels separately.
   - After GUI acceptance, create a new bounded TUI-parity plan and review whether sharing the Apple
     boundary is now justified.

### Patterns to follow

- Follow `apps/desktop/src/remote.rs` and `remote_transfer.rs` for typed lifecycle events and
  cancellation/stale-result discipline.
- Follow `apps/desktop/src/preset_folder/macos.rs` for a platform-specific implementation hidden
  behind a safe Rust boundary.
- Use `thiserror` for adapter/library errors and application-level context only at the composition
  boundary.
- Hand-write `Debug` for any type that could contain a credential, header, Apple diagnostic, or
  user-library payload requiring redaction.
- Keep public/shared signatures free of `Arc`, `Mutex`, GPUI entities, Swift handles, and foreign
  framework types.
- Keep bridge buffers bounded and validate all lengths, versions, discriminants, and UTF-8 before
  deserialisation.

### Patterns to avoid

- Do not extend the TUI's three-method `Provider` trait to cover Apple playback.
- Do not copy Apple API DTOs into domain types or use an untyped metadata map.
- Do not block the GPUI thread on Swift concurrency, HTTP, artwork, or player preparation.
- Do not let the Swift bridge call GPUI or local playback directly.
- Do not log tokens, headers, raw Apple bodies, complete user playlists, or private library IDs.
- Do not use a shell command, `osascript`, private Apple frameworks, or fabricated Apple stream
  URLs for selected-item playback.
- Do not extract a shared crate merely to make paths symmetrical.

## Verification

- [x] The Apple domain/state tests fail before implementation and pass after each TDD slice.
- [x] Apple domain and application modules import no GPUI, Swift, FFI, `reqwest`, CPAL, Symphonia,
      projectM, or SQLite types.
- [x] Raw bridge declarations exist only in `apps/desktop/src/apple_music/macos.rs`; every unsafe
      block documents its safety invariant and has adversarial length/version/null tests.
- [ ] Missing Swift tooling or unavailable MusicKit produces a typed build/runtime diagnostic, not
      an unexplained linker failure or panic.
- [ ] The app starts and local playback remains fully functional when Apple access is unavailable,
      denied, restricted, unsubscribed, revoked, offline, or rate-limited.
- [x] The native consent dialog uses the configured `NSAppleMusicUsageDescription`; no product UI
      requests a developer token, music-user token, private key, team ID, or key ID.
- [x] All playlist pages load in order; repeated or malformed cursors fail without
      presenting partial data as complete.
- [x] Selecting a playlist loads all ordered playlist entries through validated pages and presents
      a virtualized, fuzzy-searchable, read-only track browser with honest missing metadata.
- [x] Selecting a playable playlist entry sets the app-scoped MusicKit queue and begins playback
      through `ApplicationMusicPlayer`; the preparing and playing states remain observable after
      the request completes. Pause, resume, and stop use the same typed, identity-checked
      transport boundary and expose observable transitional and terminal states.
- [x] Apple mode labels BPM, EQ, waveform, and audio-reactive MilkDrop as unavailable and never
      submits synthetic samples to the analysis or projectM pipelines.
- [ ] Keyboard-only and VoiceOver users can authorise, browse playlists, choose tracks, recover from
      failure, and operate transport controls.
- [x] `apps/tui` has no implementation diff in this delivery.
- [x] Desktop base and projectM test suites, workspace tests, strict focused Clippy, formatting,
      bridge tests, and `git diff --check` pass.
- [ ] A development-signed native acceptance records authorised, denied/revoked, offline/failure,
      relaunch, playlist paging, selected playback, keyboard, VoiceOver, and capability-label
      evidence separately.
- [ ] Only after GUI acceptance does a separate boundary review decide whether to create a shared
      Apple Music crate for TUI parity.

## Pros and Cons of the Options

### Desktop-private MusicKit adapter with `ApplicationMusicPlayer`

- Good, because it uses Apple's native consent and automatic token management.
- Good, because a selected playlist or song maps to an official playback queue.
- Good, because it can later expose a proven contract to the TUI without coupling the two UIs now.
- Bad, because Swift build/linking, FFI memory ownership, and signed capability testing are new.

### Raw Apple Music REST client with manual tokens

- Good, because the TUI already demonstrates the HTTP and fixture-testing approach.
- Good, because most implementation could remain Rust-only.
- Bad, because a pasted music-user token is not an acceptable shipping authorization experience.
- Bad, because direct REST metadata still does not provide a deterministic selected-item playback
  implementation.

### Import the TUI backend and provider model

- Good, because it could reuse existing code quickly.
- Bad, because it violates the desktop product boundary from ADR-0021 and ADR-0022.
- Bad, because the TUI `Provider -> Track` contract assumes playable URLs and does not model Apple
  authorization or system transport.

### Create a shared Apple Music crate immediately

- Good, because both frontends could eventually use one implementation.
- Bad, because only the GUI shipping contract is currently authorised and proven.
- Bad, because premature extraction could freeze TUI, GPUI, Swift, or token assumptions into a
  public workspace contract.

### Keep AppleScript transport with new metadata

- Good, because the TUI has working AppleScript prior art.
- Bad, because mapping an arbitrary catalog or playlist selection to a Music.app script is less
  deterministic than setting an app-scoped MusicKit queue.
- Bad, because it would retain two Apple integration mechanisms after introducing MusicKit for
  authorization and metadata.

## More Information

Related decisions:

- ADR-0012: Add a macOS Music.app playback backend.
- ADR-0013: Use the Apple Music API as a metadata-only integration.
- ADR-0020: Reopen Apple Music API for TUI-rich Music.app integration.
- ADR-0021: Adopt a Moon-orchestrated Cargo monorepo layout.
- ADR-0022: Add the macOS-first AMP808-R3TR0 desktop player with GPUI Kit.
- ADR-0031: Stage remote media acquisition behind a provider-agnostic desktop job.
- ADR-0032: Run `yt-dlp` behind a cancellable desktop transfer adapter.

Implementation evidence recorded 2026-11-07 for the bounded authorization slice:

- The GPUI header exposes a visible, labelled Apple Music control. Authorization starts only from
  its click or keyboard activation; startup and render paths do not start the bridge.
- `AppleMusicCoordinator` owns request identity, bridge task lifetime, retry, terminal mapping, and
  stale-result rejection. GPUI imports the coordinator and domain presentation only; the raw FFI
  declaration remains confined to `apple_music/macos.rs`.
- The Swift bridge validates the exact bundle identifier and usage description before calling
  `MusicAuthorization.request()`, then reports one versioned terminal connection envelope. An
  unbundled native test proves this guard returns without prompting.
- Focused Apple tests passed 30/30, strict focused Clippy passed with warnings denied, the desktop
  suite passed 194/194, and the workspace passed 526 tests with 3 live-only tests ignored.
- Live authorization in a MusicKit-enabled, development-signed `.app`, account/subscription
  behavior, consent UI, and VoiceOver observation remain unqualified and are the next native gate.

Native delivery evidence recorded 2026-11-07:

- The explicit App ID `com.opsydyn.amp808r3tr0` was registered in the Apple Developer portal with
  only the MusicKit App Service selected. Reopening its configuration showed MusicKit still
  enabled. No Media ID, private key, manually generated token, or speculative entitlement was
  created.
- `build-apple-music-dev-app.sh` assembles the Cargo binary and checked-in `Info.plist` into a
  staged `.app`, signs it with an explicitly supplied identity, validates it before promotion, and
  preserves an existing output bundle rather than deleting it.
- `check-apple-music-dev-app.sh` validates the exact bundle identity, executable, usage
  description, signature integrity, and linked MusicKit request symbol. It rejects ad-hoc signing
  by default; an explicit opt-in exists only for assembly testing.
- An ad-hoc bundle passed the assembly validator when opt-in was present and was rejected by the
  development validator without it. This proves packaging behavior, not MusicKit authorization.
- Development signing remains blocked because Keychain contains no valid code-signing identity
  and Xcode could not retrieve the configured development team. Certificate recovery, a genuinely
  development-signed bundle, and live authorization remain open evidence gates.

Native authorization evidence recorded later on 2026-11-07:

- After the Xcode account refresh, Xcode retrieved the OPSYDYN LTD development team and created an
  Apple Development certificate and matching private key. Installing Apple's official Worldwide
  Developer Relations G3 intermediate certificate made that identity valid for code signing.
- `build-apple-music-dev-app.sh` produced a non-ad-hoc, development-signed
  `AMP808-R3TR0.app`. The repository validator passed, the signature identifier is
  `com.opsydyn.amp808r3tr0`, a development team identifier is present, and `codesign` reports the
  two expected file-access entitlements.
- Launching that bundle and activating its labelled Apple Music control exercised the native
  authorization path. The UI progressed from `NOT CONNECTED` through `WAITING FOR AUTHORISATION`
  to `CONNECTED / CATALOG + LIBRARY`; macOS recorded full Media Library authorization for the
  exact application identifier.
- This qualifies development signing, bundle identity, live authorization, and subscription
  capability discovery only. Denied/revoked recovery, relaunch persistence, playlist paging,
  selected playback, keyboard navigation, VoiceOver, and the full non-PCM capability-label pass
  remain separate native acceptance gates.

Read-only playlist-gallery evidence recorded on 2026-11-08:

- Apple sign-in now belongs to a dedicated GPUI Settings / Accounts surface. The signed app
  exposes not-connected, authorising, connected, loading, loaded, and retry states with labelled
  controls and status semantics; the player header links to Settings rather than owning consent.
- After native authorization, the coordinator fetched all 170 playlists from the live account
  across multiple `MusicLibraryRequest<Playlist>` pages before exposing the collection. The UI did
  not present a partial page sequence as a completed library.
- The first live response exposed artwork outside the HTTPS resource invariant. The boundary was
  refined test-first: opaque playlist identity and cursor progression remain strict, while invalid
  optional artwork is omitted and rendered with the existing honest placeholder instead of
  discarding the entire library. Swift also omits artwork that is not a hosted HTTPS URL before
  crossing FFI. No URL, playlist identifier, or provider payload was logged for diagnosis.
- The final development-signed build displayed the 170-entry gallery with accessible image and
  selection labels. Selecting one entry changed its accessibility label to the explicit selected
  state. This qualifies read-only paging, gallery presentation, and selection only; playlist-track
  loading, MusicKit playback, manual keyboard/VoiceOver acceptance, denial/revocation, and relaunch
  persistence remain open.

Selected-playlist track-browser evidence recorded on 2026-11-08:

- MusicKit playlist entries are loaded through the playlist relationship and its documented
  `MusicItemCollection` pagination. The versioned FFI envelope preserves opaque entry identity,
  playlist identity, order, duplicates, title, artist, optional album, optional duration, request
  identity, and cursor progression; malformed identifiers, repeated cursors, wrong-playlist pages,
  stale selections, and provider failures remain typed and redacted.
- The GPUI Settings surface now uses a master/detail flow. The bounded playlist gallery is replaced
  on selection by a visible track browser with a `CHANGE PLAYLIST` recovery action, fuzzy search
  across title/artist/album, honest `Unknown album` and `--:--` fallbacks, and a virtualized result
  list with status and list-item accessibility semantics.
- The development-signed app loaded a real selected playlist containing 48 entries and exposed
  ordered title, artist, album, and duration metadata. A live multi-term artist query reduced the
  status to `1 of 48 tracks shown`, and the return action restored the playlist gallery. No
  playback or library mutation was performed.
- Focused Apple and GPUI tests pass, strict desktop Clippy passes with warnings denied apart from
  the repository's explicit dead-code allowance, formatting and `git diff --check` pass, and the
  full workspace passes 611 tests with 3 live-only tests ignored. The signed bundle validator also
  passes.
- This qualifies read-only selected-playlist loading, pagination, search, virtualization,
  master/detail recovery, and live metadata display. `ApplicationMusicPlayer` handoff, manual
  keyboard/VoiceOver acceptance, denial/revocation, offline/failure, and relaunch persistence
  remain separate gates.

Selected-entry MusicKit playback evidence recorded on 2026-11-08:

- Xcode 26.5's macOS MusicKit interface and the bridge compile proved `SystemMusicPlayer` is
  unavailable on macOS while `ApplicationMusicPlayer` is supported from macOS 14. ADR-0035 now
  records the supported app-scoped player rather than carrying the mobile-only assumption.
- The first signed spike could load an individual `Playlist.Entry` but failed during MusicKit
  playback preparation. The playback request was refined test-first to preserve both the source
  playlist identity and selected entry identity. Swift reloads that playlist relationship and uses
  `ApplicationMusicPlayer.Queue(playlist:startingAt:)`, Apple's playlist-specific initializer.
- The signed app exposed each selected-playlist result as a keyboard- and accessibility-visible
  play button. Native acceptance observed the selected entry move from `Preparing Apple Music
  playback` to `Playing through Apple Music`; the Swift bridge emits playing only after both
  `prepareToPlay()` and `play()` succeed. The app was then closed to stop the app-scoped test
  playback.
- Bridge responses remain versioned and bounded, request and track identities are checked before
  state changes, newer requests supersede unfinished ones, and provider failures are redacted at
  the Rust boundary. Local PCM, BPM, EQ, and audio-reactive MilkDrop remain explicitly unavailable.
- This qualifies one-entry selection and playback start. Next/previous, position observation,
  queue navigation, whole-playlist activation, failure recovery, and manual keyboard/VoiceOver
  acceptance remain separate slices.

Apple pause/resume transport evidence recorded on 2026-11-08:

- The coordinator now owns typed pause and resume commands and exposes `Pausing`, `Paused`,
  `Resuming`, and `Playing` states. Request and track identity must match the active operation, and
  an inconsistent terminal status fails closed instead of silently mutating playback state.
- One versioned `transport` bridge envelope carries only request identity, track identity, and the
  validated `paused` or `playing` result. Native failure detail is redacted at the Rust boundary;
  Swift uses the existing app-scoped `ApplicationMusicPlayer` for both operations.
- GPUI exposes `PAUSE APPLE` only while playing and `RESUME APPLE` only while paused. Both are
  native buttons with explicit accessibility labels, while transitional state remains visible and
  non-interactive.
- Live development-signed acceptance observed `Playing through Apple Music` change to
  `Apple Music playback paused`, with the action changing from Pause to Resume. Resuming returned
  the status to Playing and restored the Pause action. The app was terminated after the check to
  stop app-scoped playback.
- Focused coordinator, bridge-parser, redaction, and GPUI tests pass. Strict desktop Clippy passes
  with warnings denied apart from the repository's explicit dead-code allowance; the full locked
  workspace passes 619 tests with 3 ignored, and the signed bundle validator passes.
- This qualifies observable pause/resume and accessible pointer activation. Next/previous,
  position observation, queue navigation, whole-playlist activation, failure recovery, and manual
  keyboard/VoiceOver acceptance remain separate slices.

Apple Stop transport evidence recorded on 2026-11-08:

- The existing typed transport command and response boundary now includes Stop and the validated
  `stopped` result. The coordinator exposes `Stopping` until the response matches both request and
  track identity, then preserves the selected entry in an observable `Stopped` state. A wrong
  terminal status fails closed as a playback error.
- Stop is valid from both Playing and Paused. GPUI exposes a distinct `STOP APPLE` button with the
  accessibility label `Stop Apple Music playback` in both states, removes transport controls after
  completion, and labels the selected entry and status as stopped.
- Live development-signed acceptance observed Playing with Pause and Stop controls, then
  `Apple Music playback stopped` after activating Stop. Both active transport controls disappeared,
  and the selected entry remained visibly labelled stopped. The app was closed after acceptance.
- Focused state-machine, inconsistent-response, bridge-parser, and GPUI tests pass. Strict desktop
  Clippy passes with warnings denied apart from the repository's explicit dead-code allowance; the
  full locked workspace passes 622 tests with 3 ignored, and the signed bundle validator passes.
- This qualifies explicit termination of app-scoped Apple playback. Next/previous, position
  observation, queue navigation, whole-playlist activation, failure recovery, and manual
  keyboard/VoiceOver acceptance remain separate slices.

Official Apple references checked through 2026-11-08:

- <https://developer.apple.com/help/account/services/musickit/>
- <https://developer.apple.com/documentation/musickit/musicauthorization>
- <https://developer.apple.com/documentation/musickit/using-automatic-token-generation-for-apple-music-api>
- <https://developer.apple.com/documentation/applemusicapi/get-all-library-playlists>
- <https://developer.apple.com/documentation/musickit/playlist/entries>
- <https://developer.apple.com/documentation/musickit/musicitemcollection>
- <https://developer.apple.com/documentation/musickit/applicationmusicplayer>
- <https://developer.apple.com/documentation/musickit/systemmusicplayer>
- <https://developer.apple.com/documentation/musickit/musicplayer/queue>
- <https://developer.apple.com/documentation/bundleresources/information-property-list/nsapplemusicusagedescription>

Implementation evidence recorded on 2026-11-07:

- The desktop-private Rust domain contract and pagination guard were implemented test-first.
- A Swift 6.3 `@c` bridge now returns a versioned, bounded authorization-capability envelope. The
  generated C header confirms the Rust ABI declarations, and a linked host test round-trips request
  identity through `MusicAuthorization.currentStatus`.
- Rust owns bridge buffers through a non-cloneable RAII wrapper while Swift performs allocation and
  release. Adversarial tests cover null pointers, oversized lengths, invalid UTF-8, malformed JSON,
  unsupported versions, unexpected kinds, unknown statuses, error redaction, and exactly-once
  release.
- This is host-process evidence only. Development-signed app-bundle authorization, subscription,
  consent, and revocation acceptance remain open for the next delivery slices.
- The permanent desktop identifier is now recorded as `com.opsydyn.amp808r3tr0` in a linted
  `Info.plist`, with an `NSAppleMusicUsageDescription` that names playlist browsing, metadata, and
  Apple Music playback. A repository check guards both values. App ID registration and enabling
  the MusicKit App Service remain explicit Apple Developer portal work rather than a fabricated
  entitlement.
- Passive `MusicAuthorization.currentStatus` values now map into distinct not-determined, denied,
  restricted, authorised, and unknown domain states without entering the consent-requesting state.
  Subscription inspection has independent catalog-playback, offer, and cloud-library capabilities;
  query failure does not revoke authorised library access. A versioned subscription wire envelope
  is covered, but the native async `MusicSubscription.current` call is not yet exported.
- Focused Apple tests (21), the desktop suite (185), strict focused Clippy, plist validation,
  formatting, and the full workspace suite (517 passed, 3 ignored live tests) passed. `apps/tui`
  has no implementation diff from this delivery.

Revisit this decision if MusicKit automatic token generation cannot be made reliable from the
Rust-hosted application bundle, if the C-compatible bridge cannot meet the memory and error
contract without private API, if `ApplicationMusicPlayer` cannot play the required library/catalog
items, or after accepted GUI delivery when TUI parity provides evidence for or against a shared
crate.
