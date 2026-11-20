# Strict Decision Receipts Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** 严格正文accept/reject精确核对、安全恢复与暂离，不把摘要或新版章当原决定证据。
**Architecture:** 独立前端决定attempt/validator/recovery单元，复用Service完整proposal与epoch。App memory跨面板保留未决；不改持久schema或核心决定事务。
**Tech Stack:** Existing React/TypeScript/JSDOM/Rust regression, no new dependencies.
**Spec:** docs/superpowers/specs/2026-11-18-strict-decision-receipts-design.md

## Global Constraints
云端虚构数据，不外发、不模型、不本机、Windows暂停。只读停止不证明write失败，无自动重发；真实write关闭保护不放宽。内存未决32条/32MiB编码，不存整包或持久稿件。旧历史兼容，23外部工具/数据格式不变。

## Review Focus
- 精确身份input/review/accepted版本 vs 摘要误确认。
- 同revision异正文拒绝当前成功；新版正文仅历史已采纳，不覆盖作者稿。
- 回执/章节读取失败/源删除/换库保留确知证据不恢复执行权。
- close/reopen/其他章节、pending和相反终态，不能再次决定。
- 三段以上readonly等待取消与latefinally/同batch换库，真实write仍阻塞关闭。

### Task 1: Exact strict-body decision and recovery workflow
**Files:** Modify src/WritingPanel.tsx, src/App.tsx, src/writingTypes.ts and relevant tests/fixtures; create focused helper/component if needed to keep growing WritingPanel manageable; docs/VERIFICATION.md/README. Backend only if demonstrably required, report before changing schema/API semantics.
**Interfaces:** Existing WritingAPI.proposal/epoch/state and onReload remain truth sources; add App-owned typed memory and exact decision validator/runtime local unit. Internal shapes chosen consistently in helper/types/panel/tests; no protocol changes. Config/prepare/author-review remain existing paths except guards needed to preserve pending decision.
- [x] Read currentwrite/refresh/review and App editorflush/close contracts; write failing tests proving summary refresh clears unknown and false input/version/currentbody success. Record actualRED.
- [x] Implement capture/strictcomparison, explicitterminalstatus and acceptedrevision+review validation, independent currentchapterstatus, preserved evidence and boundedmemoryadmission beforewrite; GREEN focused validators.
- [x] Wire separate decision reconcile, cancellable readrun and exact epoch sandwich, memory-scope isolation and UI control/wording. Test loss, stuckreads, late arrivals, statebatch invalidation and sideeffects.
- [x] Integrate actual WritingPanel/App close/reopen/flush, originaldirtyreview/config guards; test oldhistory/scope/otherchapter and no write replay, no backend/model changes.
- [ ] Run all Rust lockedoffline/protocol, npm test/build and QA fixture tsc; selfreview and docs, commit/report.
- [ ] Independent task+final review, scopedfix and final fresh validation; sourcearchive replaceLibrary expectedv5.
