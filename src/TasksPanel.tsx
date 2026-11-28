import React, { useEffect, useRef, useState } from "react";
import type { Task, TasksAPI, TaskDetail, TaskStatus } from "./taskTypes";
import { errorMessage } from "./workflowTypes";
import { Dialog } from "./ui/Dialog";
const labels: Record<TaskStatus, string> = {
  queued: "排队中",
  running: "检查中",
  completed: "已完成",
  cancelled: "已取消",
  failed: "失败",
  interrupted: "已中断",
};
const active = (t: Task) => t.status === "queued" || t.status === "running";
const sourceLabel = (s: string | null) =>
  ({
    none: "未记录",
    current: "版本未变化",
    stale: "待复核",
    unavailable: "不可用",
  })[s ?? "none"] ?? s;
const kindLabel: Record<string, string> = {
  setting: "设定",
  character: "角色",
  outline: "大纲",
  summary: "摘要",
  timeline: "时间线",
  foreshadow: "伏笔",
};
const trackingLabel: Record<string, string> = {
  planned: "计划中",
  written: "已发生",
  planted: "已铺设",
  resolved: "已回收",
  abandoned: "已放弃",
};
export default function TasksPanel({
  bookId,
  api,
  onClose,
  onOpen,
}: {
  bookId: string;
  api: TasksAPI;
  onClose: () => void;
  onOpen: (target: { kind: "chapter" | "material"; id: string }) => void;
}) {
  const [tasks, setTasks] = useState<Task[]>([]),
    [selected, setSelected] = useState(""),
    [detail, setDetail] = useState<TaskDetail | null>(null),
    [busy, setBusy] = useState(false),
    [loading, setLoading] = useState(false),
    [error, setError] = useState(""),
    [notice, setNotice] = useState(""),
    [refresh, setRefresh] = useState(0),
    [tab, setTab] = useState<"chapter" | "issues">("chapter"),
    [page, setPage] = useState(0),
    [scopeBook, setScopeBook] = useState(bookId);
  const mounted = useRef(true),
    bookRef = useRef(bookId),
    actionGeneration = useRef(0),
    listGeneration = useRef(0),
    detailGeneration = useRef(0),
    working = useRef(false),
    selectedRef = useRef(selected),
    keys = useRef(new Map<string, string>()),
    closeRef = useRef(onClose);
  bookRef.current = bookId;
  selectedRef.current = scopeBook === bookId ? selected : "";
  closeRef.current = onClose;
  const merge = (task: Task) =>
    setTasks((old) =>
      [task, ...old.filter((t) => t.id !== task.id)].sort(
        (a, b) => b.created_at - a.created_at || a.id.localeCompare(b.id),
      ),
    );
  async function loadList() {
    const seq = ++listGeneration.current;
    try {
      const list = await api.list(bookId);
      if (
        !mounted.current ||
        bookRef.current !== bookId ||
        seq !== listGeneration.current
      )
        return;
      const valid = list.filter((t) => t.book_id === bookId);
      setTasks(valid);
      if (!selectedRef.current && valid[0]) setSelected(valid[0].id);
    } catch (e) {
      if (
        mounted.current &&
        bookRef.current === bookId &&
        seq === listGeneration.current
      )
        setError(errorMessage(e));
    }
  }
  useEffect(() => {
    mounted.current = true;
    actionGeneration.current++;
    working.current = false;
    keys.current.clear();
    setScopeBook(bookId);
    setTasks([]);
    setSelected("");
    selectedRef.current = "";
    setDetail(null);
    setPage(0);
    setTab("chapter");
    setBusy(false);
    setLoading(false);
    setError("");
    setNotice("");
    void loadList();
    return () => {
      mounted.current = false;
      listGeneration.current++;
      detailGeneration.current++;
      actionGeneration.current++;
    };
  }, [bookId, api]);
  useEffect(() => {
    if (!selected || busy || scopeBook !== bookId) return;
    const seq = ++detailGeneration.current;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let disposed = false;
    const valid = () =>
      mounted.current &&
      bookRef.current === bookId &&
      !disposed &&
      seq === detailGeneration.current;
    async function poll() {
      if (!valid()) return;
      setLoading(true);
      try {
        const next = await api.get(bookId, selected);
        if (!valid()) return;
        if (next.task.book_id !== bookId || next.task.id !== selected)
          throw new Error("任务所属作品或标识不匹配");
        setDetail(next);
        setNotice((old) => (old.startsWith("自动刷新已暂停") ? "" : old));
        merge(next.task);
        if (active(next.task)) timer = setTimeout(() => void poll(), 1000);
      } catch (e) {
        if (valid()) {
          setError(errorMessage(e));
          setNotice("自动刷新已暂停，请手动刷新报告或重新打开面板。");
        }
      } finally {
        if (valid()) setLoading(false);
      }
    }
    void poll();
    return () => {
      disposed = true;
      if (timer) clearTimeout(timer);
    };
  }, [bookId, api, selected, refresh, busy, scopeBook]);
  function choose(id: string) {
    detailGeneration.current++;
    setSelected(id);
    setDetail(null);
    setPage(0);
    setError("");
    setNotice("");
    setRefresh((n) => n + 1);
  }
  async function action(op: "start" | "cancel" | "resume" | "retry") {
    if (working.current || scopeBook !== bookId) return;
    const seq = ++actionGeneration.current;
    const valid = () =>
      mounted.current &&
      bookRef.current === bookId &&
      seq === actionGeneration.current;
    const id = selected;
    if (op !== "start" && !id) return;
    working.current = true;
    detailGeneration.current++;
    listGeneration.current++;
    setBusy(true);
    setError("");
    setNotice("");
    const keyId = `${bookId}:${op}:${op === "start" ? "" : id}`;
    let key = keys.current.get(keyId);
    if (!key) {
      key = crypto.randomUUID();
      keys.current.set(keyId, key);
    }
    try {
      const next =
        op === "start"
          ? await api.start(bookId, key)
          : op === "retry"
            ? await api.retry(bookId, id, key)
            : op === "resume"
              ? await api.resume(bookId, id)
              : await api.cancel(bookId, id);
      if (!valid()) return;
      if (
        next.book_id !== bookId ||
        (op !== "start" && op !== "retry" && next.id !== id)
      )
        throw new Error("任务所属作品或标识不匹配");
      keys.current.delete(keyId);
      merge(next);
      setSelected(next.id);
      setDetail((old) => ({
        task: next,
        items: old?.task.id === next.id ? old.items : [],
      }));
      setPage(0);
      if (op === "cancel" && active(next))
        setNotice("已请求取消，等待任务返回最终状态。");
      else if (op === "cancel") setNotice(`任务${labels[next.status]}。`);
      else if (op === "resume") setNotice("已请求继续检查点。");
    } catch (e) {
      if (valid())
        setError(
          errorMessage(e) +
            (op === "resume" ? " 若输入已变化，请重新检查。" : ""),
        );
    } finally {
      if (valid()) {
        working.current = false;
        setBusy(false);
        setRefresh((n) => n + 1);
      }
    }
  }
  const current =
    scopeBook === bookId && detail?.task.book_id === bookId
      ? detail.task
      : undefined;
  const items = current ? detail!.items : [];
  const visibleTasks = scopeBook === bookId ? tasks : [];
  const chapters = items.filter((i) => i.kind === "chapter");
  const issues = items.filter((i) => i.needs_review);
  const rows = tab === "chapter" ? chapters : issues;
  const pages = Math.max(1, Math.ceil(rows.length / 100));
  const safePage = Math.min(page, pages - 1);
  const stale = items.filter(
      (i) => i.source_status === "stale" || i.payoff_status === "stale",
    ).length,
    unavailable = items.filter(
      (i) =>
        i.source_status === "unavailable" || i.payoff_status === "unavailable",
    ).length,
    unresolved = items.filter(
      (i) =>
        i.material_kind === "foreshadow" &&
        (i.tracking_status === "planned" || i.tracking_status === "planted"),
    ).length;
  return (
    <Dialog
      title="作品上下文检查"
      label="检查任务"
      subtitle="只做本地统计与状态检查，不改稿、不判断剧情质量。关闭面板后任务继续。"
      closeLabel="关闭检查任务"
      className="tasks-panel"
      size="xl"
      onClose={onClose}
    >
      <div className="tasks-body">
        <nav className="task-actions">
          <button
            className="primary"
            disabled={busy}
            onClick={() => void action("start")}
          >
            新建检查
          </button>
          <button disabled={busy} onClick={() => void loadList()}>
            刷新任务列表
          </button>
        </nav>
        {error && (
          <p role="alert" className="review-error">
            {error}
          </p>
        )}
        {notice && <p role="status">{notice}</p>}
        <div className="materials-layout">
          <aside aria-label="检查记录">            {visibleTasks.map((t) => (
              <button
                key={t.id}
                className="task-list-item"
                disabled={busy}
                aria-label={`任务 ${t.id} · ${labels[t.status]}`}
                title={t.id}
                aria-pressed={t.id === selected}
                onClick={() => choose(t.id)}
              >
                <span className="task-record-time">
                  {new Date(t.created_at * 1000).toLocaleString(undefined, {
                    month: "2-digit",
                    day: "2-digit",
                    hour: "2-digit",
                    minute: "2-digit",
                  })}
                </span>
                <span className="task-record-meta">
                  <span className="task-record-status">{labels[t.status]}</span>
                  <small>#{t.id.slice(0, 8)}</small>
                </span>
              </button>
            ))}
            {visibleTasks.length === 0 && <p>暂无检查记录。</p>}
          </aside>
          <main aria-busy={loading} style={{ marginLeft: 0 }}>
            {current ? (
              <>
                <div className="task-report-heading">
                  <h3>{labels[current.status]}</h3>
                  <p>
                    已检查 {current.done} / {current.total} 项
                  </p>
                </div>
                <p className="task-report-caption">
                  每项为一个章节或一条资料，进度以实际提交为准。
                </p>
                <p className="task-report-caption">
                  检查于
                  {new Date(
                    (current.finished_at ?? current.updated_at) * 1000,
                  ).toLocaleString()}
                  · 当时版本，不随以后稿件修改自动更新。
                </p>
                {(current.status !== "completed" ||
                  items.length !== current.done) && (
                  <p className="material-unsaved">
                    部分结果，检查未完成，不能作为全书结论。
                  </p>
                )}
                {current.error_message && (
                  <p role="alert">{current.error_message}</p>
                )}
                {current.reason_code && <p>原因：{current.reason_code}</p>}
                <div className="task-actions">
                  {active(current) && (
                    <button
                      disabled={busy}
                      onClick={() => void action("cancel")}
                    >
                      取消检查
                    </button>
                  )}
                  {current.status === "interrupted" && (
                    <button
                      disabled={busy}
                      onClick={() => void action("resume")}
                    >
                      继续检查点
                    </button>
                  )}
                  {!active(current) && (
                    <button
                      disabled={busy}
                      onClick={() => void action("retry")}
                    >
                      重新检查
                    </button>
                  )}
                  <button
                    disabled={busy || loading}
                    onClick={() => {
                      setError("");
                      setRefresh((n) => n + 1);
                    }}
                  >
                    刷新当前报告
                  </button>
                </div>
                {items.length === 0 ? (
                  <p>尚无检查结果，不代表没有问题。</p>
                ) : (
                  <>
                    <p>
                      已统计章节 {chapters.length} · 非空白字符{" "}
                      {chapters.reduce(
                        (sum, i) => sum + (i.char_count ?? 0),
                        0,
                      )}{" "}
                      · 来源待复核资料 {stale} · 来源不可用资料 {unavailable} ·
                      未回收伏笔 {unresolved}
                    </p>
                    <nav>
                      <button
                        aria-pressed={tab === "chapter"}
                        onClick={() => {
                          setTab("chapter");
                          setPage(0);
                        }}
                      >
                        章节字数
                      </button>
                      <button
                        aria-pressed={tab === "issues"}
                        onClick={() => {
                          setTab("issues");
                          setPage(0);
                        }}
                      >
                        待处理项（{issues.length}）
                      </button>
                    </nav>
                    {rows
                      .slice(safePage * 100, (safePage + 1) * 100)
                      .map((i) => (
                        <article
                          className="task-report-item"
                          key={`${i.kind}:${i.item_id}:${i.ordinal}`}
                        >
                          <button
                            onClick={() =>
                              onOpen({ kind: i.kind, id: i.item_id })
                            }
                          >
                            {i.title}
                          </button>
                          <p>
                            {i.kind === "chapter"
                              ? `章节 · ${i.char_count ?? "未统计"} 字`
                              : (kindLabel[i.material_kind ?? ""] ??
                                "资料")}{" "}
                            · 检查时版本 {i.revision}
                          </p>
                          {i.kind === "material" && (
                            <p>
                              来源：{sourceLabel(i.source_status)} · 回收来源：
                              {sourceLabel(i.payoff_status)}
                              {i.tracking_status
                                ? ` · ${trackingLabel[i.tracking_status] ?? i.tracking_status}`
                                : ""}
                            </p>
                          )}
                        </article>
                      ))}
                    {rows.length === 0 && (
                      <p>
                        {current.status === "completed"
                          ? "此分类无检查条目。"
                          : "已检查部分中暂无此类条目。"}
                      </p>
                    )}
                    {pages > 1 && (
                      <nav>
                        <button
                          disabled={safePage === 0}
                          onClick={() => setPage(safePage - 1)}
                        >
                          上一页
                        </button>
                        <span>
                          第 {safePage + 1} / {pages} 页，每页最多100项
                        </span>
                        <button
                          disabled={safePage === pages - 1}
                          onClick={() => setPage(safePage + 1)}
                        >
                          下一页
                        </button>
                      </nav>
                    )}
                  </>
                )}
              </>
            ) : (
              <p>{loading ? "正在读取检查记录…" : "选择一条检查记录。"}</p>
            )}
          </main>
        </div>
      </div>
    </Dialog>
  );
}
