# Bundled SQLite 防御性维护

基线v13：rusqlite0.32.1/libsqlite3-sys0.30.1，实际SQLite3.46.0，主库WAL。官方WAL-reset修复在3.51.3或特定回补；正常应用拓扑只以一个互斥Service写/checkpoint主库，未发现损坏，不把依赖维护说成漏洞复现。

选择官方rusqlite0.41.0和兼容sys0.39.1维护线（SQLite3.54.1），不是最小版本号升级；保留bundled/backup/hooks/limits，不加扩展、系统SQLite或手改vendor源码。Rust1.91.1满足最低要求；两个Cargo锁文件保持一致，通过registry校验。

先保存旧3.46实际生成的隔离库及快照、JSON内容，再升级后逐字段/历史核对和继续保存；默认回归验证运行时版本、未checkpoint WAL备份、自有多Service锁、既有migration/rollback/restore。不构造真实损坏或用户数据。外部SQL工具/硬链接风险不由sidecar锁或升级完全消除。Windows暂停。
