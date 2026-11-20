import React, { useEffect, useRef, useState } from "react";
import type { BackupAPI, BackupJob } from "./backupTypes";
import { errorMessage } from "./workflowTypes";
const stoppedRead = Symbol("stopped backup status read");
type ShelfReload = { jobId: string; pending: boolean; error: string };
const unfinished = (job: BackupJob | null) =>
  !!job && ["running", "prepared", "committing"].includes(job.status);
const phaseNames: Record<string, string> = {
  waiting: "等待处理",
  prepare: "准备候选",
  prepared: "等待确认",
  awaiting_confirmation: "等待确认",
  validate: "校验备份",
  import: "导入数据",
  export: "导出备份",
  recovery: "保存恢复前留底",
  commit: "提交恢复",
};
const statusNames = {
  running: "处理中",
  prepared: "校验完成，等待确认",
  committing: "正在提交，不能取消",
  done: "已完成",
  cancelled: "已取消",
  failed: "失败",
  expired: "候选已过期",
};
export default function BackupPanel({
  api,
  onClose,
  onPendingChange,
  onRestored,
}: {
  api: BackupAPI;
  onClose: () => void;
  onPendingChange: (pending: boolean) => void;
  onRestored: () => Promise<void>;
}) {
  const [job, setJob] = useState<BackupJob | null>(null),
    [busy, setBusy] = useState(true),
    [uncertain, setUncertain] = useState(false),
    [readWait, setReadWait] = useState<AbortController | null>(null),
    [error, setError] = useState(""),
    [notice, setNotice] = useState(""),
    [confirm, setConfirm] = useState<{ id: string; token: string } | null>(
      null,
    ),
    [refresh, setRefresh] = useState(0),
    [shelfReload, setShelfReload] = useState<ShelfReload | null>(null);
  const mounted = useRef(true),
    generation = useRef(0),
    acting = useRef(false),
    uncertainRef = useRef(false),
    readWaitRef = useRef<AbortController | null>(null),
    jobRef = useRef(job),
    panel = useRef<HTMLElement>(null),
    pendingRef = useRef(false),
    onCloseRef = useRef(onClose),
    pendingCallback = useRef(onPendingChange),
    restoreCallback = useRef(onRestored),
    shelfReloadRef = useRef<ShelfReload | null>(null),
    reloaded = useRef(new Set<string>());
  onCloseRef.current = onClose;
  pendingCallback.current = onPendingChange;
  restoreCallback.current = onRestored;
  const pending = busy || uncertain || unfinished(job);
  pendingRef.current = pending;
  useEffect(() => {
    pendingCallback.current(pending);
  }, [pending]);
  useEffect(() => {
    mounted.current = true;
    void action("current");
    return () => {
      mounted.current = false;
      generation.current++;
      const wait = readWaitRef.current;
      readWaitRef.current = null;
      wait?.abort();
      shelfReloadRef.current = null;
      acting.current = false;
      pendingCallback.current(false);
    };
  }, []);
  const requestClose = () => {
    if (acting.current || pendingRef.current) {
      setError(
        uncertainRef.current
          ? "作业结果未确认，请刷新状态核对后再关闭。"
          : "请等待作业结束，或取消并确认终态后再关闭。",
      );
      return;
    }
    onCloseRef.current();
  };
  useEffect(() => {
    const root = panel.current!,
      previous = document.activeElement as HTMLElement | null;
    const focusables = () =>
      Array.from(
        root.querySelectorAll<HTMLElement>(
          'button:not(:disabled),[tabindex="0"]',
        ),
      );
    const focusFirst = () => (focusables()[0] ?? root).focus();
    const guard = (e: FocusEvent) => {
      if (!root.contains(e.target as Node)) focusFirst();
    };
    const keys = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        requestClose();
      }
      if (e.key === "Tab") {
        const nodes = focusables(),
          first = nodes[0] ?? root,
          last = nodes.at(-1) ?? root;
        if (
          e.shiftKey &&
          (document.activeElement === first || document.activeElement === root)
        ) {
          e.preventDefault();
          last.focus();
        } else if (
          !e.shiftKey &&
          (document.activeElement === last || document.activeElement === root)
        ) {
          e.preventDefault();
          first.focus();
        }
      }
    };
    focusFirst();
    document.addEventListener("focusin", guard);
    root.addEventListener("keydown", keys);
    return () => {
      document.removeEventListener("focusin", guard);
      root.removeEventListener("keydown", keys);
      if (previous?.isConnected) previous.focus();
    };
  }, []);
  function setUnknown(value: boolean) {
    uncertainRef.current = value;
    setUncertain(value);
  }
  function acceptCurrentEmpty() {
    jobRef.current = null;
    setJob(null);
    setConfirm(null);
    setUnknown(false);
  }
  function accept(next: BackupJob, expectedId?: string) {
    if (expectedId && next.id !== expectedId)
      throw new Error("返回作业标识不匹配，需重新查询状态。");
    jobRef.current = next;
    setJob(next);
    setUnknown(false);
    setConfirm((old) =>
      old &&
      next.status === "prepared" &&
      old.id === next.id &&
      old.token === next.confirmation_token
        ? old
        : null,
    );
  }
  useEffect(() => {
    // A committed render's effect can run after an action starts but before busy updates.
    if (
      acting.current ||
      uncertainRef.current ||
      !job ||
      busy ||
      uncertain ||
      !unfinished(job)
    ) return;
    const id = job.id,
      seq = ++generation.current;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let disposed = false;
    const valid = () =>
      mounted.current && !disposed && seq === generation.current;
    async function poll() {
      try {
        const next = await api.status(id);
        if (!valid()) return;
        accept(next, id);
        if (unfinished(next)) timer = setTimeout(() => void poll(), 1000);
      } catch (e) {
        if (valid()) {
          setError(errorMessage(e));
          setUnknown(true);
          setNotice("自动刷新已暂停；作业结果未确认，请手动刷新状态。");
        }
      }
    }
    timer = setTimeout(() => void poll(), 1000);
    return () => {
      if (timer) clearTimeout(timer);
      disposed = true;
    };
  }, [api, job?.id, busy, uncertain, refresh]);
  async function reload(id: string) {
    if (
      !mounted.current ||
      jobRef.current?.id !== id ||
      (shelfReloadRef.current?.jobId === id && shelfReloadRef.current.pending)
    ) return;
    // Status reconciliation can temporarily hide the owning restore. Only a
    // newer reload (or unmount), not a change to the current job, supersedes it.
    const attempt: ShelfReload = { jobId: id, pending: true, error: "" };
    shelfReloadRef.current = attempt;
    setShelfReload(attempt);
    let failure = "";
    try {
      await restoreCallback.current();
    } catch (e) {
      failure = "恢复已完成，书架刷新失败：" + errorMessage(e);
    } finally {
      if (mounted.current && shelfReloadRef.current === attempt) {
        const settled = { jobId: id, pending: false, error: failure };
        shelfReloadRef.current = settled;
        setShelfReload(settled);
      }
    }
  }
  useEffect(() => {
    if (
      job?.kind === "restore" &&
      job.status === "done" &&
      jobRef.current?.id === job.id &&
      !reloaded.current.has(job.id)
    ) {
      reloaded.current.add(job.id);
      void reload(job.id);
    }
  }, [job?.id, job?.status]);
  async function readStatus<T>(request: () => Promise<T>) {
    const wait = new AbortController();
    readWaitRef.current = wait;
    setReadWait(wait);
    const stopped = new Promise<typeof stoppedRead>((resolve) => {
      wait.signal.addEventListener("abort", () => resolve(stoppedRead), {
        once: true,
      });
    });
    try {
      // Stop only this UI await. The IPC is not cancelled and its late rejection is consumed.
      return await Promise.race([stopped, request()]);
    } finally {
      if (readWaitRef.current === wait) {
        readWaitRef.current = null;
        setReadWait(null);
      }
    }
  }
  function stopRead(wait: AbortController) {
    if (readWaitRef.current !== wait) return;
    generation.current++;
    readWaitRef.current = null;
    pendingRef.current = true;
    setUnknown(true);
    acting.current = false;
    setReadWait(null);
    setBusy(false);
    setNotice(
      "已停止本次状态等待。后台作业可能仍在运行，请刷新状态核对；不会重新提交恢复。",
    );
    wait.abort();
  }
  async function action(
    kind:
      | "export"
      | "prepare"
      | "cancel"
      | "confirm"
      | "status"
      | "json"
      | "current",
  ) {
    if (!mounted.current || acting.current) return;
    const target = jobRef.current,
      approval = confirm;
    if (["export", "prepare", "json"].includes(kind) && pendingRef.current)
      return;
    if (
      (kind === "cancel" || kind === "confirm" || kind === "status") &&
      !target
    )
      return;
    if (
      kind === "cancel" &&
      (uncertainRef.current ||
        target!.status === "committing" ||
        target!.cancel_requested)
    )
      return;
    if (
      kind === "confirm" &&
      (!approval ||
        target!.status !== "prepared" ||
        target!.id !== approval.id ||
        target!.confirmation_token !== approval.token ||
        uncertainRef.current)
    )
      return;
    if (
      kind === "confirm" &&
      target?.expires_at &&
      Date.now() >= target.expires_at * 1000
    ) {
      setError("候选已过期，请刷新状态后重新选择。");
      setConfirm(null);
      return;
    }
    acting.current = true;
    generation.current++;
    setBusy(true);
    setError("");
    setNotice("");
    const seq = generation.current;
    const valid = () => mounted.current && seq === generation.current;
    try {
      if (kind === "json") {
        const path = await api.exportJson();
        if (valid() && path) setNotice("兼容 JSON 备份已保存：" + path);
        return;
      }
      const next =
        kind === "current"
          ? await readStatus(() => api.current())
          : kind === "export"
            ? await api.startExport()
            : kind === "prepare"
              ? await api.startPrepare()
              : kind === "cancel"
                ? await api.cancel(target!.id)
                : kind === "confirm"
                  ? await api.confirm(target!.id, approval!.token, true)
                  : await readStatus(() => api.status(target!.id));
      if (!valid() || next === stoppedRead) return;
      if (next) {
        accept(
          next,
          ["cancel", "confirm", "status"].includes(kind)
            ? target!.id
            : undefined,
        );
        if (["export", "prepare"].includes(kind)) setConfirm(null);
        if (kind === "cancel" && unfinished(next) && !next.cancel_requested)
          setNotice("已请求取消，等待服务端确认终态。");
      } else if (kind === "current") {
        acceptCurrentEmpty();
      } else if (kind === "export" || kind === "prepare") {
        setNotice("已取消文件选择。");
      }
    } catch (e) {
      if (!valid()) return;
      setError(errorMessage(e));
      if (kind === "export" || kind === "prepare") {
        // A previous terminal job cannot answer whether this start registered a new one.
        jobRef.current = null;
        setJob(null);
        setConfirm(null);
        setUnknown(true);
        try {
          const current = await readStatus(() => api.current());
          if (!valid() || current === stoppedRead) return;
          if (current) accept(current);
          else acceptCurrentEmpty();
        } catch {
          if (valid()) setNotice("无法确认是否已启动，请刷新当前任务。");
        }
      } else if (kind === "current") {
        setUnknown(true);
        setNotice("无法确认当前作业，请刷新当前任务。");
      } else if (target && ["cancel", "confirm", "status"].includes(kind)) {
        setUnknown(true);
        setNotice("作业结果未确认，请手动刷新状态；不要重复提交恢复。");
      }
    } finally {
      if (valid()) {
        acting.current = false;
        setBusy(false);
        setRefresh((n) => n + 1);
      }
    }
  }
  const expired = !!job?.expires_at && Date.now() >= job.expires_at * 1000;
  const jobReload = shelfReload?.jobId === job?.id ? shelfReload : null;
  return (
    <div className="review-backdrop">
      <section
        className="backup-panel"
        role="dialog"
        aria-modal="true"
        aria-label="备份与恢复"
        ref={panel}
        tabIndex={-1}
      >
        <header>
          <h2>备份与恢复</h2>
          <button aria-label="关闭备份面板" onClick={requestClose}>
            ×
          </button>
        </header>
        <p>只在本机处理。SQLite 文件上限512 MiB，兼容JSON上限64 MiB。</p>
        <nav>
          <button disabled={pending} onClick={() => void action("export")}>
            创建SQLite备份
          </button>
          <button disabled={pending} onClick={() => void action("prepare")}>
            选择备份并校验
          </button>
          <button disabled={pending} onClick={() => void action("json")}>
            导出兼容JSON
          </button>
        </nav>
        {readWait ? (
          <>
            <p>正在查询作业状态…</p>
            <button onClick={() => stopRead(readWait)}>停止本次状态等待</button>
          </>
        ) : (
          busy && <p>正在请求，请勿重复操作…</p>
        )}
        {error && (
          <p role="alert" className="review-error">
            {error}
          </p>
        )}
        {notice && <p role="status">{notice}</p>}
        {uncertain && !job && (
          <button disabled={busy} onClick={() => void action("current")}>
            刷新当前任务
          </button>
        )}
        {job && (
          <section aria-label="备份作业">
            <h3>{statusNames[job.status]}</h3>
            {job.source_name && <p>{job.source_name}</p>}
            <p>
              阶段：{phaseNames[job.progress.phase] ?? "处理中"} · 已处理{" "}
              {job.progress.completed}
              {job.progress.total !== null ? ` / ${job.progress.total}` : ""}
            </p>
            {job.cancel_requested && unfinished(job) && (
              <p>已请求取消，等待服务端确认终态。</p>
            )}
            {job.error_message && <p role="alert">{job.error_message}</p>}
            {job.error_code && <p>原因：{job.error_code}</p>}
            {job.status === "prepared" && job.preview && (
              <><p>写作约束 {job.preview.writing_policies??0} 项，冻结依据 {job.preview.writing_packs??0} 个，严格建议证据 {job.preview.writing_proposals??0} 条。</p>{!job.preview.writing_policies&&<p>此备份不含写作约束，将替换现有约束配置。</p>}
                {(job.preview.foreshadow_proposals??0)>0 && <p>伏笔出现建议 {job.preview.foreshadow_proposals} 条。恢复后仅保留证据，不会恢复待审执行权限。</p>}
                <dl className="backup-counts">
                  <div>
                    <dt>格式</dt>
                    <dd>{job.preview.format}</dd>
                  </div>
                  {(
                    [
                      "books",
                      "volumes",
                      "chapters",
                      "history",
                      "materials",
                      "version_labels",
                      "material_history",
                    ] as const
                  ).map((key, index) => (
                    <div key={key}>
                      <dt>
                        {
                          [
                            "作品",
                            "卷",
                            "章节",
                            "历史版本",
                            "资料",
                            "版本名称",
                            "资料内容历史",
                          ][index]
                        }
                      </dt>
                      <dd>{job.preview![key] ?? 0}</dd>
                    </div>
                  ))}
                  <div>
                    <dt>规范数据大小</dt>
                    <dd>{job.preview.canonical_bytes} 字节</dd>
                  </div>
                </dl>
                <p>
                  计数包含回收项。恢复将替换整个书库，保留备份内的正式数据和历史，包括伏笔出现建议及其决定（仅供查看）。任务、普通正文和资料修改提案及其操作审计不从备份恢复，旧授权失效。
                </p>
                <p>
                  提交前会保存当前书库的已验证快照；留底失败不会写入新书库。
                </p>
                {job.expires_at && (
                  <p>
                    候选有效至{" "}
                    {new Date(job.expires_at * 1000).toLocaleString()}。
                  </p>
                )}
                {expired && <p>候选已到期，请刷新状态后重新选择。</p>}
                <button
                  disabled={
                    busy ||
                    uncertain ||
                    expired ||
                    !job.confirmation_token ||
                    !!confirm
                  }
                  onClick={() =>
                    setConfirm({ id: job.id, token: job.confirmation_token! })
                  }
                >
                  恢复此备份
                </button>
              </>
            )}
            {job.status === "prepared" && (
              <button
                disabled={busy || uncertain || job.cancel_requested}
                onClick={() => void action("cancel")}
              >
                放弃候选
              </button>
            )}
            {job.status === "running" && (
              <button
                disabled={busy || uncertain || job.cancel_requested}
                onClick={() => void action("cancel")}
              >
                请求取消
              </button>
            )}
            {(unfinished(job) || uncertain) && (
              <button disabled={busy} onClick={() => void action("status")}>
                刷新作业状态
              </button>
            )}
            {confirm && job.status === "prepared" && (
              <div
                className="review-confirm"
                role="alertdialog"
                aria-label="确认恢复书库"
              >
                <p>
                  确认使用已校验候选替换整个书库，并清除上述运行记录与授权？包括替换当前启用或暂停的写作约束。备份中的严格建议仅作为证据归档，启用配置恢复为暂停；旧备份可能没有这些配置。
                </p>
                <button disabled={busy} onClick={() => setConfirm(null)}>
                  返回预览
                </button>
                <button
                  disabled={busy || uncertain || expired}
                  onClick={() => void action("confirm")}
                >
                  确认替换整个书库
                </button>
              </div>
            )}
            {job.status === "done" && (
              <>
                <p>
                  {job.kind === "restore"
                    ? "书库恢复已完成。"
                    : "SQLite备份已完成。"}
                </p>
                {job.result?.output_path && <p>{job.result.output_path}</p>}
                {job.result?.recovery_path && (
                  <>
                    <h4>恢复前留底</h4>
                    <p>{job.result.recovery_path}</p>
                  </>
                )}
                {jobReload?.pending && <p>恢复成功，正在重新加载书架…</p>}
                {jobReload?.error && (
                  <>
                    <p role="alert">{jobReload.error}</p>
                    <button onClick={() => void reload(job.id)}>
                      重新加载书架
                    </button>
                  </>
                )}
              </>
            )}
          </section>
        )}
      </section>
    </div>
  );
}
