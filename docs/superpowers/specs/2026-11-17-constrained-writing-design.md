# 下一阶段设计：有依据的章节写作提案

状态：v13已交付，parent已授权立即实施此设计。仍不调用模型、不传稿、不做 Windows 验证。

## 方案选择
- **A：给现有 reason 加引用文字。** 最省改动，但引用可伪造，资料更新不使旧稿失效，旧接口可绕过；不推荐。
- **B：持久严格配置 + 固定约束包 + 专用严格提案。** 新增三个小领域表，复用单写服务、章历史、书级授权和人工审核；推荐。机械校验明确，旧提案不混成已检查提案。
- **C：自动规划多章、语义规则引擎和模型编排。** 验证面太大，当前不做。

完整纵向范围限定为**已有章节**：作者选择必需资料→生成冻结包→授权智能体分块读取→提交带引用/冲突说明的新正文→作者审核→事务重核后保存。不自动建章、不批量采纳，不给“语义绝对正确”的保证。

## 最小持久数据

### 1. writing_policies（正式数据）
每个目标章至多一条：`chapter_id, book_id, revision, mode(active|paused|disabled), goal, required_material_ids[], updated_at`。目标≤4096 UTF-8字节；必须选择1..32张同书活动资料，可选任何现有资料类型，不强制每本书都有角色/大纲卡。引用以ID而非标题保存。先不做可选素材、规则权重、前文自动召回或复杂目标长度规则。

作者可信界面配置，所有变更CAS+epoch；智能体只读。更新必需集合/目标使policy revision+1，旧包及旧严格提案不可再采纳。暂停禁止新包、新严格建议及采纳，**仍阻断旧普通提案**。停用需明确说明以后允许无约束提案，并在同一事务取消该章全部待审普通/严格提案；不得让停用复活旧待审稿。再次启用也取消已有普通待审稿，作为审计记录留存。核心守卫最终负责，不能只隐藏按钮。

### 2. writing_packs（不可变依据，正式数据）
`id, book_id, chapter_id, policy_revision, originating_epoch, created_at, expires_at, payload, byte_count`。payload含政策快照、目标章节**完整**正文/标题/revision及父链、每张必需资料完整内容/metadata/revision、来源/payoff章节ID/revision及活动状态。服务生成随机ID和冻结字节；引用到包内条目/字段/字符范围。无需新增哈希依赖，可用不可变服务端快照及精确比较；若以后加摘要，不自创散列算法。

TTL30分钟；每包编码后≤4MiB、目标正文≤1MiB（与现有agent新正文限制一致）、每卡遵循现有256KiB。任何必需资料超总预算都明确拒绝，**不截断**。所有包和严格提案证据合计≤64MiB、包≤1000；满额拒绝、不自动删除。容量用byte_count和SQL长度预检，不批量载入正文排序。索引不带payload，全文按最多32KiB原始UTF-8分块返回，实际JSON/MCP包络再检查2MiB帧限制。

生成包时在同一读事务重核活动父链。任一必需卡 `content_review_required=true`、source/payoff stale/unavailable或关联卷章失效即拒绝。无来源但未标待复核的原始设定可用；“无来源”不是造假，也不等于语义已证明。引用存在但指向别书或已回收者不能成为硬约束。

### 3. writing_proposals（严格提案与决策证据，正式数据+显式运行态）
`id, pack_id, chapter_id, base_revision, new_body, citations[], conflicts[], client_id, idempotency_key, payload, decision, created_at, decided_at, accepted_revision, live_epoch`。新正文≤1MiB，引用≤128项，冲突≤32项，总附注≤64KiB；复用全部待审数/每章数的既有配额原则。严格提案数量≤1000且纳入前述64MiB总证据预算。

引用：`entry_id, field_key, start, end, explanation`，字符边界以Unicode scalar计数、范围必须实际存在。每个必需条目至少有一条合法引用并有简短说明；这只证明“引用覆盖”，不证明模型确实遵循。冲突项：涉及条目ID列表+解释；本阶段是智能体自报/作者发现，不声称有语义检测器。作者界面能看到“无冲突为提交方声明，不是系统认证”。

## 接口与强制守卫
可信GUI：`configure_writing_policy(...expected_revision,epoch)`；`pause/disable`合并在配置动作；`get_policy`；`prepare_writing_pack(chapter_id,expected_policy_revision,expected_chapter_revision,epoch)`；`list/get_strict_proposal`；`decide_strict_proposal(id,approve,expected_policy_revision,expected_chapter_revision,epoch,conflicts_ack)`。

agent书级授权新增：`get_writing_policy(chapter_id)`、`get_writing_pack_manifest(pack_id)`、`read_writing_pack_entry(pack_id,entry_id,cursor)`、`propose_constrained_edit(pack_id,new_body,citations,conflicts,idempotency_key)`、`get/cancel_own_constrained_proposal`。**首阶段包由作者界面生成**，避免agent反复造包耗尽配额或偷偷选择依据。agent不得配置/暂停/停用/复核来源/采纳。

严格模式是章节级核心约束，不是工具提示：
- `Service::propose_edit` 与 `Service::decide_proposal(approve=true)` 在事务内遇到active/paused policy都拒绝，即使调用者绕过AgentBroker、提案早于启用或UI过期。
- 普通人工编辑继续可用；它会改变chapter revision，使旧严格提案失效。历史恢复、拆章、资料恢复等现有可信操作同理；本阶段不自动把原章policy复制给拆出的新章，拆章UI须提醒新章未配置严格约束。
- 严格提案提交和采纳分别重核policy revision、包TTL、epoch、目标内容/版本/父链、所有必需资料内容/metadata/revision与来源依赖。提交时检查不替代采纳时检查。
- 采纳用Immediate事务进行最后一次核查，复用save_in_transaction，保存正文、章历史、决定/accepted_revision同事务；返回已准备好的结果，不在commit后load失败时误示提交失败。
- 引用覆盖不足/依赖变动属于硬阻断；自报语义冲突必须作者逐次确认才能批准，不能由agent设ack。作者若改变正文，应作为新稿重新审核，不默默修改已批准payload。

## UI最小闭环
章节“写作约束”面板选择卡片、显示状态并配置目标；生成包后清楚列出必需卡及当前包是否有效。严格建议审核将新旧正文、必需卡引用、冲突清单并排展示，必须二次确认；普通建议标“无约束依据”，严格启用时不可采纳。dirty稿先保存或取消，不丢稿；异步scope隔离、重复点击锁、未知提交先重读、不自动重复采纳沿用现有保障。

## 备份/恢复与审计（不能恢复成授权）
新JSON格式9，读兼容1..8；SQLite白名单/验证/计数/暂存/恢复同步纳入policy、pack、严格提案证据。所有引用归属、编码长度、byte_count、唯一键及配额均验证；保留原已验证恢复前快照。64MiB JSON/512MiB SQLite源限制不放宽。

- policy、冻结包、严格提案payload和历史decision是正式数据，必须备份保留。包及引用是冻结证据，不依赖被引用资料仍存在；不得因资料回收/永久删除而删掉批准依据。目标作品永久删除仍按已授权级联语义删除所属证据。
- 运行时授权、待审可执行性**不**恢复：恢复旋转epoch、清会话/旧普通提案；所有恢复的strict proposal强制 `live_epoch=NULL`，UI显示“历史归档”，即便原decision=pending也没有批准入口。保留decision=accepted仅是历史事实，不能再次执行。恢复包因originating_epoch不同只供历史查看，必须生成新包/新提案。
- 恢复的active policy保留保护但变paused，作者查看新书库后明确恢复active；不是自动停用防护。改动policy revision使旧包不可复活。
- 旧备份没有严格配置时不能假装保存了规则：恢复预览明确“此备份不含写作约束，将替换现有约束配置”。当前库存在active/paused保护配置时，恢复任何备份都须额外明确确认会替换当前配置，核心所有恢复入口校验该确认，不能仅靠UI文案。普通备份升级不得静默删除当前配置或历史证据。
- 不做自动证据清理。配额用尽提供清楚阻断，历史清理另列后续任务；不要靠清空审核记录释放容量。

## 验收重点
1. 设定变了而章节没变仍拒绝旧建议；大纲回收/来源改稿/资料恢复待复核/父链移动/整库恢复同样拒绝，旧普通提交及旧普通采纳不能绕过。
2. pack绑定书/章/政策，跨书、错误revision、伪造字段/字符范围、未覆盖必需卡、必需内容超预算全部拒绝，无静默截断；emoji/CRLF引用边界测试。
3. policy暂停仍锁普通通道；停用取消现有待审稿，再启用不复活。配置更新/恢复后旧包和提案不可执行。
4. 幂等、未知回执、并发与故障回滚；采纳后历史和decision一致；客户端不能代替作者确认冲突。
5. JSON/SQLite新旧备份往返保留证据但不复活批准/授权；篡改证据、超额、无匹配归属拒绝；旧备份缺规则警告必须覆盖实际恢复路径。
6. 一份可走通的虚构“角色不能游泳+本章过河大纲”任务：agent可提交带引用但语义矛盾的稿件，系统不能谎称语义通过；作者看见依据并拒绝。此例验证审核可见性而非宣称关键词规则能理解剧情。

## 可承诺与剩余风险
可承诺指定版本依据、引用可追溯、机械冲突阻断、强制人工采纳；不能承诺模型语义绝对正确。引用覆盖可能被机械凑数，作者仍须审阅。首阶段不处理资料互相矛盾的自动裁决，不接模型、不外发稿件；Windows/真实socket限制沿用并单列。

## Controller implementation rulings
引用范围用Unicode scalar索引，包读取用可验证UTF-8字节边界游标；完整条目由服务构造，不接外部路径。严格建议提交方client_id由Broker会话给定，不信任客户端伪造。停用/重启/修改policy都须CAS且取消现有待审，不允许停用复活旧建议。正文编辑和历史恢复仍可由作者直接执行，但不叫经约束审查。源/引用变化重核以实际内容metadata及版本/状态为准，不能只看revision。原章拆出新章不继承policy，拆章确认文案须明确。
