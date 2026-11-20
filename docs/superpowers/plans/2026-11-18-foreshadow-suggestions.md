# Foreshadow Suggestions Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** 已保存正文中的伏笔引用由智能体提交建议，作者可审阅并安全登记。
**Architecture:** 独立不可变建议和决定，可信采纳与正式出现创建同事务；既有Broker仅追加review-only接口，备份保留历史而不恢复可执行权限。
**Tech Stack:** Rust/rusqlite/React/Tauri，既有本地工具链，无新网络依赖。
**Spec:** docs/superpowers/specs/2026-11-18-foreshadow-suggestions-design.md

## Global Constraints
仅云端虚构数据；不接模型、不外发、不用用户本机、Windows暂停。quote4096/note1024/reason2048 UTF8字节，单建议编码64KiB；每卡20pending/全库100pending/1000全状态/32MiB存储JSON（record+payload）。正式出现原有限额不变。提交只写待审，不改正文/卡片/状态；只有可信人工确认才能登记正式引用。没有语义自动判定。

## Review Focus
- 来源已移动/同正文不同revision/卷重排仍须阻止旧建议，不能找同句自动通过。
- 并发、迟到或未知回执不能重复登记或将旧库建议写进新库。
- 拒绝不依赖来源可用，历史源删除后引文必须仍在。
- 备份缺新表、旧格式夹带新字段、矛盾accepted引用必须替换前拒绝。
- UI网络读取失败不能丢失已采纳回执，私有字段不能通过协议泄漏。

### Task 1: Complete review-only occurrence suggestion slice
**Files:** Create src-tauri/src/foreshadow_proposals.rs, src-tauri/tests/foreshadow_proposals.rs, src/ForeshadowProposalPanel.tsx, tests/ForeshadowProposalPanel.test.tsx. Modify foreshadow.rs transactional helper, lib.rs/core.rs/backup_restore.rs/agent.rs/agent_stdio.rs/desktop.rs; src/api.ts/foreshadowTypes.ts/ForeshadowPanel.tsx and relevant App/QA fixture integration; protocol tests/docs/VERIFICATION.
**Interfaces:** Domain types ForeshadowProposalInput/ForeshadowProposal/ForeshadowProposalReceipt, methods propose/get/cancel under injected client+book+epoch, trusted paginated list/detail/decide with explicit epoch and expected revision. Exact Rust signatures should follow established Service conventions and be defined consistently in core/desktop/frontend. Public method names and payload are fixed in spec. Acceptance reuses new crate-private in-transaction occurrence creation helper shared with existing trusted create, preserving its observable validation and idempotency. No generic arbitrary insert interface to external callers.
- [x] Write failing isolated core tests for exact quotes, identity/scope/limits, submit immutability, idempotency, source and card conflict, accepted transaction rollback, parentrevision pack invalidation, rejection after source removal and archived epoch denial. Run focused cargo test and verify failures are actual missing behavior.
- [x] Implement proposal schema/serialization, bounded validation and review-only lifecycle; refactor only needed shared transactional helper. Green focused tests and existing foreshadow/constrained suites.
- [x] Write RED migration/JSON13/SQLite roundtrip, missing current table, malformed aliases/status/accepted projection, history after purge and restoration archive cases. Implement validation before replacement, retain legacy compatibility and preserved restore snapshot guarantees; run GREEN.
- [x] Extend Broker and JSON/MCP registries to three methods using injected identity, deny_unknown_fields, bounded receipts. Add actual subprocess cases for samebook/getown/cancel/duplicate/refusedwrite; run real pipe harness and assert no formal occurrence created without trusted approval.
- [x] TDD author pending/detail/history view and exact-confirmation flow; include repeated clicks, stale source, reject available, partialload error, lost/mismatched receipt, sameepoch readback and changedepoch no-write, editor flush integration. Use existing modal safety patterns, do not duplicate manuscript buffers.
- [ ] Run full cargo test/build --locked --offline, npm test/build, QA fixture tsc. Update interface docs with actual tool/scenario counts and honest limits. Self-review, git diff --check, commit and report.
- [ ] Controller independent task review then final review; fix scoped findings through implementer, rerun complete checks; source-only ZIP replace Library v3 with guarded nextversion.
