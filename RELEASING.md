# Releasing amp808

This document describes the current maintainer release flow for `amp808`.

## Current release model

`amp808` currently uses:

- [`release-plz`](https://release-plz.dev/) for version bumps, tags, and GitHub releases
- GitHub Actions for cross-platform binary builds and asset uploads
- **GitHub releases only** for now — no crates.io publishing yet

The release behavior is configured in [`release-plz.toml`](release-plz.toml):

- `git_only = true`
- `publish = false`
- `release_always = false`

That means:

- release-plz determines versions from Git tags
- no crate is published to crates.io
- releases happen **only after merging the release PR**

## One-time GitHub repository setup

In the repository settings, enable GitHub Actions to write PRs:

1. Open **Settings → Actions → General**
2. Under **Workflow permissions**, enable:
   - **Read and write permissions**
   - **Allow GitHub Actions to create and approve pull requests**

Without this, release-plz cannot open or update the release PR.

## Normal release flow

1. Merge normal code PRs into `main`
2. The `Release` workflow runs on pushes to `main`
3. release-plz opens or updates a **release PR**
4. Review the release PR
   - expected version bump(s)
   - generated release metadata
   - no accidental changes
5. Merge the release PR
6. The `Release` workflow runs again and:
   - runs Linux release gates (`cargo test --quiet --locked` and `cargo clippy --all-targets --locked -- -D warnings`)
   - creates the Git tag
   - creates the GitHub release
   - builds release binaries for Linux, macOS, and Windows
   - uploads those binaries to the GitHub release

## Important maintainer notes

- **Do not manually bump** the version in `Cargo.toml` for normal releases. release-plz owns the release bump flow.
- Docs-only pushes generally do **not** trigger the release workflow because markdown and demo-image changes are ignored.
- The release workflow uploads artifacts to the GitHub release created by release-plz, so the tag and release are the source of truth.
- The current release name format is `amp808 vX.Y.Z`.
- Use the GitHub issue template at `.github/ISSUE_TEMPLATE/release-checklist.md` when you want a visible release runbook/checklist attached to the repo.

## What to review in the release PR

Before merging, check:

- version bump looks reasonable for the changes included
- only intended package metadata changed
- CI is green
- release timing is sensible

## If a release PR does not appear

Check the following:

- a code change (not only markdown/demo assets) landed on `main`
- the `Release` workflow ran successfully
- GitHub Actions permissions are enabled for PR creation
- there is not already an open release-plz PR being updated

## If the GitHub release exists but assets are missing

Check the `build-release-assets` job in `.github/workflows/release.yml`.

That job runs only when release-plz reports that a release was actually created.

## Manual dry run

There is also a manual GitHub Actions workflow at `.github/workflows/release-dry-run.yml`.

Use it from the Actions tab when you want to validate whether the currently selected ref
**would** produce a release without creating:

- a tag
- a GitHub release
- uploaded assets

The workflow runs `release-plz release` with `dry_run: true` and prints the resulting JSON summary.

Typical use cases:

- sanity-checking release-plz behavior before merging a release PR
- testing release logic after workflow/config changes
- confirming that a ref would not accidentally produce a release

## Future: enabling crates.io publishing

When `amp808` is ready for crates.io, update [`release-plz.toml`](release-plz.toml):

- change `git_only = false`
- change `publish = true`

Then add crates.io authentication using either:

- trusted publishing, or
- `CARGO_REGISTRY_TOKEN`

At that point, re-check the workflow permissions and release-plz settings before the first publish.
