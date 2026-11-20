# Material Context Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans. Track checked steps.

**Goal:** 来源可追溯、过期可见、书内检索、资料建议人工采纳闭环。
**Architecture:** 同一SQLite Service和事务；资料/来源与搜索核心、不可变提案模块、受限Broker、React界面隔离开发后整体审查。
**Tech Stack:** Existing Rust/rusqlite/Tauri/React/TypeScript; no new dependencies.
**Spec:** docs/superpowers/specs/2026-11-17-material-context-design.md

## Global Constraints
云端；Windows暂停；无模型/稿件传输/公网；所有资料及正文保留；外部仅单书授权；人工采纳；备份迁移原子。

## Review Focus
- 来源永久删除后不得成为最新；保留来源标识/文本。
- 旧草稿/旧提案与来源改动同时发生，不能覆盖当前稿。
- 同键不同载荷、撤回/采纳重放、跨书搜索不得越权。
- 搜索/卡片异步迟到响应不得把用户导航或草稿覆盖。
- 旧备份与非法来源引用不得在验证前修改原库。

## Task 1 Core
- [x] 写来源状态、summary、旧库迁移、搜索隔离/特殊字符/限额、备份拒绝/往返失败测试并运行确认失败。
- [x] 在materials.rs/core.rs增量实现来源契约、事务保存helper、v5；新增search模块返回有限纯文本片段。
- [x] 完整core/material tests通过，提交owned文件。

## Task 2 Proposal and transport
- [x] 测试幂等、同书/本人范围、来源与资料CAS、并发采纳、失败回滚、撤销与epoch。
- [x] 独立material_workflow模块复用资料事务helper，Broker只暴露提交/本人查撤；stdio声明匹配。
- [x] 增加真实管道fixture场景，不声称socket验证；运行tests并提交owned文件。

## Task 3 UI and integration
- [x] 写summary/source选择与状态、旧source保留、提案diff/二次确认/dirty/异步失败测试。
- [x] 扩展MaterialsPanel及类型；独立SearchPanel支持纯文本查询与打开命中；接App/api/desktop。
- [x] 前端测试、类型检查、生产构建；虚构预览端到端UI验证。

## Task 4 Whole stage
- [x] 独立安全审查，修复实际问题并复跑全部Rust/stdio/front。
- [x] 更新阶段能力/限制说明，同一Library源码身份版本替换后交付。

## Decisions and review record
Ruling: 本阶段仅既有资料更新提案，不实现智能体新建/删除或资料完整历史回滚；缩小不可逆影响面，未来新增需独立设计。
Ruling: 正文与资料提案独立幂等域/配额，各32MiB；不重构既有正文提案，文档明确合计容量。
Implemented checkpoints: UI bb9bc13; proposal/transport 011f2ec. 整体独立只读审查已通过，复核来源提示作为非阻断改进补入。
Ruling: 外部资料提案另限序列化记录512KiB，写入前拒绝会导致JSON/MCP响应过大的建议；避免出现已保存但客户端只收到超限错误的歧义，不限制人工资料编辑。

Final verification: 107 Rust标准测试 + 7实际管道场景；52前端；2socket ignored。检查点806aaa1(core)、3a68a19(UI)、cd30dec(material workflow)；最终增量编码上限审查随后记录在验证文档。
