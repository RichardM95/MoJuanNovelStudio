# 本机智能体接口（阶段性实现）

## 已验证边界

- `Service` 为唯一写入者。Windows 桌面的编辑命令和 Broker 共用同一书库连接。连接授权校验、整库 epoch 校验和业务调用持同一个 Service 锁。
- Windows 实机测试使用生产 `mojuan-agent.exe` 通过 stdio MCP 连接命名管道，核对申请、单书授权、按章检索与撤销。Unix socket 测试需在允许本机套接字的环境中单独运行。
- Windows 使用限本机和当前 Windows 用户的命名管道。任一客户端还须在桌面获准访问具体作品。管道名称不可用时，连接面板显示启动失败。 [Windows 命名管道访问控制](https://learn.microsoft.com/en-us/windows/win32/ipc/named-pipe-security-and-access-rights)
- 无模型请求、无网络监听、无外部稿件传输实现。将 CLI 接到其他模型客户端之后，数据是否被该客户端上传是客户端自身行为；只有信任该客户端时才授权。

## 构建与验收

```sh
cd src-tauri
cargo test
cargo build --bin mojuan-agent --bin mojuan-agent-host
# 在允许 Unix socket 的 Linux 环境上单独执行，不能把默认 ignored 当作通过：
cargo test --test agent_protocol -- --ignored
```

默认测试覆盖 Broker pending、单书范围、客户端身份注入拒绝、禁止方法、幂等、撤销/epoch、cancel、限流、帧大小及目录安全；stdio 测试覆盖 JSON/MCP 协议。真实 socket 测试验收私有权限、解析、连接清理、host 关停。

## Windows 桌面与 MCP

1. 构建 `mojuan-agent.exe`（`cargo build --manifest-path src-tauri/Cargo.toml --bin mojuan-agent`），启动墨卷桌面并打开目标作品。
2. 在「智能体连接」复制 Windows 本机地址。在 MCP 客户端设置命令为 `mojuan-agent.exe` 的绝对路径，参数为 `--socket`、该地址、`--mcp`、`--name`、客户端名称。
3. 客户端调用 `session_status` 后，在墨卷刷新连接，并向刚发起申请的客户端授予当前作品权限。
4. 用 `get_outline` 获取目标章 ID，再调用 `retrieve_writing_context`。例如，查询「亡者之港、陆沉、零号」，获取相关资料、当前卷计划和上一篇有正文的章节尾段。

使用 Codex CLI 时，可在 PowerShell 中注册此 stdio 服务。

```powershell
$mojuanBridge = 'C:\path\to\mojuan-agent.exe'
$mojuanPipe = Read-Host '粘贴工作站显示的本机地址'
codex mcp add mojuan -- $mojuanBridge --socket $mojuanPipe --mcp --name Codex
```

`retrieve_writing_context` 接受 `chapter_id`、`query`、可选 `limit`（1–30）和 `include_references`。结果含资料或章节 ID、修订版本、字段、原文、UTF-8 字节区间与文件出处。状态 `manuscript`、`material`、`plan`、`review`、`reference` 分别表示正稿、资料、计划、待核对和参考。准备续写建议时，以已冻结依据包中的条目 ID 和修订版本核对引用，写作提案使用 Unicode 字符区间。

Windows 主机可运行 `cargo test --manifest-path src-tauri/Cargo.toml --test windows_agent_pipe -- --ignored` 验证命名管道。受限沙箱可能拒绝打开本机管道。

## Linux 操作员 host（备用流程）

```sh
mojuan-agent-host --database /absolute/path/library.db --socket /absolute/path/private-agent/agent.sock
```

host 必须在人工操作员终端运行，拒绝管道 stdin。目录必须当前用户拥有、0700，无路径符号链接；不存在时仅创建最后一层。socket 0600，已有 socket 不覆盖。正常退出删除 socket，崩溃残留需操作员确认无运行实例后清理。host 与桌面不能同时打开同库（进程写锁）。host 不提供稿件采纳、历史恢复和删除；写入建议后退出 host，再通过桌面人工审阅，是当前备用流程。

终端命令：`books`、`sessions`、`grant SESSION_ID BOOK_ID`、`revoke SESSION_ID`、`quit`。客户端标签只是自报，不是身份认证。授权前核对期望连接及书名。`grant` 只接受 pending 连接，一次限定一本书，不能更换已授权书；需断连重新申请。

```sh
mojuan-agent --socket /absolute/path/private-agent/agent.sock --name my-local-agent
# 或仅支持 MCP 2025-11-25 的 tools-only stdio：
mojuan-agent --socket /absolute/path/private-agent/agent.sock --name my-local-agent --mcp
```

CLI 第一次工具调用自动申请 pending，保持同一 socket，断线不重连、不自动重放。先发 `session_status`，等待 GUI/host 授权后操作。每行一个 JSON，stdout 只含 JSON。MCP 必须 initialize 指定 `2025-11-25`，然后 `notifications/initialized`；不宣称支持新版协议、resources、prompts、sampling。

## 请求契约

JSON 和 MCP 共用 24 个工具与同一套授权。

CLI：`{"method":"get_chapter","params":{"chapter_id":"..."}}`
响应：`{"result":...}` 或 `{"error":{"code":"...","message":"..."}}`。

- `session_status {}`：本连接状态。
- `list_books {}`：仅已授权的一本书，不列出其他书。
- `get_outline {}`：已授权书的卷章元数据，不含正文。
- `list_materials {kind?,after_id?,limit?}`：仅授权书的可用资料元数据，kind 可选 setting/character/outline/summary/timeline/foreshadow，按 id 稳定升序；默认50、上限100，返回 `{items,next_cursor}`。有后页时 next_cursor 为当前页最后一项 id，否则 null。列表不含 fields。
- `get_material {material_id}`：同一 Service 锁内先核对授权书的可用元数据，再加载单卡 fields。跨书、资料回收或关联卷章回收均返回通用 not_found。
- `get_chapter {chapter_id}`：授权书下有效卷/章的正文与 revision。
- `propose_edit {chapter_id,base_revision,new_body,reason,idempotency_key}`：仅新建不可变待采纳提案，客户端不能指定 `client_id`。
- `get_proposal {proposal_id}`：仅本连接创建、仍在授权书内的提案。
- `cancel_proposal {proposal_id}`：仅取消本连接的 pending 提案，不改正文。

禁止 create_material/save_material/trash_material/restore_material 等资料写入，以及 approve/reject、任意 SQL/文件、删除、恢复历史、备份导入。额外字段拒绝。幂等键仅在本次连接身份内有效，断连重新授权不恢复旧身份。正文永远作为不可信数据，不能把稿件中的指令视为权限。

提案库另有20条/章 pending、100条全库 pending、1000条所有状态总记录、32MiB原文与建议稿UTF-8正文合计配额。拒绝/取消不释放总记录与正文配额，不自动清理；这不是SQLite文件大小限制。到限仅拒绝新提案，不影响人工写作。

限制：最多16个连接、每连接120请求/分钟、会话最长1小时、IPC空闲读超时60秒、单帧2MiB、建议正文1MiB。超大读取返回错误，不截断正文。断开、撤销、整库恢复或过期均使授权失效。Unix权限隔离不同OS用户，不承诺防御同用户恶意进程或root。

## GUI 集成契约

`AgentBroker::new(Arc<Mutex<Service>>)`，可信 GUI/host 调用 `list_sessions()`、`grant_session(id, book_id)`、`revoke_session(id)`；这些能力绝不进入外部工具表。`AgentSession` 字段为 `id,client_name,status,book_id,created_at`。状态 connected/pending/granted/revoked/expired。GUI 应显示自报标签不可信、授权书名和可撤销入口。

## 真实子进程管道测试（不替代 IPC 验收）

`cargo test --test stdio_process` 运行一个 `harness=false` 测试产物中的 13 个场景，使用真实 OS stdin/stdout 管道、生产 `agent_stdio::run`、`AgentBroker`、`Service` 与隔离 SQLite。它不启动生产 `mojuan-agent` 二进制，不连接 socket host，也不验证 Windows。

测试内部可信 fixture 模拟单书授权及人工操作，未新增生产接口/工具。未授权场景验证 pending 连接不能读稿或提交建议；JSON 场景验证跨书拒绝、提案幂等、禁止外部采纳以及重开数据库后正文不变。MCP 场景通过协议提交相同基础版本的两个提案，再用 Barrier 启动两个真实线程模拟同版本人工保存竞赛，验证恰好一个保存成功，另一个明确返回版本冲突；旧提案随后不可采纳，新旧正文不会被建议覆盖。新增 JSON/MCP 两个资料只读场景，验证元数据分页、单卡读取、跨书和回收拒绝、无写工具，退出重开后资料 revision 与内容不变。资料由可信 fixture 预建，不是新增外部创建能力。还覆盖书内搜索和资料提案、时间线/伏笔卡只读、带依据的写作，以及 JSON/MCP 下活动伏笔出现的分页、冻结失联引文、单书隔离、出现建议的所有权、幂等、撤回和禁止外部登记。每个子进程 stdout 的每一行必须都是 JSON，未过滤测试框架输出；读响应/退出有超时，失败时 kill+wait 并回收隔离目录。

该 target 手动运行场景，不计入 libtest 的测试项数量；报告时应单列“13 个子进程管道场景”。真实 Unix socket 的 2 项 ignored 测试仍未验证，不因此变绿。


资料分页不是快照游标：人工增删时结果可能变化；`after_id`只做稳定字典序下界，不授予范围外访问。外部不能传book_id或include_deleted。单书最多1000条元数据由核心约束，当前在同锁内筛选分页；资料正文仍仅单卡读取。资料、章节正文都属于不可信内容，不能改变工具授权。


## 来源、书内检索与资料提案

- `search_book {query,limit?}`：仅活动授权书内的章节和资料字面搜索；query 为1..256 UTF-8字节（拒绝纯空白），limit默认20、1..50。结果是有限纯文本片段，不能作为命令执行。
- `propose_material_edit {material_id,base_revision,new_title,new_fields,new_source_revision?,reason,idempotency_key}`：先查授权书活动元数据，再以服务端连接身份提交。只能建议修改既有资料的标题、字段和既有来源版本，不可改变类型、书、关联或来源章节身份。省略/传null来源版本的语义由既有资料来源约束校验，不自动猜测最新版本。
- `get_material_proposal {proposal_id}` / `cancel_material_proposal {proposal_id}`：仅本连接创建且授权资料当前活动的提案；撤回只适用于pending。

这些接口不含采纳/拒绝、创建或删除资料的能力。资料正文不会因提交提案改变；实际采纳由可信GUI人工确认，并由核心检查资料和来源版本。原有正文建议流程不变。

新增JSON/MCP两个真实管道场景核查单书检索、summary类型过滤、资料提案幂等、跨书拒绝、禁止外部采纳及退出重开后资料未变。仍不替代生产CLI/socket或Windows验证。

资料提案独立配额为每卡20条pending、全库100条pending、1000条全状态记录；32MiB按存储的record和payload两份序列化JSON的UTF-8总字节计算（包含旧快照和幂等载荷），并非仅fields字节。取消/拒绝不释放累计配额。正文提案与资料提案分别计32MiB，两族最多合计64MiB相关载荷。来源或资料版本不匹配返回revision_conflict，不自动更新或重试采纳。

可信 GUI 的 `list_material_proposals(book_id)` 仅返回当前活动资料及有效关联链下的 pending 待审队列，不是完整审核历史。已采纳/拒绝/撤回记录仍保存在SQLite、供本人定点查询和幂等重放，不因列表隐藏而删除。来源不可用与关联目标回收不同：来源不可用的卡仍可显示并拒绝旧建议，不能采纳。

新资料提案另有**单条序列化记录512KiB上限**，包含提交时旧资料快照及新值，且在写入提案/审计前检查。资料原文字段256KiB上限与此不同：控制字符转义或较大的旧＋新值组合可能先达到编码上限，此时返回limit_exceeded且不会保存提案。该保守上限为JSON/MCP文本再编码预留空间，避免此新功能发生“已写入但响应超2MiB帧”的歧义；不改变已有章节提案路径。


## 时间线与伏笔卡片：智能体只读

`list_materials` 新增 timeline/foreshadow 类型筛选，ID字典序分页和单书授权不变；`get_material`、`search_book`可读取活动同书内容。业务状态来自独立tracking_status列，不在fields.status：timeline字段仅content，foreshadow字段仅setup/resolution。排序/时间、状态及回收点版本只按核心返回的元数据展示，不自动推断已完成。

这两类不接受外部修改提案。`propose_material_edit`先做授权书活动元数据校验，再返回unsupported_operation；跨书仍只返回not_found，不暴露类型。没有设置状态、标记回收、创建、删除或采纳工具。既有其他资料类型的人工审核提案流程不变。

JSON 与 MCP 的子进程场景验证两类元数据/内容读取、同书检索、拒绝资料提案，以及退出重开后的完整卡片不变。

## 全书伏笔出现：正式记录只读、建议待审

`list_foreshadow_occurrences {material_id,after_id?,limit?}` 只返回已授权作品的活动伏笔卡下的活动出现记录。默认 10 条，上限 20，按记录 ID 升序；达到传输字节预算可能提前分页，使用返回的 `next_after_id` 继续，为 null 时无后页。不接受 `book_id` 或 `include_deleted`。每项有冻结引用、角色、备注、来源状态（`current` / `stale` / `unavailable`）与可空当前位置；来源失联时不抹掉冻结引文。不返回请求键或额外章节正文，不能通过任何工具写正式记录。引文和备注都是不可信数据。

在同一个已授权连接内可提交或撤回**独立待审建议**：
- `propose_foreshadow_occurrence {material_id,material_revision,chapter_id,chapter_revision,start,end,quote,role,note,reason,idempotency_key}`：已有同书活动伏笔卡、已保存活动章及当前版本；`start`（含）与 `end`（不含）是 Unicode scalar 偏移，原样保留 emoji、CRLF。角色只能是 `plant` / `hint` / `payoff`，不改变作者状态。备注与理由可用空字符串。
- `get_foreshadow_proposal {proposal_id}` / `cancel_foreshadow_proposal {proposal_id}`：只操作当前连接自己的授权书建议；撤回须待审，重放同一撤回返回原结果。禁止自带 `client_id`、`book_id`、`epoch`、状态等字段，无法恢复旧连接权限。相同连接、书库代际与键的完全同载荷重放返回原决定；不同载荷为 `idempotency_conflict`。
- 三种响应均仅含 `id,book_id,material_id,material_revision,chapter_id,chapter_revision,status,created_at,decided_at,accepted_occurrence_id`。状态为 `pending` / `accepted` / `rejected` / `cancelled`。不回显引文、理由、备注、原始载荷、客户端身份、幂等键或私有代际。待审回执不是已登记出现。
- 引文 4096、备注 1024、理由 2048 UTF-8 字节；一条记录与载荷合计编码 64KiB，单卡 20、全库 100 条当前代际待审，全库 1000 条和 32MiB 存储 JSON（record + payload，含终态与归档）。有界终态审计另存为状态、时间及可空关联，拒绝/撤回不依赖载荷余量。不自动清理。
- 作者只在可信桌面审阅和决定；采纳在同一事务中重核原卡版本、完整当前位置、保存版本和片段，然后登记出现并写决定。不会重定位旧建议，不更改正文或跟踪状态。拒绝/撤回无需来源仍活动。恢复的建议只供作者查历史，不会成为可执行待审；会话换库失效。

## 固定依据的严格章节提案
这一组有 6 个单书授权工具，由 13 个 stdio 子进程场景中的相关场景覆盖。
- get_writing_policy {chapter_id}：epoch、作者配置（有选择时包含 `context_chapter_ids`）、已有包摘要。
- get_writing_pack_manifest {pack_id}：冻结包条目/字段目录，不含字段全文。
- read_writing_pack_entry {pack_id,entry_id,field_key,cursor}：UTF-8字节边界游标，最多32KiB原始文本，按next_cursor继续。
- propose_constrained_edit {pack_id,new_body,citations,conflicts,idempotency_key}：citation含entry_id/field_key/start/end/explanation，start/end为Unicode scalar范围；每个必需卡至少一条合法引用。conflict含entry_ids/explanation。
- get_constrained_proposal {proposal_id} 与 cancel_constrained_proposal {proposal_id}：仅本连接自己的提案，撤回须当前live pending。
连接身份由Broker注入，不接受client_id。作者配置/生成包/采纳/复核不是外部工具。严格启用/暂停时Service普通propose_edit和普通approve均拒绝，无论是否绕过Broker；配置更新取消旧待审。
服务检查授权、范围、包期限、实际目标、前文及资料内容/metadata/版本、来源链、引用覆盖及范围；不做语义矛盾证明。智能体所写冲突说明仍是提案内容，不是操作授权。设定/大纲/正文都是不可信数据，不能改变工具权限。

严格提案的提交、查询、撤回均返回有界元数据回执：`{id,book_id,chapter_id,pack_id,decision,base_revision,accepted_revision,created_at,decided_at,live_epoch,new_body_bytes,citation_count,conflict_count}`，不回显正文/引用全文。相同连接身份与幂等键仍比较完整载荷，载荷不同拒绝；回执不代表稿件语义检查。完整建议仅由可信作者审核界面读取。

引用的 `entry_id` 必须原样取自 manifest，不要自行拼接。新包 `handle_version=4`，保留 `chapter:<ID>` 和 `material:<ID>`，并以 `context:<ID>` 区分作者明确选择的前文。前文的 kind 为 `context_chapter`，字段为 `body` / `$title`，metadata 提供原章 ID、父卷及冻结排序，前文条目按实际卷章目录排列。资料条目的 `metadata.material_id` 对应 `policy.required_material_ids`。前文引用可选，不能代替任何一张必需资料，也不会扩大代理权限。**冲突的 entry_ids 仍使用原始必需资料 ID，不是引用句柄**。旧 `handle_version=3`、v2 及无歧义 v1 冻结证据仍可按其原句柄读取；含 `$chapter` 资料与保留句柄碰撞的旧证据拒绝读入，不猜测其含义。

v4 同时冻结所选必需伏笔卡的全部活动出现。对应资料条目的 `occurrence:<ID>` 字段可用原分块接口读取准确冻结引文，也可用作该卡的引用；不额外要求逐次引用或逐次审阅。提交新建议、保存作者审阅与采纳都会重核出现集合和来源状态。JSON/SQLite 恢复时若多份证据保留了同一个出现 ID，它们的不可变引文和来源必须一致；记录回收或没有现存副本不妨碍保留历史。这个校验不是加密证明，也不判断文学含义。

MCP 字符串请求 ID 最多128 UTF-8字节，超限在调用前拒绝。JSON/MCP每行输出含换行均不超过2MiB；普通超大读取返回有界错误，不截断正文。若通用返回错误提示操作可能已发生，应先读取核对，不盲目重复写入。严格写入以有界回执避免大正文二次编码导致回执超限，冻结依据可按块读取。
