# Large Library Backup Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans.

**Goal:** 可预检、明确确认、有验证留底的大书库SQLite备份恢复。
**Architecture:** 受限只读源→私有受信staging→一次性预览令牌→同Service排他事务导入。单槽后台manager承载实际进度与取消，UI仅操作不透明作业。
**Tech Stack:** Rust rusqlite与React，无模型或网络服务。
**Spec:** docs/superpowers/specs/2026-11-17-large-backup-design.md

## Global Constraints
云端；Windows暂停；不覆盖liveDB文件；不外发；恢复前验证留底；首版512MiB上限。

## Review Focus
- 外来schema/WAL/超长字段/慢查询拒绝且源不变。
- 预览冻结、token一次性/过期、当前库变化不能跳过确认。
- 每阶段取消/磁盘失败回滚，不删除既有备份或原稿。
- manager/Service锁顺序、关机与commit竞争不死锁不假报取消。
- 提交成功后刷新失败独立呈现，旧UI稿件不能再次写回。

## Core
- [ ] RED：大于64MiB往返、恶意schema、超限、取消、留底失败、事务失败与旧请求失效。
- [ ] 实现流式白名单验证/准备/恢复、SQLite导出和JSON兼容。
- [ ] GREEN并提交owned。
## Manager
- [ ] RED：单槽/真实进度、取消、过期、重复confirm、锁竞争与shutdown。
- [ ] 实现私有候选生命周期及后台作业，外部工具不增加。
- [ ] GREEN并提交owned。
## UI and integration
- [ ] RED：预检后确认、终态/取消竞争、迟到请求、prepared取消、提交后重载失败。
- [ ] BackupPanel/API/desktop/App接线；旧稿状态清理和关闭保护。
- [ ] 浏览器虚构流程、完整前后端测试、独立安全审查与修复。
- [ ] 更新验证文档并交付同一Library源码新版本，保留分支不推送。
