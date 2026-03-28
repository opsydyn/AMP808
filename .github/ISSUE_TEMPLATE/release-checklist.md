---
name: Release checklist
about: Track an amp808 GitHub release from release PR to uploaded assets
title: "release: amp808 v"
labels: [release]
---

## Release summary

- Target version:
- Release PR:
- Notes / scope:

## Before merging the release PR

- [ ] Release PR has been reviewed
- [ ] Version bump looks correct for the included changes
- [ ] CI is green
- [ ] Generated release notes look sane
- [ ] Release timing is approved

## Release execution

- [ ] Release PR merged to `main`
- [ ] `Release` workflow started successfully
- [ ] GitHub release/tag created
- [ ] Linux asset uploaded
- [ ] macOS asset uploaded
- [ ] Windows asset uploaded

## Post-release checks

- [ ] Opened the GitHub release and checked attached assets
- [ ] Downloaded at least one artifact for a quick sanity check
- [ ] Logged any follow-up fixes or regressions
- [ ] Closed this checklist issue
