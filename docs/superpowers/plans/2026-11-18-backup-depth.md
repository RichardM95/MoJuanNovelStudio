# Backup depth implementation plan

> For agentic workers: use superpowers:subagent-driven-development with task and final independent review.

**Goal:** Fix a reproduced cancellation race and prove the accepted original chapter survives real backup/recovery.
**Architecture:** Keep BackupPanel's shared action generation and synchronous lock, add the missing poll-entry guard. Extend the existing original acceptance harness via production BackupJobManager without new product hooks.
**Tech stack:** React/JSDOM, Rust, SQLite, OS stdio pipes.
**Spec:** ../specs/2026-11-18-backup-depth-design.md

## Global constraints
Cloud only. No Windows/native claims, no external manuscripts/models, no user databases. Respect single-writer lock and confirmation/epoch rules. No timing inflation or rerun-as-root-cause claims. Preserve evidence. No new feature scope.

## Review focus
- An old passive effect must not supersede a newly started action; preserve older request invalidation.
- Unknown cancellation result must not trigger repeat writes; status recovery remains possible.
- Post-accept restore must archive author decisions, not revive an old actionable suggestion.
- Main file and recovery snapshot must contain different verified versions; path existence is insufficient.
- A restored library must invalidate old authorization and keep old decisions non-actionable under the new epoch.

## Task 1: BackupPanel race
Files: src/BackupPanel.tsx, tests/BackupPanel.test.tsx. Interfaces unchanged: BackupAPI action/status contract and terminal distinction.
- [x] Deterministic immediately available-button click regression, prove old code drops acknowledgement.
- [x] Minimal synchronous acting guard before poll takes generation; preserve post-action polling.
- [x] Independent unknown-cancel-result regression for running/prepared jobs; guard buttons and handler until successful status read.
- [x] Verify old polls, pending duplicates, failed/successful status recovery, terminal close and late unmount results; focused tests/build.
- [x] Commit and independent task review (45124b3, PASS).

## Task 2: Accepted chapter backup/recovery
Files: src-tauri/tests/fixtures/single_chapter_acceptance.rs and optional adjacent fixture module, docs/ACCEPTANCE.md, docs/VERIFICATION.md, qa/acceptance/README.md. Consume existing original v2 acceptance/history and production BackupJobManager; do not add external agent authority.
- [x] Export actual SQLite, save distinctive author v3, verify baseline history.
- [x] Exercise cancelled or stale candidate and missing writing-guard acknowledgement without mutation, then confirm fresh candidate and observe terminal result.
- [x] Assert restored v2, archive/audit/reference semantics, paused policy, changed epoch and revoked authorization; old decision rejection under both epochs.
- [x] Reopen persisted main database, export exact TXT bytes; open pre-restore snapshot for v3/history and formally prepare it as recoverable candidate, then cancel.
- [x] Focused real harness tests, evidence documentation and scoped commit; independent task review.

## Finish
- [x] Independent whole-branch review and required fixes.
- [x] Full Rust/frontend/build/QA matrix, retain failures and distinguish evidence levels.
- [ ] Replace same Library item, verified source archive with no databases/private logs/build outputs.
