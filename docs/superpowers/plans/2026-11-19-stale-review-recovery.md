# 过期建议保留备注 Implementation Plan

> **For agentic workers:** Use superpowers:test-driven-development and verification-before-completion.

**Goal:** 用原创 21 章样本复现并解除未保存备注阻塞旧建议退出的困局。

**Architecture:** 已有写作会话负责单份有界的纯文本备注副本，WritingPanel 明确保留、显示、复制或移除，不改变 Rust 或既有采纳流程。

**Tech Stack:** React、TypeScript、Vitest。

**Spec:** docs/superpowers/specs/2026-11-19-stale-review-recovery-design.md

- [x] 写复现测试：旧保存被拒、原地查源、暂存退出、同章重开/new 空白审阅。
- [x] 验证缺失功能红测。
- [x] 加单槽位边界、相同范围核对、只读查看和复制结果提示。
- [x] 验证无静默覆盖、上下限、换库/作用域隔离和误判防护。
- [x] 运行前端回归和构建，补验收文档，交独立审查。
