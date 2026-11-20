# 创作资料库 Implementation Plan

> **For agentic workers:** Use subagent-driven-development / executing-plans; test before implementation.

**Goal:** 设定、角色与大纲可人工编辑/恢复，授权智能体可逐条安全读取。
**Architecture:** 同一Service/SQLite资料表；GUI受信写入，Broker仅读取。备份v4包含资料，旧版兼容。
**Tech Stack:** Existing Rust/rusqlite/Tauri/React/TypeScript, no new dependencies.
**Spec:** ../specs/2026-11-17-materials-design.md

## Global Constraints
仅云端；停止Windows验证；不调用模型、不外发稿件；只用隔离测试库；旧存档无损迁移。

## Review Focus
- 跨书target ID、回收父链和整库恢复不得泄露资料。
- 保存冲突与切卡失败必须保留草稿，不误报成功。
- v4缺字段/重复ID/非法引用须事务前拒绝，原资料不变。
- 删除关联卷章不能意外删除大纲文字。
- 外部工具表不得出现资料写入、采纳或恢复入口。

## Task 1: Domain and backups
Files: src-tauri/src/materials.rs, core.rs, lib.rs, tests/materials.rs and affected backup tests.
- [ ] 先写迁移/CRUD/revision/回收/关联/配额/备份兼容失败测试并确认红。
- [ ] 实现Material/MaterialSummary及Service list/get/create/save/trash/restore_material；同书检查、参数化SQL及事务。
- [ ] 接入v4完整校验/恢复事务，SQLite快照schema检查；执行全Rust测试并提交owned文件。

## Task 2: Safe external reads
Files: src-tauri/src/agent.rs, agent_stdio.rs, tests/agent_protocol.rs, agent_stdio.rs, stdio_process.rs.
- [ ] 先写未授权/跨书/回收/分页/无写工具测试。
- [ ] list_materials(kind?,after_id?,limit?)元数据页，get_material(material_id)单条；验证scope在同Service锁内。
- [ ] 扩展MCP工具描述并验证真实子进程路径，提交owned文件。

## Task 3: Human editing UI
Files: src/materialTypes.ts, MaterialsPanel.tsx, tests/MaterialsPanel.test.tsx; integration App.tsx/api.ts/types.ts/desktop.rs.
- [ ] 先测点击一项显示相应字段、保存冲突保留、切卡/关闭拦截、回收恢复、异步过期响应。
- [ ] 三类资料导航+单卡编辑；显式保存，未保存不切走；outline target可选。
- [ ] App打开前flush正文，可信Tauri命令集成；虚构fixture视觉验收；前端测试/build。

## Task 4: Review and delivery
- [ ] 独立审查备份和授权边界，修复后全测；仅明确通过的范围。
- [ ] 源码检查点、完整备份格式与限制说明、Library更新。
