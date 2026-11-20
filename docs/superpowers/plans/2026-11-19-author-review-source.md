# 作者来源核对 Implementation Plan
> **For agentic workers:** Use superpowers:test-driven-development and verification-before-completion.
**Goal:** 留在正文建议里核对跨卷伏笔的已保存原文周边。
**Architecture:** WritingPanel 按建议和冻结出现绑定小型只读展开，App 仅传 foreshadow epoch/sourceWindow；新增内部桌面只读命令，调用现有 Service，不扩 agent 协议。
**Spec:** docs/superpowers/specs/2026-11-19-author-review-source-design.md
- [x] 用 21 节原创夹具将第 2 章映射到第 21 章建议的冻结出现，测试版本、位置、范围以及关闭时保留未保存的审阅/原稿；先见原版本无按钮失败。
- [x] 模拟同引文旧版本、CRLF/emoji、移位、只读挂起和 epoch 变化；旧结果不能发布。
- [x] 添加只读展开并最小接线，禁止对归档/过期库发起读。回归验证与验收文档报告真实边界。
