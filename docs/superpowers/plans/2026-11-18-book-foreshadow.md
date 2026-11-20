# Book Foreshadow Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** 全书伏笔总览与跨章多次出现、引用来源及版本追踪。
**Architecture:** 复用foreshadow材料主体和作者状态，独立不可变出现记录/可恢复生命周期；写作包冻结并重核出现，外部仅书级只读分页。
**Tech Stack:** Existing Rust/SQLite/React/Tauri; no model or external manuscript transfer.
**Spec:** docs/superpowers/specs/2026-11-18-book-foreshadow-design.md

## Global Constraints
云端，不用本机、不传稿、不接模型、Windows暂停。quote4096B/note1024B/单条编码32KiB；每卡32含回收，全库10000/32MiB；10MiB章读取预检，包4MiB/总64MiB沿用。作者决定角色与回收状态，派生状态只表示来源新旧。旧内容不虚构/不静默删除。

## Review Focus
- 同一卡跨卷多次记录，当前排序与历史位置不能混淆。
- 章节删除仍保留引文，回收恢复不把旧引文标current。
- 记录集合变化与source变动都能阻止旧写作包。
- JSON12/旧SQLite/冻结证据保持完整但不恢复运行权限。
- 新建未知回执幂等与旧epoch读取，不串书或重复写入。

### Task1: Full book foreshadow vertical slice
**Files:** new foreshadow domain/UI/types/tests; materials/lib/core/backup_restore/constrained_writing/agent/agent_stdio/desktop/api/App integration and QA/docs.
**Interfaces:** ForeshadowOccurrence plus summary/page types; trusted create(book,material,chapter,chapter_revision,range,quote,role,note,epoch,key), list/get, trash/restore(expected_revision,epoch). Exact serialization chosen consistent with spec. Add trusted body-free directory and bounded chapter read if existing APIs insufficient. External list-only method; no mutation. Book dashboard uses existing material lifecycle editor for primary source/payoff/status.
- [x] Recover official toolchain/dependencies, preserve verified baseline, write failing exactquote/CAS/idempotency/source-status/limit tests.
- [x] Implement transactional occurrences and parentrevision invalidation, review current/frozen sorting and lifecycle, green.
- [x] TDD JSON12/SQLite migration/restore/alias/budget; integrate frozenpack4 and all existing liveguards, oldpacks no bypass.
- [x] TDD agent onebook read-only paging and realJSON/MCP pipe guards; implement bounded protocol/data projection.
- [x] Implement fullbook dashboard, reusable primary editor, excerpt selection/jump/dirty/unknown-safe UI; focused regressions and fixture.
- [ ] Full tests/build, independent task and final broad review, fix issues, truthful verification docs.
- [ ] Source-only archive replace current Library version2 with returned nextversion.
