# Apple Music Capability Status Implementation Plan

> **For agentic workers:** Use `superpowers:subagent-driven-development` or `superpowers:executing-plans`. Execute tasks in dependency order; checkboxes track implementation, not planning approval.

**Goal:** Deliver reliable read-only Apple Music library browsing in the native Music.app TUI, with visible setup, loading, empty, loaded, and failure states.

**Architecture:** Keep configuration, safe API errors, pagination, and Apple DTOs in `src/external/apple_music_api.rs`. Keep request identity and browser lifecycle in `src/ui/mod.rs`; all layouts consume that state. Music.app retains playback ownership, and Apple items never enter the local queue.

**Tech Stack:** Existing Rust 2024 / pinned Rust 1.95.0, Tokio, Reqwest 0.12, Serde, thiserror, Ratatui 0.30, and local HTTP fixture tests. No new dependency or service layer.

**Spec:** [Approved design](../specs/2026-07-14-apple-music-capability-status-design.md), governed by [ADR-0020](../../../adr/0020-reopen-apple-music-api-for-tui-rich-music-app-integration.md).

**Revision:** 2026-11-04. Replaces the July execution steps following the code-grounded review. The accepted product boundary remains read-only metadata plus existing Music.app transport.

## Global Constraints

- Missing credentials remain non-fatal in `--backend music-app`.
- Use `APPLE_MUSIC_DEVELOPER_TOKEN`, `APPLE_MUSIC_USER_TOKEN`, and optional `APPLE_MUSIC_STOREFRONT`. No signing, token issuer, automatic authorization, or credential persistence.
- Keep Apple Music API types and credentials out of `src/player/`. No fabricated playable URLs, catalog search, playlist writes, or playback handoff in this slice.
- Music.app visuals remain synthetic and transport-linked.
- Enter on a track reports `Apple Music playback handoff not implemented yet` without changing playback or the local queue.
- Raw response bodies, token values, and request headers never enter UI state, logs, or error/debug output.
- Use TDD for implementation. Run `cargo test` successfully before every implementation commit, per AGENTS.md.
- Normal tests use fake backends and local HTTP fixtures. Live Apple tests remain ignored and explicitly credential-gated.
- Work on the current `main` branch, honoring the trunk-based workflow. Do not rename branches, create worktrees, push, or stage unrelated edits.

## Resolved Review Decisions

| Concern | Decision |
| --- | --- |
| Pane visibility | Music.app backend selection enables the pane, independently of client availability. |
| State ownership | Replace `apple_music_error` with `apple_music_status`. Keep `prov_loading` for generic providers only. Apple failures never overwrite global playback `err`. |
| Concurrency | One active request, monotonically increasing request IDs, matching-response checks. Back cancels track loading and restores cached playlists. |
| Recovery | 5-second connection timeout, 15-second request timeout, 60-second whole-operation deadline. No automatic retry. `R` retries the failed Apple request. |
| Pagination | Follow playlist and track relationship `next` links. Reject repeated/off-origin cursors; stop with an error before a 101st page. Never report partial data as complete. |
| Layouts | Cover Compact, Drum808, Expanded, and Drum808Expanded. In expanded Music.app mode, combine the existing browser and local-playlist columns into one Apple pane. |
| Live gate | Missing credentials or an unexercised track endpoint mean unvalidated, not success. Publish README setup only after both live checks succeed. |

## Review Focus

1. Missing, blank, malformed, or non-Unicode credentials: safe classification and a visible pane (Tasks 1, 3, 4).
2. Back during loading, empty playlists, repeated Enter, and out-of-order responses: preserve navigation and reject obsolete results (Task 3).
3. Auth/server errors, malformed JSON, stalled servers, and bodies containing token-like text: bounded requests and safe failures (Tasks 1, 2).
4. Pagination, sparse metadata, missing relationships, and later-page failures: no truncated or falsely empty success (Task 2).
5. Narrow terminals and layout switching: readable setup copy, valid focus, unchanged playback and queue (Tasks 3, 4).

## File Map And Preflight

Tasks are sequential because they share interfaces. Review each task before starting its successor. Keep changes in existing modules; no unrelated UI restructuring.

| Task | Files |
| --- | --- |
| 1 | `src/external/apple_music_api.rs`: configuration and diagnostics |
| 2 | Same client and `tests/fixtures/apple_music/`: bounded complete reads |
| 3 | `src/main.rs`, `src/ui/mod.rs`, `src/ui/keys.rs`: lifecycle and interaction |
| 4 | `src/ui/view.rs`, `src/ui/view_808.rs`, `src/ui/view_expanded.rs`, `src/ui/keys.rs`, UI tests: layout parity |
| 5 | Client ignored tests; `README.md` after live validation |
| 6 | Final checks; evidence in this plan and ADR-0020 |

- [ ] Read the approved spec, ADR-0020 and AGENTS.md; inspect `git status --short --branch` and `git log -5 --oneline` before execution.
- [ ] Record baseline `cargo test`, `cargo fmt --all --check`, and `cargo clippy --all-targets --all-features -- -D warnings` results. Distinguish existing failures from regressions; do not format unrelated files.
- [ ] Resolve tooling restrictions before claiming passing checks or committing implementation. The November review encountered `protecting target directories while Cargo runs: Operation not permitted`; this was an environment failure, not a Rust test failure.
- [ ] Preserve pre-existing ADR-0013/0014/index changes and untracked ADR-0020. Do not use `git add .`; consolidate accepted documentation only after the Task 6 diff review.

## Task 1: Deterministic Setup And Redacted Diagnostics

**Files:** `src/external/apple_music_api.rs`, its unit tests.

**Interfaces:** add `AppleMusicSetup::{Ready(AppleMusicClient), Unavailable { missing: Vec<&'static str> }, Error(AppleMusicSetupError)}`. Define cloneable/equality-testable thiserror variants `Missing(&'static str)`, `InvalidHeader(&'static str)`, `InvalidEnvironment(&'static str)`, `InvalidBaseUrl`, and `ClientBuild`. Display only variable names and fixed explanations; never retain credential values or underlying errors as printable sources.

Produce `AppleMusicClient::from_env_status() -> AppleMusicSetup`, private `from_env_reader(read: impl Fn(&str) -> Result<String, std::env::VarError>) -> AppleMusicSetup`, and `from_tokens(config: AppleMusicConfig) -> Result<Self, AppleMusicSetupError>`. Retain `from_env() -> anyhow::Result<Self>` as a compatibility wrapper over the same classifier.

- [ ] **Red: add constructor/reader tests with real assertions.** Reuse `sample_config()`; no environment mutation:

```rust
#[test]
fn apple_music_missing_tokens_are_classified_in_stable_order() {
    let setup = AppleMusicClient::from_env_reader(|_| Err(std::env::VarError::NotPresent));
    assert!(matches!(setup, AppleMusicSetup::Unavailable { missing }
        if missing == vec!["APPLE_MUSIC_DEVELOPER_TOKEN", "APPLE_MUSIC_USER_TOKEN"]));
}

#[test]
fn apple_music_malformed_token_fails_without_disclosure() {
    let mut config = sample_config();
    config.developer_token = "private-dev\nvalue".into();
    let error = AppleMusicClient::from_tokens(config).unwrap_err();
    assert_eq!(error, AppleMusicSetupError::InvalidHeader("APPLE_MUSIC_DEVELOPER_TOKEN"));
    assert!(!format!("{error:?} {error}").contains("private-dev"));
}

#[test]
fn apple_music_config_and_client_debug_are_redacted() {
    let config = sample_config();
    assert!(!format!("{config:?}").contains("dev-token"));
    let client = AppleMusicClient::from_tokens(config).unwrap();
    assert!(!format!("{client:?}").contains("user-token"));
    assert!(!format!("{:?}", AppleMusicSetup::Ready(client)).contains("dev-token"));
}
```

- [ ] **Red: complete the input matrix.** Use injected readers to assert each single missing/blank token returns only that missing name; blank storefront yields `None`; valid values yield Ready with `storefront() == Some("gb")`. Inject `VarError::NotUnicode(OsString::from("sensitive-value"))` and assert `Error(InvalidEnvironment(variable))` without the value in Display/Debug. Repeat malformed-header assertions for the user token. Migrate the existing environment-mutating tests to this seam and remove the unused mutex.
- [ ] Run `cargo test -p amp808 external::apple_music_api::tests`. Confirm the new tests fail for absent behavior/symbols before implementation.
- [ ] **Green:** read each production variable once through `|name| std::env::var(name)`. Classify invalid Unicode explicitly; collect all absent/blank required names. Validate header characters eagerly, mark credential headers sensitive, and implement redacted Debug for config/client. Validate base URLs; use `Client::builder().build()` and map its failures to `ClientBuild`.
- [ ] Run focused tests again, then `cargo test` and `git diff --check`. Expected: all pass. Stage only the client and commit `feat: classify Apple Music setup safely`.

## Task 2: Safe, Bounded And Complete Metadata Reads

**Files:** client and tests; retain the two existing success fixtures unchanged. Add `library_playlists_page_1.json`, `library_playlists_page_2.json`, `library_playlist_tracks_page_1.json`, `library_playlist_tracks_page_2.json`, and `library_playlist_tracks_empty.json` under `tests/fixtures/apple_music/`.

**Interfaces:** introduce `AppleMusicApiError::{DeveloperTokenRejected, UserAuthorizationRejected, RateLimited, Http(u16), Transport, Timeout, Decode, Pagination}` with Clone, PartialEq, Eq, Debug and fixed Display messages. Both fetch methods now return `Result<Vec<LibraryPlaylist>, AppleMusicApiError>` / `Result<Vec<LibraryTrack>, AppleMusicApiError>`. No caller receives raw Reqwest errors.

Use private `from_tokens_with_timeout(config: AppleMusicConfig, request_timeout: Duration) -> Result<Self, AppleMusicSetupError>`; production `from_tokens` delegates with 15 seconds. Connect timeout is 5 seconds; wrap each complete paginated operation in a 60-second Tokio deadline.

- [ ] **Red: expand the fixture server.** Add test-only `MockReply { status: &'static str, body: &'static str, delay: Duration }` and `spawn_scripted_server(replies: Vec<MockReply>) -> (String, oneshot::Receiver<Vec<String>>)`. Read headers through `\r\n\r\n`; serve one complete request per response; bound each server wait to two seconds. Keep existing single-response helpers as wrappers. Add `client_for(base_url: String) -> AppleMusicClient`, using `sample_config()` with that URL.

```rust
#[tokio::test]
async fn apple_music_unauthorized_body_never_reaches_the_error() {
    let (url, _requests) = spawn_response_server(
        "HTTP/1.1 401 Unauthorized", "user-token private response body",
    ).await;
    let error = client_for(url).library_playlists().await.unwrap_err();
    assert_eq!(error, AppleMusicApiError::DeveloperTokenRejected);
    assert!(!format!("{error:?} {error}").contains("user-token"));
    assert!(!error.to_string().contains("private response body"));
}
```

Add named HTTP cases asserting 403 -> `UserAuthorizationRejected`, 429 -> `RateLimited`, 503 -> `Http(503)`, invalid successful JSON -> `Decode`, refused connection -> `Transport`, and a 200 ms delayed response with a 20 ms client timeout -> `Timeout`. The 403 message must allow invalid or insufficient authorization; do not claim expiry is proven.

- [ ] **Red: pin pagination and empty-result semantics.** Copy the existing playlist fixture into the new page-1 fixture and add a `next` to a second page containing `p.playlist-3`. Copy the existing track fixture into its new page-1 fixture and add a relationship `next` to a direct relationship response containing `t.track-3`. Existing single-response tests continue using the unchanged original fixtures. Use these assertions in `apple_music_playlist_pagination_collects_all_items` and `apple_music_track_pagination_preserves_order` after the respective client calls:

```rust
assert_eq!(playlists.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
    vec!["p.playlist-1", "p.playlist-2", "p.playlist-3"]);
assert_eq!(requests.len(), 2);
assert!(requests[1].starts_with("GET /v1/me/library/playlists?offset=2 HTTP/1.1\r\n"));
```

```rust
assert_eq!(tracks.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(),
    vec!["t.track-1", "t.track-2", "t.track-3"]);
assert!(requests[1].starts_with("GET /v1/me/library/playlists/p.playlist-1/tracks?offset=2 HTTP/1.1\r\n"));
```

Further cases: explicit `relationships.tracks.data: []` asserts `tracks.is_empty()`; missing requested relationship asserts `Err(Decode)`; a second-page 503 asserts `Err(Http(503))`, never partial success. Missing artist/album become empty strings; missing ID/title fails parsing. Assert expected dummy auth headers on every successful page.
- [ ] **Red: bound pagination.** Assert repeated cursors and foreign origins yield `Err(Pagination)`, with no request reaching the foreign server. Assert 100 responses still containing `next` produce `Err(Pagination)` and exactly 100 requests. Assert a same-origin relative cursor preserves `/v1` once. Include a redirect response and assert it is reported as `Http(302)` without following it.
- [ ] Run `cargo test -p amp808 external::apple_music_api::tests`; expect meaningful failures, not server hangs.
- [ ] **Green:** replace response-body interpolation in `ensure_success` with status classification and safe fixed errors. Add optional `next` fields to list/relationship DTOs; parse explicit empty relationships separately from missing ones. Preserve initial `?include=tracks`; parse subsequent pages as direct relationship data. Update existing error tests to assert typed outcomes.
- [ ] **Green:** implement private `next_page_url(base: &url::Url, cursor: &str) -> Result<url::Url, AppleMusicApiError>`. Resolve against the API origin and accept only that origin and `/v1/` paths without userinfo/fragments. Disable automatic redirects, track visited URLs, enforce page/operation limits, and return no partial collection on failure. Build playlist IDs with URL path segments, not unescaped interpolation.
- [ ] Run focused tests, `cargo test`, and `git diff --check`. Stage only the client/fixtures; commit `fix: complete and bound Apple Music metadata requests` after all pass.

## Task 3: Deterministic Browser Lifecycle And Interaction

**Files:** `src/ui/mod.rs`, `src/ui/keys.rs`, `src/main.rs`, existing tests.

**Interfaces:** retain the client, track vector, `AppleMusicTrackContext`, and cached `provider_lists`. Replace `apple_music_error` with the status below; add `apple_music_request_seq: u64` and `apple_music_task: Option<tokio::task::JoinHandle<()>>` to App.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppleMusicLoadTarget { Playlists, Tracks }

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppleMusicStatus {
    Unavailable { missing: Vec<&'static str> },
    ClientError(AppleMusicSetupError),
    Loading { request_id: u64, target: AppleMusicLoadTarget },
    Loaded { target: AppleMusicLoadTarget, count: usize },
    Empty { target: AppleMusicLoadTarget },
    ApiError { target: AppleMusicLoadTarget, error: AppleMusicApiError },
}

pub enum AppleMusicItems {
    Playlists(Vec<LibraryPlaylist>),
    Tracks(Vec<LibraryTrack>),
}
```

Replace the three Apple AppMsg variants with `AppleMusicResponse { request_id: u64, result: Result<AppleMusicItems, AppleMusicApiError> }`. Produce these App methods:

```rust
pub fn configure_apple_music_browser(&mut self, setup: AppleMusicSetup);
pub fn fetch_apple_music_playlists(&mut self);
pub fn fetch_apple_music_tracks(&mut self, playlist_id: &str, playlist_name: &str);
fn begin_apple_music_playlists(&mut self) -> Option<u64>;
fn begin_apple_music_tracks(&mut self, context: AppleMusicTrackContext) -> Option<u64>;
fn apply_apple_music_response(&mut self, request_id: u64,
    result: Result<AppleMusicItems, AppleMusicApiError>);
pub fn return_to_apple_music_playlists(&mut self);
pub fn retry_apple_music(&mut self);
```

The `begin_` helpers update state and return identity without spawning or I/O. Fetch wrappers call them and spawn once. This gives lifecycle tests a deterministic seam.

- [ ] **Red: add visibility and navigation assertions.** Reuse `build_music_app_test_app()`. Add `app_with_apple_client() -> App`, attaching an Arc client with dummy tokens and `https://example.test/v1` to the fake backend without configuring/spawning. Convert existing Apple message tests to the new response contract.

```rust
#[test]
fn apple_music_without_tokens_keeps_a_reachable_pane() {
    let mut app = build_music_app_test_app();
    app.configure_apple_music_browser(AppleMusicSetup::Unavailable {
        missing: vec!["APPLE_MUSIC_DEVELOPER_TOKEN", "APPLE_MUSIC_USER_TOKEN"],
    });
    assert!(app.has_provider_pane());
    assert!(app.browsing_apple_music());
    assert_eq!(app.focus, Focus::Provider);
    assert_eq!(app.provider_item_count(), 0);
}

#[test]
fn apple_music_back_from_empty_tracks_restores_playlists() {
    let mut app = app_with_apple_client();
    app.provider_lists = vec![PlaylistInfo { id: "p.1".into(), name: "Favorites".into(), track_count: 0 }];
    let id = app.begin_apple_music_tracks(AppleMusicTrackContext {
        playlist_id: "p.1".into(), playlist_name: "Favorites".into(),
    }).unwrap();
    app.apply_apple_music_response(id, Ok(AppleMusicItems::Tracks(vec![])));
    app.return_to_apple_music_playlists();
    assert!(app.apple_music_track_context.is_none());
    assert_eq!(app.apple_music_status,
        AppleMusicStatus::Loaded { target: AppleMusicLoadTarget::Playlists, count: 1 });
    assert_eq!(app.provider_item_count(), 1);
}

#[test]
fn apple_music_duplicate_and_stale_requests_cannot_replace_state() {
    let mut app = app_with_apple_client();
    let context = AppleMusicTrackContext { playlist_id: "p.1".into(), playlist_name: "One".into() };
    let old = app.begin_apple_music_tracks(context.clone()).unwrap();
    assert!(app.begin_apple_music_tracks(context.clone()).is_none());
    app.return_to_apple_music_playlists();
    let current = app.begin_apple_music_tracks(context).unwrap();
    assert!(current > old);
    app.apply_apple_music_response(old, Err(AppleMusicApiError::Transport));
    assert_eq!(app.apple_music_status,
        AppleMusicStatus::Loading { request_id: current, target: AppleMusicLoadTarget::Tracks });
}
```

- [ ] **Red: complete transitions.** Assert both targets map non-empty results to Loaded(count), explicit empty results to Empty, and errors to ApiError(target). Assert stale success is ignored too. Back from loading/error restores cached lists; retry/success removes the Apple error. A sentinel `app.err` must survive Apple success/failure. Setup errors leave no client/task and retain provider focus. Injected Apple responses cannot change a local fake backend's state.
- [ ] Run `cargo test -p amp808 ui::tests::apple_music`; expect failures before implementation. Tests calling only `begin_` need no runtime; tests calling fetch wrappers must use `#[tokio::test]` and local fixtures.
- [ ] **Green:** make pane availability and Apple identity backend-driven. Default status is Unavailable with both required names; it is hidden for local mode. Every Music.app setup result gets provider focus. Ready starts the initial fetch immediately. Configure resets prior tasks/data; main invokes `from_env_status()` only in the MusicApp branch and removes the old tuple.
- [ ] **Green:** reject starts outside Music.app, without a client, or while Loading. Advance request identity, set target/context before drawing, and clear stale track rows. Apply responses only for Music.app, the active ID, and matching payload kind. Back aborts track work, invalidates Loading, clears track context/rows, resets cursor, and derives Loaded/Empty from cached playlists. Drop completed handles and abort outstanding work when the TUI exits. Apple code never reads/writes `prov_loading` or global `err`.
- [ ] **Red/green: key behavior.** Add `R` scoped to Apple ApiError; retry that target with its existing track context. Enter is inert unless a valid Loaded row is selected; use bounds-checked lookup. Esc/Backspace invokes Back from all track states. Extend the handoff test to assert queue contents/index and fake backend snapshot are unchanged. Assert repeated Enter during Loading leaves request ID unchanged, out-of-range cursors cannot panic, and R has no effect in setup/loading/non-Apple states. Setup/auth copy explains that replaced environment credentials require restart; R does not refresh credentials.
- [ ] Run `cargo test -p amp808 apple_music`, `cargo test`, and `git diff --check`. Stage these three source files and commit `feat: make Apple Music browsing lifecycle deterministic` after all pass.

## Task 4: Render And Navigate All Four Layouts

**Files:** `src/ui/view.rs`, `src/ui/view_808.rs`, `src/ui/view_expanded.rs`, `src/ui/keys.rs`, tests in `src/ui/mod.rs` and existing expanded-view tests.

**Interfaces:** add `App::apple_music_status_lines(&self) -> Vec<String>` (empty for Loaded), and `pub(super) fn render_apple_music_status(&self, frame: &mut Frame, area: Rect, style: Style) -> bool` in `view.rs` (true when status rendered). Expose existing provider renderers as `pub(super)` for expanded reuse.

Exact base copy; slashes separate logical lines:

| State | Copy |
| --- | --- |
| Unavailable | `Apple Music setup (optional)` / `Set missing variables:` / each actual missing variable on its own line / `Restart after setup.` |
| ClientError | `Apple Music setup error` / safe typed error / `Update setup and restart.` |
| Loading/Playlists | `Loading Apple Music playlists...` |
| Loading/Tracks | `Loading Apple Music tracks...` / `Esc: return to playlists` |
| Empty/Playlists | `No Apple Music playlists found.` |
| Empty/Tracks | `No tracks in this playlist.` / `Esc: return to playlists` |
| ApiError | `Apple Music request failed` / safe typed error / `R: retry` plus `Esc: playlists` for tracks |

Auth errors also say `Check credentials and restart.`. Use existing error/normal-text palette for primary copy; dim only secondary hints. Do not communicate state solely by color.

- [ ] **Red: render the full app.** Add `rendered_text(app: &mut App, width: u16, height: u16) -> String` in UI tests, using `Terminal<TestBackend>` and concatenating each row's cell symbols plus newline. No real audio/automation and no manual provider-focus override:

```rust
#[test]
fn apple_music_setup_is_visible_in_all_layouts() {
    for (mode, width, height) in [
        (ViewMode::Compact, 80, 32), (ViewMode::Drum808, 100, 40),
        (ViewMode::Expanded, 120, 44), (ViewMode::Drum808Expanded, 120, 44),
    ] {
        let mut app = build_music_app_test_app();
        app.view_mode = mode;
        app.configure_apple_music_browser(AppleMusicSetup::Unavailable {
            missing: vec!["APPLE_MUSIC_DEVELOPER_TOKEN", "APPLE_MUSIC_USER_TOKEN"],
        });
        let text = rendered_text(&mut app, width, height);
        assert!(text.contains("setup (optional)"), "{mode:?}");
        assert!(text.contains("APPLE_MUSIC_DEVELOPER_TOKEN"), "{mode:?}");
        assert!(text.contains("APPLE_MUSIC_USER_TOKEN"), "{mode:?}");
    }
}
```

- [ ] **Red: draw real transitions.** Render Loading, Empty, Loaded, ClientError and ApiError for both targets; assert status marker, loaded item text and relevant hints. After empty-track -> Back assert `Favorites` returns and `No tracks in this playlist.` disappears. Test 40x32 Compact, 80x32 Drum808, expanded minimum 120x36, and existing below-minimum guards. Required variable names must be readable at supported sizes; tiny rectangles must not panic. Local/generic-provider buffers must contain no Apple copy.
- [ ] Run `cargo test -p amp808 apple_music` and `cargo test -p amp808 ui::view_expanded::tests`. Expect new buffer assertions to fail before wiring.
- [ ] **Green:** shared status rendering precedes generic loading/empty branches. Use `Paragraph::wrap(Wrap { trim: false })` and logical lines; preserve loaded row styles and zero-area guards. Prioritize status and missing-variable names; put secondary hints in existing help/footer space when height is constrained.
- [ ] **Green:** for expanded Music.app views, render the Apple pane over the union of browser/local-playlist rectangles. Preserve now-playing/visualizer areas and local-mode layout. Change `next_focus_expanded(current: Focus, has_eq: bool, apple_music: bool) -> Focus`: Music.app cycles Provider -> Playlist/transport -> Provider without invisible file-browser focus; existing local cycling remains. Normalize focus on layout switches. Update Music.app expanded tests to expect Apple content, and retain separate local tests asserting File Browser and Playlist columns.
- [ ] Run `cargo test`, `cargo fmt --all --check`, and `git diff --check`. Stage only this task's UI files and commit `feat: show Apple Music status across TUI layouts` after all pass.

## Task 5: Live Validation And Documentation Gate

**Files:** ignored client tests; `README.md` only after successful live validation.

**Interfaces:** ignored tests `live_library_playlists_smoke_test` and `live_library_playlist_tracks_smoke_test`; test-only `live_client() -> Option<AppleMusicClient>`. No new credential variables or stored secrets.

- [ ] Add explicit prerequisite handling. `live_client()` maps Ready to a client, Unavailable to None with `SKIP: live Apple Music credentials unavailable`, and setup errors to a safe test failure. Both tests use `#[ignore = "requires Apple Music credentials; run explicitly"]`. A harness `ok` after SKIP is not evidence of live validation.
- [ ] Require actual endpoint exercise. Playlist test accepts an empty library but prints `LIVE_OK: playlists endpoint exercised` only after a successful request. Track test fails with a safe prerequisite message if no playlist exists; otherwise fetch its tracks, even if empty. Assert returned IDs/titles are non-empty and print `LIVE_OK: playlist tracks endpoint exercised` only after that request succeeds. Never print real IDs, titles, credentials or bodies.
- [ ] Run `cargo test -p amp808 live_library` without `--ignored`; expect two ignored tests and zero live requests. Run `cargo test`, then commit only test changes as `test: make Apple Music live validation explicit`.
- [ ] With credentials provisioned, run `cargo test -p amp808 live_library -- --ignored --nocapture --test-threads=1`. Expected validation: exit 0, both LIVE_OK markers, no SKIP. Check availability via the safe setup helper, never token-dumping commands or shell tracing. Missing credentials/playlist prerequisite means pending; API errors mean failed. Continue independent offline checks in Task 6.
- [ ] After both live checks succeed, document the three variables, required/optional status, developer versus user ownership, and `cargo run -- --backend music-app` under Music.app in README. State credentials are manually provisioned in the launching shell; no acquisition/storage is implemented. Document Enter, Esc, R, restart after credential replacement, optional metadata setup, and metadata-only track selection. Storefront is reserved for catalog work and does not alter `/me/library` calls. Link Apple's [developer token](https://developer.apple.com/documentation/applemusicapi/generating-developer-tokens) and [user authorization](https://developer.apple.com/documentation/applemusicapi/user-authentication-for-musickit) guidance after rechecking it. Do not publish unvalidated setup instructions if live validation is pending.
- [ ] Run `git diff --check` and `cargo test` before committing README changes as `docs: document validated Apple Music metadata setup`.

## Task 6: Final Verification And Evidence

- [ ] After the last relevant change, run `cargo fmt --all --check`, `cargo test`, and `cargo clippy --all-targets --all-features -- -D warnings`. Expected exit 0; live tests remain ignored under normal testing. Targeted commands supplement, never replace, full tests before commits.
- [ ] In an interactive terminal run `env -u APPLE_MUSIC_DEVELOPER_TOKEN -u APPLE_MUSIC_USER_TOKEN -u APPLE_MUSIC_STOREFRONT cargo run -- --backend music-app`. Check optional setup copy, Tab reachability, all four layouts, and existing transport controls. If automation/terminal access is unavailable, retain fake-backend coverage and mark the manual check pending.
- [ ] During credentialed manual validation, browse playlists/tracks, return from an empty playlist, retry a transient failure, and confirm track selection changes neither the local queue nor Music.app playback. Record this separately from HTTP smoke tests.
- [ ] Review boundaries with `rg -n 'apple_music_api|AppleMusicClient|AppleMusicConfig|APPLE_MUSIC_' src/player src/playback_backend.rs`. Expected no matches; rg exit 1 is success for this negative check. The existing Music.app transport adapter is allowed. Review the scoped diff for unrelated changes.
- [ ] Append dated evidence under ADR-0020's existing `More Information`: exact commands, pass/fail/blocked, live marker results, manual checks. Do not pre-check acceptance boxes or describe skipped tests as passing. When live validation is blocked, report offline implementation verified with live/README release gates pending.
- [ ] Before a documentation commit, review the pre-existing ADR-0013/0014/index diffs. Stage ADR-0020 with its accepted linkage updates only after confirming their scope; preserve unrelated edits. Record the executed plan/spec with the decision when consolidating documentation. Run `cargo test` before committing; do not push.

## Review Coverage And Later Slices

| November finding | Acceptance evidence |
| --- | --- |
| Hidden no-client pane | Tasks 3-4: backend-driven visibility, initial focus, full-frame assertions |
| Stale navigation / requests | Task 3: Back from empty/error/loading, duplicate suppression, stale success/failure rejection |
| Unsafe errors / Debug | Tasks 1-2: eager validation, redaction and body-omission assertions |
| Fabricated setup-error test | Task 1: malformed header and non-Unicode input through real constructors/readers |
| Stub tests / bad commands / commit gate | Assertions, single Cargo filters and full tests before each implementation commit |
| False live success | Task 5: ignored offline gate, SKIP versus LIVE_OK, mandatory track request |
| Truncated data | Task 2: both pagination response shapes, later-page errors, no partial success |

Catalog search, item enrichment, proven Music.app handoff, discovery views, library writes, and automatic authorization remain later slices.

API details were checked during the November review against Apple's [pagination](https://developer.apple.com/documentation/applemusicapi/fetching-resources-by-page), [request/response handling](https://developer.apple.com/documentation/applemusicapi/handling-requests-and-responses), and [playlist track relationship](https://developer.apple.com/documentation/applemusicapi/libraryplaylists/relationships-data.dictionary/libraryplayliststracksrelationship) documentation.
