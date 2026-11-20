# Chapter Version Browser Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans. Steps use checkbox syntax.

**Goal:** 分页按需浏览并命名重要章节快照，保留人工恢复及版本保护。
**Architecture:** 核心独立history metadata/label服务，复用既有正文版本；React独立HistoryPanel，仅选中历史才请求正文。单Service/SQLite事务与现有scope边界保持。
**Tech Stack:** Rust rusqlite + React TypeScript；不增依赖。
**Spec:** docs/superpowers/specs/2026-11-17-version-browser-design.md

## Global Constraints
云端；Windows暂停；不外发书稿；不删除旧历史；生产列表不加载正文。

## Review Focus
- 跨书/跨章版本ID和回收父链拒绝。
- 游标稳定且加载范围有界，未选中版本不读正文。
- 命名并发/失败保留草稿，不影响正文revision。
- 迟到详情和恢复确认不得指向更新后的选择或章节。
- 备份迁移、非法标签导入整体回滚并保留旧库。

## Task 1 Core
- [x] 写作用域、分页、标签CAS、备份兼容/拒绝、快照和外部工具未扩权测试，观察RED。
- [x] 实现独立版本浏览模块与标签迁移、JSON新版本；旧history兼容。
- [x] 聚焦GREEN并提交owned文件。
## Task 2 UI
- [x] 独立HistoryPanel测试分页/按需正文、迟到请求、草稿保护、恢复确认及提案页不加载历史。
- [x] 实现组件并从ReviewPanel分离；保持原提案确认和焦点管理。
- [x] 完整前端测试及构建，提交owned。
## Task 3 Integration
- [x] API和desktop可信命令接线；QA fixture及真实浏览器单版本场景。
- [x] 独立安全审查，修复、完整Rust/pipe/前端复跑。
- [x] 文档与验证边界、保留分支、同一Library源码新版本交付。

保持feat/agent-stage源码分支，不推送或合并其他分支。Windows原生行为未验收。
