# Author Review Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将逐项作者判断保存为建议证据并强制采纳绑定它。
**Architecture:** 保留单写Service与冻结依据；可信作者保存review，外部只提案。JSON10/SQLite兼容旧证据，不恢复执行授权。
**Tech Stack:** Existing Rust/SQLite/React/Tauri, no model or network dependency.
**Spec:** docs/superpowers/specs/2026-11-18-author-review-design.md

## Global Constraints
仅云；不外发、不接模型、Windows暂停。1..32份必需资料，备注4096UTF8字节/项、review编码64KiB，计现有64MiB证据预算。仅全符合可采纳，原冲突确认仍独立。旧已采纳不伪造review。未知回执先核对，不重复写。

## Review Focus
- 已有无review建议能否绕过新采纳守卫，旧accepted证据是否仍可读。
- review与稿件/包/epoch/配置/版本不一致必须拒绝。
- 缺项/重复/错误ID/多字节转义限额在事务前拒绝。
- 并发或确认时编辑review不能采纳过时结果。
- 备份恢复既保留人工判断又不恢复执行权限。

### Task1: Full author-review vertical slice
**Files:** constrained_writing.rs, lib.rs/desktop.rs/api.ts/writingTypes.ts/WritingPanel.tsx; backup validation/backup_restore; tests, QA fixture and docs.
**Interfaces:** Define typed AuthorReviewItem {material_id,verdict,note}, AuthorReview {revision,items,...binding}; trusted save_constrained_review with expected_revision and epoch, and reviewed decide with expected_review_revision. Existing old decide approve must not bypass. Exact storage implementation chosen after inspecting current evidence validators.
- [x] Establish toolchain/dependencies through allowed official flows; preserve source baseline.
- [x] Write failing core cases for incomplete/invalid/CAS/oldentry/atomic acceptance; implement bounded persistent review and transaction guards.
- [x] Write JSON10/SQLite old/new/archive/limit roundtrips then implement validator/schema changes.
- [x] Add author-only native/API/UI checklist with dirty/confirmation/unknown response protections and focused regressions.
- [x] Verify external MCP/JSON cannot write/accept reviews and existing receipt bounds persist using real pipe scenarios.
- [x] Full tests/build, independent task review and final broad review, fix regressions, document actual limitations.
- [x] Prepare verified source-only archive; replacement uses the reattached Library identity and version guard. Actual upload version is confirmed by Library, not inferred from this plan.
