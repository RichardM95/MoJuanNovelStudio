# Book Replace Implementation Plan
> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development. Steps use checkbox syntax.
**Goal:** 安全的作品正文批量字面替换。
**Architecture:** 内存可信候选及单Service事务，独立React面板预览确认。
**Tech Stack:** Rust/rusqlite, React/TypeScript，无新依赖。
**Spec:** docs/superpowers/specs/2026-11-17-book-replace-design.md
## Global Constraints
云端、Windows暂停、无稿件外发、保持历史、仅活动当前作品正文。
## Review Focus
- 中文UTF8、非重叠替换、空词和扩张上限。
- 不得截断预览后批量提交隐藏结果。
- 新增/移动/回收/修改章节和epoch后确认拒绝。
- 未保存编辑及异步响应、重复确认不能误写。
- 中途数据库失败整体回滚，成功后加载失败不可重复提交。
## Task 1 Core
- [x] 先写真实SQLite RED测试：精确预览/提交/历史；多字节；空词；容量；全书manifest冲突；失败原子回滚；重复确认。
- [x] 实现book_replace模块及私有候选生命周期，返回metadata预览与按需详情，冻结输入，期限及epoch/manifest校验。
- [x] 聚焦及完整cargo test通过并提交owned。
## Task 2 UI
- [x] 独立ReplacePanel测试RED：输入改变旧预览失效、迟到结果、二次确认、冲突、提交成功但reload失败、pending关闭。
- [x] 实现面板及Types，metadata列表单章详情，错误清晰中文。
- [x] 完整npm test/build并提交owned。
## Task 3 Integration
- [x] desktop/api/App接线，进入先flush、退出保护、独立reload测试。
- [x] 外部入口拒绝测试，独立审查修复，完整回归。
- [x] 记录边界，Git归档，原Library v8更新为v9。
