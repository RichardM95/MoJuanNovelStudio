# Local Tasks Implementation Plan

> **For agentic workers:** Use superpowers:subagent-driven-development or superpowers:executing-plans. Preserve isolated tests and independent review.

**Goal:** 真实上下文检查任务的可见进度、取消、重试和安全恢复。
**Architecture:** SQLite持久manifest/checkpoint/metadata结果，单后台线程共享Service；React任务面板轮询。
**Tech Stack:** Existing Rust/rusqlite/React/Tauri; no new dependency.
**Spec:** docs/superpowers/specs/2026-11-17-local-tasks-design.md

## Global Constraints
仅云端开发；Windows暂停；无模型/网络/稿件外发；不改正文；无外部任务工具；单Service写入。
## Review Focus
- done/结果必须同事务，重复step不得重复计数。
- 取消/完成竞争和失败不能谎报取消成功。
- 恢复epoch与旧游标、重启残留状态不能自动续跑。
- 先处理的章节后来改动必须在最终manifest检查发现。
- UI迟到poll/切任务/关闭/重试不能覆盖新目标或制造重复任务。

## Task 1 Persistence
- [x] 写真实字数/资料问题、进度、取消、幂等、重开/resume、输入变化、配额、restore清理回归，先观察失败。
- [x] 实现独立tasks模块及core迁移/恢复/快照接线，step每次一项，完成前复核全manifest。
- [x] 回归通过提交。
## Task 2 Runner
- [x] 真实线程测试运行到完成、取消、并发编辑/restore、idle/stop无死锁。
- [x] 单线程Condvar唤醒，每步释放Service锁，停止/join不持Service锁；不增加旁路DB。
- [x] 回归通过提交。
## Task 3 UI and desktop
- [x] 面板状态/真实进度、部分报告、取消竞争、resume/retry去重、迟到poll测试。
- [x] App先flush再开任务，API/desktop共用Service并唤醒runner；任务结果打开目标仍限定当前书。
- [x] 前端完整测试及构建；虚构预览自然检查流程。
## Task 4 Integration
- [x] 独立安全审查及修复；完整Rust/pipe/前端复跑。
- [x] 文档明确已验与Windows未验、任务不进JSON；保存同一Library源码新版本。

收尾：保持feat/agent-stage分支，不合并或推送；Windows继续暂停。交付为同一Library源码版本，不是EXE。
