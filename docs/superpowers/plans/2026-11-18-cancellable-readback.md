# Cancellable Readback Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** 停止等待永久挂起的只读核对，同时保护未决写入和已核实证据。
**Architecture:** 只读核对运行令牌与真实写入隔离，逐await副作用gate和显式停止等待；沿用App内存target隔离。
**Tech Stack:** Existing React/TypeScript/JSDOM; no new dependencies or backend schema.
**Spec:** docs/superpowers/specs/2026-11-18-cancellable-readback-design.md

## Global Constraints
云端虚构数据，不外发、不接模型、不用本机、Windows暂停。取消只读等待不等于取消底层传输/事务，也不证明写入失败。禁止自动重发或解除原未决attempt，真实写入保护不放宽。

## Review Focus
- 首epoch/get/末epoch任何一处never-settle都须能停止等待与退出。
- 取消后late resolve/reject/finally不能覆盖新核对/新书/新epoch或释放其锁。
- 读到准确terminal但末epoch挂起，取消须保留证据而不擅自settle。
- 暂离重开仍隔离同目标，其他目标正常；实际decide仍受原保护。
- 连点/关闭/卸载、父modal与nativeblocking接口无泄漏和unhandled rejection。

### Task 1: Cancellable read-only recovery lifecycle
**Files:** Modify src/ForeshadowProposalPanel.tsx and only necessary parent/type wiring; tests/ForeshadowProposalPanel.test.tsx and App/Foreshadow tests; docs/VERIFICATION.md and appropriate README help. Avoid unrelated backend changes.
**Interfaces:** Existing getEpoch/get/decide and App memory registry unchanged in external meaning. Add explicit UI stop-wait control for reconcile only; design local invocation token and cleanup in component, not generic global timeout layer.
- [x] Reproduce RED with deferred promises at each of reconcile's three awaits, proving current stop/exit unavailable. Trace busy/writeLock to parent before applying fix.
- [x] Implement independent read invocation identity, explicit cancellation with pending isolation retained, valid late-result gate before each side effect, preserving exact received evidence.
- [x] GREEN focused cancellation/late outcome/new-run/unmount/changedepoch/closed-reopened/other-target tests, plus no additional decide invocation and actual write still blocked.
- [ ] Inspect related list/detail/comparison and parent close paths; only fix direct new lifecycle risks, document boundaries. Run all frontend tests, production build and QA tsc, full Rust/protocol regression to prove no data-path change. Commit/report.
- [ ] Independent task and final review, scoped fixes and fresh aggregate validation; update same Library v4 using guarded nextversion.
