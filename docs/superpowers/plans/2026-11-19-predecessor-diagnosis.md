# 冻结前文位置核对 Implementation Plan

> **For agentic workers:** Use test-driven-development and verification-before-completion.

**Goal:** 作者可看出哪篇冻结前文现在排在目标章之后。

**Architecture:** Rust 同事务按包读取已冻结 ID 与实时元数据，桌面只读异步桥；WritingPanel 显式核对并导航既有选择区。

**Spec:** docs/superpowers/specs/2026-11-19-predecessor-diagnosis-design.md

- [x] 用《汐灯阁》移动分支复现现有无精确核对入口。
- [x] 用生产 Service 专项断言位置前后、不可见及 scope/epoch，配套 UI 覆盖晚到、无写入、草稿。
- [x] 已运行完整 Rust 与前端验证。前端全量未全过，超时根因未定；结果、隔离对照及未测原生范围见 [验收记录](../../ACCEPTANCE.md)。
