# Crash/relaunch draft recovery — backend implementation plan

Contract: [draft recovery backend](../specs/2026-11-20-draft-recovery-backend.md).

1. Add real SQLite tests in `src-tauri/tests/draft_recovery.rs`; verify missing API red. Check reopen/exact Unicode and CRLF, absent sidecar on ordinary reads, excluded backup, late A/B, token+stream+sequence fences, wrong scopes, restore with reused IDs and quotas.
2. Add `src-tauri/src/draft_recovery.rs` for durable scoped storage and classification. Store a lazily available canonical-derived sidecar path and in-process lease in `core::Service`; expose only desktop methods. Keep canonical schema/restore/agent interfaces unchanged. Run focused tests until green.
3. Wire four existing-Store-locked Tauri commands via background blocking workers in `desktop.rs`. Coordinate exact serde/TypeScript contract with frontend implementer. Linux suite covers real core and storage; Windows registration is only inspected, never claimed tested.
4. Verify focused tests, style and changed-file scope independently. Controller owns aggregate runs, review and eventual artifact.
