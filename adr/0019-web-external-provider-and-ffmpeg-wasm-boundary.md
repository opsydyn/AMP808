---
status: accepted
date: 2026-06-21
decision-makers: alan
---

# Define the web external provider and ffmpeg.wasm boundary

## Context and Problem Statement

AMP808 native/TUI playback supports a broad source surface:

- local and HTTP media decoded by Symphonia, with FFmpeg subprocess fallback for unsupported
  formats (ADR-0003)
- YouTube, SoundCloud, Bandcamp, and other supported pages through two-phase `yt-dlp` resolution
  and lazy local temp-file playback (ADR-0005)
- provider integrations through a native-side provider trait (ADR-0007)

AMP808 Web is a different runtime. ADR-0017 chose a static GitHub Pages-compatible Ratzilla web app
with browser-owned `HTMLAudioElement` playback and Web Audio analysis. ADR-0018 then added a Web
Audio gain/EQ graph after that media element. The web app has no subprocess access, no native
filesystem resolver, no `yt-dlp`, no native FFmpeg, and no server-side proxy that could resolve
provider URLs or repair CORS.

The question is: how should AMP808 Web handle requests for a TUI-like "FFmpeg and SoundCloud"
experience without violating the static web architecture or misleading users about what the
browser can legally and technically do?

## Decision Drivers

- Preserve the accepted static GitHub Pages boundary from ADR-0017.
- Keep browser playback owned by `HTMLAudioElement`, with Web Audio used for analysis and DSP.
- Avoid implying that the browser can run native `ffmpeg` or `yt-dlp` subprocesses.
- Avoid provider scraping, stream ripping, or CORS bypass behavior in the static web app.
- Keep future extension points explicit: local-file transcoding, official provider APIs, or a
  backend-assisted resolver each need different constraints.
- Prefer clear visible errors and handoff paths over silent fake playback or synthetic analyser
  motion.

## Considered Options

- Keep AMP808 Web static-only and support local files plus direct CORS-enabled media URLs.
- Add `ffmpeg.wasm` as a lazy, optional local-file import/transcode path.
- Add SoundCloud/yt-dlp parity directly inside the static browser app.
- Add a backend-assisted provider resolver for SoundCloud and other page URLs.
- Use official provider APIs or embeds where terms allow playback or metadata access.

## Decision Outcome

Chosen option: keep AMP808 Web static-only for playback today, and explicitly separate optional
browser-local transcoding from provider URL resolution.

AMP808 Web will support:

1. Browser-selected local audio files, including drag and drop, through object URLs.
2. Direct hosted audio URLs only when the browser can load them and Web Audio can analyse them under
   CORS.
3. Browser-native `HTMLAudioElement` transport, followed by the existing Web Audio gain/EQ/analyser
   graph.
4. Clear unsupported-provider messaging when a pasted URL is a page/provider URL rather than a
   browser-playable media URL.

AMP808 Web will not support in the static GitHub Pages app:

1. Running native `ffmpeg`.
2. Running `yt-dlp`.
3. Resolving SoundCloud, YouTube, Bandcamp, or other page URLs into media streams.
4. Proxying remote audio or bypassing CORS.
5. Downloading, ripping, scraping, or permanently copying provider-hosted audio.

`ffmpeg.wasm` remains a future option only for browser-local file import/transcode, where the user
has explicitly provided the file to the browser. If added, it must be lazy-loaded, must not become
the primary playback engine, and must not be used to resolve or capture provider streams.

SoundCloud-style playback parity with the TUI requires a separate non-static decision. That future
decision must choose either an official provider/API/embed path or a backend-assisted resolver with
explicit legal, operational, privacy, and deployment consequences.

## Non-goals

- No TUI-equivalent `yt-dlp` support inside the static web app.
- No SoundCloud stream extraction in browser code.
- No CORS proxy in GitHub Pages.
- No custom Web Audio sample scheduler in this decision.
- No `ffmpeg.wasm` dependency in this ADR. This ADR defines when it would be allowed, not that it is
  being added now.
- No change to native/TUI `yt-dlp` or FFmpeg behavior.

## Consequences

- Good, because AMP808 Web remains deployable as static GitHub Pages output.
- Good, because users get honest messaging for SoundCloud/page URLs instead of a broken or
  misleading attempt to play them.
- Good, because local files and direct CORS-enabled audio URLs remain simple, private, and
  browser-native.
- Good, because a future `ffmpeg.wasm` experiment has a narrow boundary: local files only,
  lazy-loaded, and optional.
- Bad, because AMP808 Web will not match native/TUI provider playback parity without adding a
  backend or official provider integration.
- Bad, because some unsupported local formats may remain unsupported until a future local
  transcode/import slice is explicitly approved.
- Neutral, because provider playback becomes a product/deployment decision, not a hidden browser
  workaround.

## Implementation Plan

- **Affected paths**:
  - `docs/ratzilla-web-808-roadmap.md`: add a web external-provider phase that tracks provider URL
    messaging, optional `ffmpeg.wasm` evaluation, and backend/API decision points.
  - `ROADMAP.md`: add the boundary as a tracked Ratzilla Web 808 roadmap item.
  - `README.md`: document that AMP808 Web does not support SoundCloud/yt-dlp page URLs in the
    static build and requires direct CORS-enabled audio URLs.
  - `adr/README.md`: add this ADR to the index.
  - Future implementation, if approved: `crates/amp808-core/src/web_audio.rs` for source
    classification and messages, `apps/web/src/main.rs` for UI handling.
- **Patterns to follow**:
  - Keep direct hosted URL handling tied to browser media/CORS capability.
  - Classify provider/page URLs before load attempts when the URL is recognisable.
  - Show actionable copy: use the native AMP808 app for SoundCloud/yt-dlp pages, or provide a
    direct CORS-enabled audio URL for AMP808 Web.
  - If `ffmpeg.wasm` is evaluated, gate it behind lazy loading and tests for unsupported
    user-provided local files only.
  - Treat any backend/provider resolver as a new ADR, not an implementation detail inside
    `apps/web`.
- **Patterns to avoid**:
  - Do not add `yt-dlp`, native FFmpeg, subprocess, filesystem scanning, or provider resolver
    dependencies to `apps/web`.
  - Do not hide provider failures behind synthetic analyser motion.
  - Do not fetch provider pages from browser code to scrape stream URLs.
  - Do not add a proxy route and still call the deployment "GitHub Pages only".
- **First implementation slice**:
  1. Add pure source classification for likely provider/page URLs.
  2. Add tests for SoundCloud, YouTube, Bandcamp, direct media extension URLs, local file labels, and
     existing CORS error messages.
  3. Render a specific unsupported-provider message in the web UI when a recognised page URL is
     pasted.
  4. Keep the existing direct hosted URL path unchanged for URLs that look like direct media.
  5. Update browser smoke notes for provider/page URL handling.
- **Future optional slice**:
  1. Evaluate `ffmpeg.wasm` bundle size, load time, wasm target compatibility, and browser header
     requirements.
  2. Decide whether a single-threaded local-file-only importer is worth the cost.
  3. Create a new ADR before adding the dependency.

## Verification

- [ ] `cargo test -p amp808-core web_audio::tests` covers provider/page URL classification if that
      implementation slice is started.
- [ ] `cargo test --workspace --locked` passes.
- [ ] `cargo fmt --all --check` passes.
- [ ] `cargo clippy -p amp808_web --target wasm32-unknown-unknown -- -D warnings` passes after any
      web UI implementation.
- [ ] `NO_COLOR=false trunk build --release --public-url /` from `apps/web` succeeds after any web
      UI implementation.
- [ ] Pasting a SoundCloud page URL in AMP808 Web shows a specific unsupported-provider/static-web
      message.
- [ ] Pasting a direct CORS-enabled audio URL still uses the existing browser playback path.
- [ ] Non-CORS direct audio URLs still show the existing CORS/media failure path.
- [ ] The `apps/web` wasm dependency graph still excludes native `ffmpeg`, `yt-dlp`, subprocess, and
      native filesystem dependencies.

## Alternatives Considered

### Static-only browser playback

This keeps AMP808 Web on local files and direct CORS-enabled audio URLs.

- Good, because it preserves GitHub Pages deployment.
- Good, because it matches browser security and media APIs.
- Bad, because provider pages are not playable by URL.

### Optional ffmpeg.wasm for local files

This would use a WebAssembly FFmpeg build to import or transcode user-provided local files that the
browser cannot play directly.

- Good, because it could improve local-file format coverage without uploading files.
- Bad, because it adds large assets, startup cost, worker complexity, and possible cross-origin
  isolation requirements for faster variants.
- Bad, because it still does not solve SoundCloud, `yt-dlp`, or CORS.

### yt-dlp parity inside the browser

This would try to reproduce ADR-0005 in the browser.

- Bad, because browsers cannot run the native `yt-dlp` subprocess model.
- Bad, because it would encourage scraping/extraction behavior from browser code.
- Bad, because it would still fail on provider restrictions and CORS.

### Backend-assisted resolver

This would add a server or serverless component to resolve provider URLs, possibly using official
APIs or native tools outside the browser.

- Good, because it could enable provider playback parity in a controlled environment.
- Bad, because it breaks the static GitHub Pages-only architecture.
- Bad, because it introduces hosting, abuse controls, provider terms, privacy, logging, rate limits,
  and maintenance concerns.
- Neutral, because it remains viable as a future product decision with its own ADR.

### Official provider API or embed path

This would use provider-supported APIs or embeds for SoundCloud-like experiences.

- Good, because it can respect provider contracts.
- Bad, because it may not expose analyser-compatible raw audio or full playback control.
- Bad, because it may require authentication, app registration, or backend token handling.

## More Information

- ADR-0003: Symphonia decode with FFmpeg fallback
- ADR-0005: Two-phase yt-dlp resolution for YouTube/SoundCloud/Bandcamp
- ADR-0007: Provider trait for external music services
- ADR-0017: Add a Ratzilla Web 808 player target
- ADR-0018: Add a Web Audio EQ control graph for browser knobs
