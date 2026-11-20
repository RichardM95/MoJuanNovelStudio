# 安全按光标拆章

## Intent / selected approach
v11可安全导入全文单章但缺乏就地拆章。选择单章按光标两段拆分预览和原子提交，不做自由批量分章（误判风险）或合并（跨章关联迁移语义未获授权）。延续Rust/React/SQLite离线桌面架构。用户委托继续，parent同意此阶段及关联保护；不新增架构问题。

## Behavior and boundary
正文编辑器在光标位置提供“拆分章节”。先flush原稿，失败/中文输入法正在组合时不打开。仅折叠光标支持，选择范围不悄悄丢弃；首尾拒绝。捕获DOM UTF-16 offset；textarea换行规范化时先验证DOM文本等于正文换行规范化结果，再映射回原文UTF-16位置，不改原文；Rust按Unicode scalar验证UTF-16边界，拒绝代理对中间及CRLF中间，保留前后原始字节（不trim、不加换行、不重排字符）。允许合法组合字符边界但说明精确光标切分，非语义分章。

Service候选准备显式带book_id/chapter_id/expected_revision/utf16_offset/new_title，检查活动父链、书归属、revision且10MiB正文读前SQL预算。新章名沿用现有非空4096B规则，候选仅一个10分钟，旧候选新准备失败也失效。准备在一致读事务中冻结两段及当前卷活动章节metadata manifest（ID及标题累计最多16MiB，超限明确拒绝）（id/order/revision/title及原章卷/父链）；返回原章/新章名称、字节数和各最多2000Unicode字符预览，截断注明，可按需查看两段完整文本（单章总10MiB）。不load整库。

确认只传候选ID，在Immediate事务中重核epoch、原章正文/revision/卷与活动目录manifest，拒绝并发修改、改名、排序、新增/回收、跨卷移动；不会自动重新基于新正文拆。原章保留ID/title、正文前半revision+1，使用save_in_transaction保存拆前完整历史及拆后版本；新章UUID、后半正文revision0及初始历史/审计，紧接原章。仅该卷活动章节按稳定sort_order,id重编号，并跳过回收节点保留的sort_order以维持全卷唯一性；回收节点不变。事务内核查树总100000和历史100000预算含可能补baseline；任一步失败所有正文、目录、历史、审计回滚。提交消费候选，错ID不可消费别的候选；成功仅返回book_id/original_chapter_id/new_chapter_id，避免提交后load报错导致重复。

## Linked context
原章现有关联资料、来源、时间线、伏笔保留原chapter_id，绝不复制给新章。原章引用的摘要/资料来源revision因此过期/待复核；旧原章智能体建议因base_revision冲突不能采纳。新章不继承这些关联或建议。UI确认前明确此规则，并提示拆前正文可在历史查看；恢复历史只恢复原章，不能自动移除新章，勿称一键撤销。测试真实资料来源/摘要和提案，不只说明文案。若既有状态计算不能识别则本阶段补齐最小保护。

## UI and recovery
明确前后预览和新章名，修改新章名必须重新预览，不允许混用旧候选。二次确认，pending时禁止关闭/双击写入，迟到响应不覆盖新草稿。成功重载选择新章并用新revision创建SaveQueue。成功后load失败只重读，未知提交响应先核对重读，不自动重试拆章；核对失败阻止关闭。App保存失败保留草稿。外部智能体无拆章路由。

## Non-goals / verification
不合并、不自动迁移关联、不恢复结构的一键撤销、不连接模型、不外发稿件、不访问本机；Windows验证暂停。测试中文emoji/CRLF/混合换行、UTF16非法位置、empty/边缘/范围选中、字节拼接等于拆前、标题/容量、排序并发/epoch、历史与关联状态、重复/未知回执、事务故障、未保存与IME、重载失败。浏览器仅虚构fixture，独立审查和全部Rust/UI/build回归后更新同一Library。
