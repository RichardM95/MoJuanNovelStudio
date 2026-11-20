# 跨卷远距伏笔验收计划

> **For agentic workers:** Use superpowers:subagent-driven-development for implementation and independent review.

**目标：** 补上全书伏笔早于八章普通前文时的精准引用和过期保护证据。

**方案：** 仅增加自主创作的短节夹具及验收测试，不改产品。使用真实 Service、隔离文件 SQLite 和生产 Broker，文学判断由独立阅读者另记。

**约束：** Windows 验证暂停。不用用户本机或真实稿件；不接外部模型，不外发。短节样本不代表完整长篇，不宣称机器能判断语义回收。八章上限针对冻结的普通前文，而非书级授权下的一切读取。

- [x] 核查旧覆盖：`prior_context.rs` 验证八章上限；`foreshadow.rs` 和原创单章续段验证出现来源过期；补充合并远距案例。
- [x] 写入 `qa/acceptance/longrange-foreshadow.txt`：原创 21 短节，第 2 章埋设，第 12 章相似呼应，第 21 章正文是候选。分两卷，第 21 章原稿与候选分离。
- [x] 增加 `src-tauri/tests/longrange_foreshadow.rs`：只冻结第 13–20 章普通前文，同书伏笔出现独立提供第 2 章来源和原文；异书会话不能读取该出现列表或包内字段。
- [x] 原引文和位置字节保留，只追加正文生成第 2 章 v2；报告旧来源过期，冻结旧引文仍可追溯但不能授予写入。重新冻结、重提、采纳均拒绝，目标正文和历史重开后不变。
- [x] 跑 `cargo test --locked --manifest-path src-tauri/Cargo.toml --test longrange_foreshadow` 和完整 Rust 回归；在 `docs/ACCEPTANCE.md` 分开记录工程断言和独立文本判断。
