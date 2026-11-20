# Material content history Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** 可恢复资料文字且不误标关联有效的内容历史。
**Architecture:** 事务历史与reviewflag，可信桌面分页读取/恢复/明确复核；备份format8兼容旧版。
**Tech Stack:** existing Rust SQLite React Tauri.
**Spec:** docs/superpowers/specs/2026-11-17-material-history-design.md

## Global Constraints
仅云、不外发、不接模型、不验证Windows；历史100000条/256MiB/单份现有256KiB约束；分页20默认50上限；JSONformat8读1..7、JSON64MiB、SQLite512MiB；历史不静默删除；恢复只title/fields，强制待复核。

## Review Focus
- 旧库/备份没有历史和flag仍无损迁移，含回收基线完整。
- 事务失败内容/revision/历史/flag全回滚，限额不载入所有历史。
- 恢复保留关联/剧情metadata，来源不能误报current。
- 普通保存/agent采纳不清flag，明确动作校验当前来源。
- UI迟到/重复/未知回执/未保存资料不丢稿或重复恢复。

### Task1: Complete material history vertical slice
**Files:** new material_history.rs and tests/MaterialHistoryPanel/types; modify materials structs/migration/writes, core backup, backup_restore whitelist/validation/staging/copy, native/api/MaterialsPanel, fixtures.
**Interfaces:** worker chooses exact paginatedhistory/detail/restore/ack types, expectedrevision+epoch; no externalagent mutations. Shared writes and backups use canonical validation. Return updated record without postcommitfailedload.
- [x] Write failing migration/history/restore/freshness tests, red.
- [x] Implement bounded core/history, shared write hooks and backup format8/SQLite compatibility, green.
- [x] Write failing UI dirty/late/reconciliation/ack tests, red.
- [x] Implement UI/native/fixture, green.
- [x] Commit tested slice and commands/limits report.
- [x] Independent scoped review and final broadreview, repair findings.
- [x] Full regression/build, docs and replace same Library v12→v13.
