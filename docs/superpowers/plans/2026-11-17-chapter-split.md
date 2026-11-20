# Chapter split Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** 原文不丢、关联不误继承的按光标拆章。
**Architecture:** 单个有界冻结候选，Immediate事务保存原章/新章/历史/顺序；React两步预览及回执核对。
**Tech Stack:** existing Rust SQLite React TypeScript Tauri.
**Spec:** docs/superpowers/specs/2026-11-17-chapter-split-design.md

## Global Constraints
仅云、不外发、不接模型、不验证Windows；单章10MiB、树/历史各100000、目录metadata16MiB、候选10分钟、每段预览2000Unicode字符；旧关联留原章，新章不继承。

## Review Focus
- DOM UTF16与Rust UTF8位置一致，代理对及CRLF中间拒绝。
- 未保存和IME操作不丢稿；成功后新SaveQueue revision正确。
- 目录或内容并发变动整次拒绝，rollback覆盖所有表。
- 资料摘要旧revision过期、旧建议冲突，UI准确说明。
- 未知提交/加载失败仅核对，不重复拆章。

### Task1: Safe split vertical slice
**Files:** new src-tauri/src/chapter_split.rs, src/SplitPanel.tsx, src/splitTypes.ts and tests; modify Service/lib/desktop, types/api/App/style; qa fictional fixture.
**Interfaces:** worker settles exact serialized SplitPreview/SplitOutcome and SplitAPI prepare(book,chapter,expectedRevision,utf16offset,newTitle), detail(id,side), commit(id), discard(id). Capture cursor beforeflush then obtain saved revision via latest result/queue, not stale App closure.
- [x] Write failing core boundary/transaction/context tests, red.
- [x] Implement bounded snapshot, scoped manifest, atomic split, history/association checks; green.
- [x] Write failing panel/App tests including dirtysave/IME/selection/cursor, reconciliation; red.
- [x] Implement native/UI/API and fictional QA integration; green.
- [x] Commit, report covering commands and limitations.
- [x] Independent spec/quality review, fixes and broad review.
- [x] Full Rust/UI/build regression, document, replace Library v11 with v12.
