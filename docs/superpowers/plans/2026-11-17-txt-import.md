# TXT Import Implementation Plan
> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development. Steps use checkbox syntax.
**Goal:** 预览并确认TXT导入新作品。
**Architecture:** 有界冻结源候选/保守分章与单章模式/事务INSERT，React元数据列表按需详情。
**Tech Stack:** Rust/rusqlite与React，无新依赖。
**Spec:** docs/superpowers/specs/2026-11-17-txt-import-design.md
## Global Constraints
云端；不覆盖旧稿、不改变源文件、不外发；Windows暂停；16MiB源、2000章、200卷、10MiB单章。
## Review Focus
- 中文/BOM/CRLF/LF/无末尾换行正文保真。
- 标题误判可改为单章，标题前文字不丢。
- 不支持编码、超限不可部分导入或截断。
- 更新候选旧ID/迟到响应、二次确认绑定当前配置。
- 同名新ID、中途失败全回滚、未知回执不重复导入。
## Task1 Core
- [x] 先RED测试解析、换行保真、单章退路、非法输入/容量、候选生命周期、事务回滚与旧稿不变。
- [x] 实现txt_import模块/Service候选与公共metadata/detail/outcome类型，冻结源重新配置，不重读路径。
- [x] 完整cargo test，owned提交与独立审查。
## Task2 UI
- [x] 先RED面板测试选择取消/标题模式变化/按需详情/迟到/二次确认/未知回执与刷新失败。
- [x] ImportPanel及types/scoped样式，当前候选配置确认，状态保护。
- [x] 全套前端/build并独立审查。
## Task3 Integration
- [x] desktop有界读取/对话框/API/App flush和重载接线；隔离读取及外部方法拒绝测试。
- [x] 虚构浏览器验收，完整回归和最终独立审查。
- [x] 更新文档及同一Library v9→v10，保留分支，不推送。
