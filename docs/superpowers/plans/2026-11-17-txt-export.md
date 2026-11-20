# 定向 TXT 导出 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** 可核查的全书、单卷、单章 TXT 导出。
**Architecture:** Service有界一致读取形成冻结字节候选；React选项/预览；原生另存为消费候选并create_new。
**Tech Stack:** Rust SQLite React TypeScript Tauri existing dependencies.
**Spec:** docs/superpowers/specs/2026-11-17-txt-export-design.md

## Global Constraints
仅云环境，不发送书稿，不连接模型，不验证Windows，不推送；32MiB输出、10MiB单章、100000节点、10分钟候选；预览前4000Unicode字符；不覆盖现有文件；新流程不load整库；保留旧兼容API。

## Review Focus
- 导出选择与预览冻结字节一致，不可变参数混用。
- 未保存正文必须先flush，失败不得打开。
- 参数变化、关闭、晚响应及连续点击不重复写入。
- 目标冲突和部分写失败不得覆盖旧数据或留下假成品。
- 卷章排序、跨书/回收节点、中文及混合换行正确。

### Task 1: Complete export vertical slice
**Files:** new src-tauri/src/txt_export.rs, src/ExportPanel.tsx, src/exportTypes.ts and focused tests; modify core Service, lib.rs, desktop.rs, api.ts, types.ts, App.tsx, style.css, qa preview fixture as needed.
**Interfaces:** Define serialized ExportOptions (scope type + optional target ID; include_metadata, include_headings, newline, bom), ExportPreview (id, bytes, volumes, chapters, text, truncated, expires_at), and API prepare(bookId,options), save(id): Promise<string|null>, discard(id). Rust prepare_txt_export, take_txt_export, discard_txt_export; exact type declarations shared through committed code before integration.
- [x] Write failing core candidate/rendering and file safety tests; verify red.
- [x] Implement bounded consistent scoped rendering/frozen candidate; focused tests green.
- [x] Write failing component/App tests for confirmation, invalidation, pending and save flush; verify red.
- [x] Implement ExportPanel/native routing/App optional API integration and fictional QA fixture; focused tests green.
- [x] Commit tested slice and write report with test commands, remaining limits.
- [x] Independent spec/quality review and repair findings.
- [x] Run full Rust/UI/build regression; broad review; record verification.
- [x] Package tracked sources only and replace same Library v10 with v11, preserving identity.
