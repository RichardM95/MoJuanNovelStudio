# 章节版本管理

## 目标与选择
当前每次自动保存已有完整历史快照，但ReviewPanel打开时连同提案一次加载本章全部历史正文。选择先改善历史浏览和重要版本命名，避免长篇高频编辑拖慢正常审核。全书替换涉及批量改稿，不是本轮；SQLite备份入口/大库容量作为后续独立主题。此选择由实现者按委派的优先级作出。

## 范围
- 历史标签页才加载元数据，按revision倒序稳定游标分页；点击一个版本才读取其正文。列表不含正文。
- 允许命名已有快照，清空名称不删除快照。名称单独版本并发校验；不改变正式正文revision。
- 支持只看命名版本，旧历史完整保留，不调整自动保存、不自动清理。
- 历史恢复须明确二次确认，绑定同书同章版本及当前正文revision；恢复生成新版本，原历史不抹去。
- 标签纳入新版JSON备份与恢复前SQLite快照；旧备份无标签时恢复为空，不推断名称。
- 生产UI不调用旧全量history接口；旧核心接口保留兼容现有测试/调用。

## 安全与体验
分页、详情、命名、恢复均验证活动作品/章节/版本作用域。迟到请求不能覆盖新章/新选择；命名草稿失败保留且阻止切换/退出，明确取消可放弃。恢复或命名在数据库事务中完成，版本冲突不能覆盖。任务检查无需因历史命名失效，因为正式书稿和目录未变。

不增加外部智能体工具，不连接模型、不外发书稿。Windows原生验证继续暂停；Linux数据库与浏览器虚构fixture分别验收。

## 接口与精确边界
VersionSummary仅id/chapter_id/revision/source/created_at/label/label_revision；VersionPage为items/next_cursor/epoch；VersionDetail为version(既有ChapterVersion正文)/label/label_revision/epoch。
list_chapter_versions(book,chapter,cursor,limit,marked_only)默认UI50，最大100；游标绑定book/chapter/filter/epoch/before_revision且最大2048B。并发命名变化不承诺分页快照，刷新回首页。
get_chapter_version按书章版作用域；set_chapter_version_label绑定expected_label_revision及expected_epoch；restore_chapter_version_scoped绑定expected正文revision及expected_epoch。旧epoch均拒绝写入。
名称trim空串转None，最多120 Unicode标量及480 UTF8字节，无控制字符。标签无行rev0，清空保留墓碑行与CAS计数；计数1..9007199254740991，到上限拒绝增加，导入越界拒绝。未保存标签草稿不可被悄然丢弃；App接收pending保护beforeunload/原生关闭，实际Windows行为仍未验证。
