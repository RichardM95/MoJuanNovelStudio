# Prior Context Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** 将作者选择的前文全文纳入智能体可读且版本绑定的冻结依据。
**Architecture:** 扩展现有policy/pack记录与handle3，复用单写服务、分块读取和人工审阅。保持旧API与旧归档兼容、JSON11严格验证。
**Tech Stack:** Existing Rust/SQLite/React/Tauri, no new model/network dependency.
**Spec:** docs/superpowers/specs/2026-11-18-prior-context-design.md

## Global Constraints
仅云，不外发、不接模型、Windows暂停。0..8前文、每章1MiB、总包4MiB/证据64MiB、32KiB块。旧configure保留已有前文，前文是补充上下文不是自动语义证明，必需卡审阅和人工采纳不放宽。

## Review Focus
- 标题序号与实际跨卷排序不一致，以稳定目录键为准。
- 当前引用删除后配置可修正且历史包不丢失。
- 新字段在旧API、回执匹配、未知保存及重基流程中不被遗漏。
- 一个章同时作为source和context，冻结证据不能互相矛盾。
- JSON旧格式与新句柄/上下文不互相冒充，恢复不复活执行权限。

### Task1: Complete frozen prior-context vertical slice
**Files:** constrained_writing.rs/core.rs/desktop.rs/agent_stdio.rs; writingTypes.ts/WritingPanel.tsx/api.ts/App.tsx/QA; Rust and UI tests/docs.
**Interfaces:** WritingPolicy.context_chapter_ids defaultempty; WritingPack.context_chapters typed frozen Chapter+Volume snapshots; trusted configure_writing_policy_with_context mirrors legacy params plus explicit context IDs. Legacy configure preserves saved IDs atomically. Existing manifest/chunk/citation APIs expose context:<id> but no new external mutation method. New trusted bounded directory metadata reader may reuse existing outline; no chapter bodies in selection listing.
- [x] TDD core contextshape/order/limits/oldAPI and submit-review-accept dependency guards, implement exact bounded snapshots.
- [x] TDD JSON11/SQLite compatibility, frozen-current separation and overlapping source/context consistency; implement validators and bounded readers.
- [x] Wire trusted desktop/API and author selection/preview/receipt guards; focused UI tests for dirty/lost/replaced scope plus contextquotes.
- [x] Real JSON/MCP pipe tests for chunk/citation/bookscope and no mutation authority; regression on reviewed acceptance and backups.
- [x] Full test/build, independent task and broad review, fix identified gaps, document actual limits.
- [x] Prepare verified source-only archive; replace existing reattached Library identity using current version guard. Library response confirms actual delivery version.
