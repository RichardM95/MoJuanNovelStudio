# 创作资料内容历史

## Intent / approaches
现有资料只有回收，没有修改历史。选择保存标题和fields的内容版本及人工确认恢复，不做全对象回滚（会错误迁移章节/来源/剧情状态），不接外部模型。延续Rust/React/SQLite；parent批准并要求来源复核、迁移和备份兼容。

## Storage and writes
新增material_content_versions表，FK material_id ON DELETE CASCADE，记录id、material_id、material_revision、title、fields JSON、created_at、source，唯一(material_id,material_revision)。单快照遵循现有256KiB卡内容/标题约束；全库最多100000条、累计内容256MiB，超限事务拒绝，不自动清理。统计使用可验证byte_count避免载入所有历史。
迁移为现有每张资料（含回收）保留当前内容baseline，不虚构过去；迁移事务原子。创建保存初始版本，手动保存和人工采纳资料建议经同一写入服务保留修改前后内容。纯metadata变动可不增加重复历史，但下一次内容变更须补当前baseline。历史失败回滚当前写入。回收不删除历史，永久级联语义不变。

## Scoped reads and restore
可信桌面单书/单资料分页索引默认20上限50、按ID读取单份正文、epoch及revision保护；列表不取fields，拒绝越书/其他资料/失效父链。回收卡须先恢复。读取单份合法256KiB，错误静态提示。
恢复前展示历史和当前标题/字段及不恢复项。确认检查epoch/material revision、快照归属、完整活动父链，只恢复title和fields，保留kind/book/volume/chapter/source/payoff/story_order/story_time/tracking_status。继续既有shape/quota/tracking约束，旧内容不满足当前剧情状态时拒绝，不悄改状态。恢复是新revision并保存前后历史。成功结果在commit前准备，不以postcommitload报错诱发重试。

## Association freshness
materials新增content_review_required默认false（serde默认兼容旧数据）。任何历史内容恢复设true，即使来源revision仍相同；UI显示“内容已恢复，关联待人工复核”。有source/payoff时派生状态不得显示current，显示stale；无引用也保留待复核标记。
普通编辑、建议采纳、回收恢复、目标脱离均不可静默清除。新增仅可信桌面明确复核动作，expected material revision+epoch；现有source/payoff必须属于同作品活动章且revision为当前，否则拒绝。确认后清flag且增加material revision/更新时间令旧建议冲突。外部智能体不能恢复历史或清标记，读取能看到待复核。状态不是内容正确保证，作者仍需核对。

## UI
资料编辑器打开历史前须保存或取消未保存资料，不自动丢稿。索引分页/点条目读全文；防迟到覆盖新卡，二次确认，busy禁止重复写入/关闭。未知回执或成功后重载失败先核对重读，仅重读不再次恢复；核对失败阻止关闭。刷新后按新revision继续编辑。明确复核按钮同样保护并说明来源是否可核验。

## Backups
JSON升级format8保留历史与reviewflag，兼容1..7并补当前baseline。JSON仍64MiB超限提示SQLite。SQLite全量备份/白名单/验证/暂存/计数/恢复支持新表和字段；验证历史归属/shape/revision/byte_count/总量，保留恢复前快照和中断保护。旧SQLite缺表/字段仍可导入并补baseline。新历史不可静默丢弃，preview注明资料历史数量，SQLite512MiB限制保持。正式历史不按运行时提案/任务清理。

## Tests / limits
现有库含回收迁移、初始/手动/建议/恢复历史、故障回滚/配额、分页懒读/越权、epoch/CAS、关联保留与强制待复核、过期来源不准清flag、普通/agent不准清flag、JSON旧新/SQLite旧新roundtrip及篡改历史/中断。仅云，不访问用户本机、不外发、不接模型，Windows继续暂停。不做历史清理或复杂全文diff。
