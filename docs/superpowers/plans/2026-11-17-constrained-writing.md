# Constrained writing Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 作者配置必需依据，智能体提案可追溯且不能绕过依赖重核和人工采纳。
**Architecture:** 三个领域表policy/immutablepack/strictproposal。共享单写Service做旧通道守卫与严格提案事务；author GUI配置及审核、book-authorized MCP/JSON读取与提案；JSON9/SQLite保留归档证据而不恢复执行权限。
**Tech Stack:** Existing Rust/SQLite/React/Tauri; no new network/model dependencies.
**Spec:** docs/superpowers/specs/2026-11-17-constrained-writing-design.md

## Global Constraints
仅云、不外发、不接模型、不验证Windows；已存在章；必需卡1..32；目标4096UTF8字节；目标/新正文1MiB；pack4MiB/30分钟、分块32KiB；包和严格提案各1000、合计证据64MiB；引用128/冲突32/附注64KiB；JSON9兼容1..8且64MiB，SQLite512MiB；不静默截断或清证据。语义绝对正确不可保证。

## Review Focus
- 两个旧入口Service普通提交及普通采纳均守卫，paused不能绕过，停用不复活旧待审。
- 包在一致事务读依赖，提交和采纳重核资料内容/版本/状态/来源即使目标revision未变。
- 伪引用/跨书/待复核/限额/包过期拒绝，真实MCP帧与分块边界有界。
- 整库恢复保留证据但proposal归档/epoch失效/policy暂停，所有入口替换受保护配置都需明确确认。
- UI未保存/异步/重复/未知回执只核对不重写，冲突由作者确认，不显示语义自动通过。

### Task1: Complete constrained writing vertical slice
**Files:** new constrained_writing.rs + tests + WritingPanel/types/QA; modify Service migrations/lib/workflow guards, agent broker/stdio schema/tool handling, desktop/api/App/ReviewPanel/SplitPanel, core backup/backup_restore/jobs/UI types and existing fixtures.
**Interfaces:** implementer chooses exact serialized policy/packmanifest/entrychunk/citation/conflict/proposal types and trusted/agent methods from spec. Keep actor identity Broker-owned; scoped user session. Domain module internal helpers reused transactionally by legacy guards andrestore validation. No postcommitload failure ambiguity.
- [x] Failing tests oldroute bypass/pack scope+contentdependency/flags/refbounds+idempotency/atomic decisions, red.
- [x] Implement core/migration/guards with bounded evidence and explicit disable semantics, green.
- [x] Failing JSON9/SQLite archive+oldbackupack tests, red; implement canonicalvalidation/copy/restore protections, green.
- [x] Implement book-authorized transport, schema and real subprocesspipe tests; no sockets or Windowsclaim.
- [x] Failing GUI setup/review/dirty/late/unknown/conflict acknowledgement tests, red.
- [x] Implement full author configuration/pack manifest/strict review flow and fictional fixture, green.
- [x] Commit with exact commands/results/limits report; independent task and broad reviews.
- [x] Full regression/build and independent review; source ZIP delivery uses the existing Library identity and version guard. Delivery version is confirmed by Library, not inferred from this plan.
