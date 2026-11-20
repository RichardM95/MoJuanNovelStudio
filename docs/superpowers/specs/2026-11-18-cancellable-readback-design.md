# 可停止等待的伏笔只读核对

用户明确要求修复只读核对 IPC 永不返回导致无法退出，保持未知写入不能重发或误判。基线8a987ce，云端、不外发稿件、不接模型、不用本机、Windows暂停。

根因：ForeshadowProposalPanel.reconcile 与 decide 共用 writeLock/busy，并在 finally 解除；任何一次getEpoch/get不返回，finally不会执行，父modal/nativeclose都被busy永久锁住。目标不是取消底层事务，而是可中止前端对只读检查的等待。

选择显式“停止等待核对”而非自动重试或把整个调用超时判失败。对核对的首个epoch读取、detail读取、末尾epoch读取三处均适用。取消后保留原App内存未决attempt及已知证据，只解除当前只读等待的交互阻塞，可暂离/重开继续只读核对；不调用decide、不取消建议、不更改正式记录。准确提示“停止等待不代表原写入失败”。实际采纳/拒绝写入仍沿既有保护，本轮不放宽其关闭边界。

每次只读核对拥有独立代数/身份，停止、换书/换epoch、关闭/重开及卸载都使该次失效。每个await后在任何memory/界面/回执/刷新副作用之前校验；迟到resolve/reject/finally不得恢复旧终态、解除新调用锁、显示旧错误、触发新库读取或onChanged。不声称能abort Tauri传输；底层promise可继续执行，但其结果不再影响本次UI。

如果停止前已收到精确terminal并保留，它仍以旧epoch未最终核实的证据展示，原attempt不得因此清除；后续重新核对确认epoch后才settle。停止发生在读回前则不制造回执。再次核对有新generation，并发点击不启动两次活跃核对；旧任务finally不得清除新task。被取消读任务不造成unhandled rejection。

检查相邻list/detail/comparison加载、父modal关闭和App beforeunload/nativeblocking，确保只读等待不会新增长期阻塞，不能用关闭重开绕过target隔离。无需新服务端schema、数据迁移或网络依赖；本阶段有意义验收是完整恢复路径的可中止性与迟到结果隔离，而非新增小说功能。
