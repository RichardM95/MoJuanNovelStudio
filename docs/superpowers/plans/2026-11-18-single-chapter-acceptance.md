# Single Chapter Acceptance Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** 固定可复现基线，用原创两短章样本验收单章写作闭环而不扩功能。
**Architecture:** Shared JSON fiction fixture + real Service/stdio integration layer + actual React App/mockbridge layer, clearly separate evidence levels.
**Tech Stack:** Existing Rust/rusqlite/stdio, React/JSDOM/Vitest, TXT export service; no dependencies.
**Spec:** docs/superpowers/specs/2026-11-18-single-chapter-acceptance-design.md

## Global Constraints
仅云端虚构数据，不模型/外发/用户设备/Windows。只测试与文档，无产品改动；发现bug先报告最小RED。当前样本两个短章，不冒称20–30章/长篇或语义已验。源ZIP SHA2566c40972d353374e6f82088c94f5e2fc9bb5a86a7f2afcc4c4d09eecab4c6ac4f，基线9d45ef19b00659b456f093396513c3a3a216db6c。

## Review Focus
- 真pipe和mockUI层明确，不把模拟作者决定称AI语义验证。
- 接受/拒绝后重开与TXT真实字节一致，幂等无重复历史。
- 事务故障正文/历史/决定不半写，负例有保留原状态断言。
- 错/丢回执、dirty稿、late换库不触发多余写或覆盖。
- fixture完整原创单数据源，资源临时隔离，证据与命令可复跑。

### Task 1: Baseline and original chapter acceptance evidence
**Files:** Create qa/acceptance/single-chapter.json and README; docs/ACCEPTANCE.md; new Rust integration test (e.g. src-tauri/tests/single_chapter_acceptance.rs) and new actual App flow test tests/SingleChapterAcceptance.test.tsx; extend existing harness test helper only if needed, preserve protocol production. Cargo.toml test-target declaration allowed if harnessfree. Update verification doc with actual results.
**Interfaces:** FixtureJSONschema documented fields forwork/volumes/chapters/materials/original/accepted/rejected/dirty drafts. Existing core command/strict proposal/review/export APIs unchanged; test trustedauthor uses Service only in fixture, external messages productionBroker noaccept endpoint. Exact test count derived outputs.
- [x] Read existing stdio_process harness, AppWriting helpers and TXTexport test contracts; write baseline evidence inventory and fixture schema/story before test plumbing.
- [x] Add coherent realService/realpipe positive acceptance + author rejection + transactional rollback tests; prove no preaccept bodychange, exact proposal versions, close/reopen persistence and actualTXTbytes.
- [x] Add coherent App sample walkthrough with explicit authorreview/confirmation and recovery/reopen/export. Map existing boundarytests for remaining risks, add fixture-based negative paths where meaningful ratherthan copy tests blindly.
- [x] Run focusedtests, inspect exportedbyteassertions and cleanup; ifproductbug showRED andpauseaffectedstep, no weakening expectedbehavior.
- [ ] Run fullRust/protocol/UI/build/QA; record exact implemented/automated/realenvironment/unverified table and reproducible commands. Selfreview, commit/report.
- [ ] Independent task+finalreview and final freshsuite; sourceZIP replaceLibrary expectedv6, noDB/output/logs.
