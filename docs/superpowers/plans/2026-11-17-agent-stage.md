# Safe Agent Proposal Stage Implementation Plan

**Goal:** A local agent can read one explicitly authorized work and propose a chapter edit; the user reviews and applies it, and can restore history without losing newer changes.
**Architecture:** One application service owns the SQLite writer. GUI commands and scoped local-agent requests share that service. External operations never include acceptance, deletion, restore, SQL or arbitrary file access.
**Tech Stack:** Existing Rust/Tauri/SQLite/React; local IPC adapter; versioned JSON protocol and constrained MCP stdio compatibility.
**Spec:** ../../SPEC.md and ../../AGENT-STAGE.md

## Constraints
Cloud-only work; baseline archive comment verified as 2eda5b54d2e5aa1c7fa3ac0a75ee436f7f329d21. No external model calls, manuscript transmission, public listening port, copied AGPL code or existing user database manipulation. Windows binary/runtime verification remains a separate gate.

## Review focus
- Editing during a pending proposal must conflict rather than overwrite.
- Duplicate/replayed requests must not create a second proposal or revision.
- A book-scoped client cannot read another book by guessing IDs.
- Restoring the entire library invalidates old sessions and proposals.
- Failed storage transaction must not create partial history or accepted status.

## Task 1: Transactional workflow core
- [x] Add failing tests for chapter history, stale proposal approval, idempotency payload mismatch, history restore conflicts, recycled-chain access, restore epoch invalidation and second-writer refusal.
- [x] Implement immutable proposal/history records, conditional transactional writes, schema migrations and backup v3 compatibility with v1/v2.
- [x] Replace destructive whole-table rewriting with ID-based updates/deletions so retained history survives unrelated mutations.
- [x] Run all Rust tests and independent review before integrating consumers.

## Task 2: Local scoped agent protocol
- [x] Define bounded JSON request envelopes and structured errors, with an explicit MCP protocol version. Test invalid frames, no-session requests, cross-book IDs and forbidden methods before implementation.
- [x] Add an in-process authorization broker sharing the same writer; sessions are book-scoped and revoked on disconnect, user revocation or library epoch change.
- [x] Implement local-only transport and JSON CLI; no database opening in clients. If platform ACL verification cannot be established, fail closed on that platform.
- [x] Add a tools-only MCP stdio adapter that advertises exactly its supported protocol revision and capabilities. Do not claim every MCP revision is supported.

## Task 3: Desktop review and history
- [x] Add failing UI tests for proposal diff, stale approval, rejection, save-before-review, version restore confirmation and conflict retention.
- [x] Add work-scoped review/history panels and explicit before/after content comparison. Session approval/revocation is provided by the trusted Linux terminal host in this stage; desktop connection UI and Windows IPC are deferred.
- [x] Wire trusted desktop-only commands; update the editor from the committed result and rebuild its save queue at the new revision.

## Task 4: Verification and handoff
- [x] Re-run original tests plus new domain/protocol/UI tests, compile production UI and native-core adapter.
- [x] Use official managed preview for interface QA with isolated fixtures; distinguish fixture results from SQLite/native tests.
- [x] Fresh independent code review; resolve serious findings and re-run affected tests.
- [x] Save source checkpoint, build/usage documentation and a complete known-limits report. Do not claim an unbuilt or untested Windows EXE.

## Verification gates not passed
- [ ] Real Unix IPC integration (cloud sandbox returns EPERM on bind; explicit tests remain ignored by default).
- [ ] Windows desktop build, installation, native file dialogs/IME and named-pipe ACLs. Windows IPC is disabled.
- [ ] Live external-model integration (intentionally excluded, no manuscripts sent).
