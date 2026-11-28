import React, { useEffect, useRef, useState } from "react";
import type {
  SplitAPI,
  SplitTarget,
  SplitPreview,
  SplitOutcome,
} from "./splitTypes";
import { errorMessage } from "./workflowTypes";
import { Dialog } from "./ui/Dialog";
export default function SplitPanel({
  api,
  target,
  onClose,
  onPendingChange,
  onCommitted,
}: {
  api: SplitAPI;
  target: SplitTarget;
  onClose: () => void;
  onPendingChange: (p: boolean) => void;
  onCommitted: (o?: SplitOutcome) => Promise<void>;
}) {
  const [title, setTitle] = useState(target.title + "（续）"),
    [preview, setPreview] = useState<SplitPreview | null>(null),
    [detail, setDetail] = useState<string | null>(null),
    [confirm, setConfirm] = useState(false),
    [busy, setBusy] = useState(false),
    [needsRead, setNeedsRead] = useState(false),
    [done, setDone] = useState(false),
    [message, setMessage] = useState("");
  const mounted = useRef(true),
    generation = useRef(0),
    candidate = useRef<string | null>(null),
    lock = useRef(false),
    write = useRef(false),
    mustRead = useRef(false),
    settled = useRef(false),
    outcome = useRef<SplitOutcome | undefined>(undefined),
    commitError = useRef(""),
    callbacks = useRef({ onClose, onPendingChange, onCommitted });
  callbacks.current = { onClose, onPendingChange, onCommitted };
  const discard = (id: string) => {
    void api.discard(id).catch(() => {});
  };
  const pending = (p: boolean) => {
    setBusy(p);
    callbacks.current.onPendingChange(p || mustRead.current);
  };
  function close() {
    if (lock.current || mustRead.current) return;
    callbacks.current.onClose();
  }
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      generation.current++;
      if (candidate.current) discard(candidate.current);
      callbacks.current.onPendingChange(false);
    };
  }, [api]);
  function change(value: string) {
    if (write.current || mustRead.current || settled.current) return;
    generation.current++;
    if (candidate.current) discard(candidate.current);
    candidate.current = null;
    setTitle(value);
    setPreview(null);
    setDetail(null);
    setConfirm(false);
    setMessage("");
  }
  async function prepare() {
    if (lock.current || settled.current) return;
    lock.current = true;
    pending(true);
    const seq = ++generation.current;
    if (candidate.current) discard(candidate.current);
    candidate.current = null;
    setPreview(null);
    setDetail(null);
    setConfirm(false);
    setMessage("");
    try {
      const p = await api.prepare(
        target.bookId,
        target.chapterId,
        target.revision,
        target.offset,
        title,
      );
      if (!mounted.current || seq !== generation.current) {
        discard(p.id);
        return;
      }
      candidate.current = p.id;
      setPreview(p);
    } catch (e) {
      if (mounted.current && seq === generation.current)
        setMessage(errorMessage(e));
    } finally {
      lock.current = false;
      if (mounted.current) pending(false);
    }
  }
  async function show(side: "before" | "after") {
    if (lock.current || !preview) return;
    lock.current = true;
    pending(true);
    const seq = generation.current;
    try {
      const body = await api.detail(preview.id, side);
      if (mounted.current && seq === generation.current) setDetail(body);
    } catch (e) {
      if (mounted.current && seq === generation.current)
        setMessage(errorMessage(e));
    } finally {
      lock.current = false;
      if (mounted.current) pending(false);
    }
  }
  async function reload() {
    if (lock.current) return;
    lock.current = true;
    pending(true);
    try {
      await callbacks.current.onCommitted(outcome.current);
      if (mounted.current) {
        mustRead.current = false;
        setNeedsRead(false);
        setDone(true);
        setMessage(
          outcome.current
            ? "拆章已完成"
            : (commitError.current
                ? "未能确认拆章：" + commitError.current + "。"
                : "") + "已重新读取，请核对目录与正文，未自动重试拆章。",
        );
      }
    } catch (e) {
      if (mounted.current) setMessage(errorMessage(e));
    } finally {
      lock.current = false;
      if (mounted.current) pending(false);
    }
  }
  function ask() {
    if (!preview || lock.current) return;
    if (preview.expires_at * 1000 <= Date.now()) {
      setMessage("预览已过期，请重新生成");
      return;
    }
    setConfirm(true);
  }
  async function commit() {
    if (lock.current || !preview || !confirm || settled.current) return;
    if (preview.expires_at * 1000 <= Date.now()) {
      setMessage("预览已过期，请重新生成");
      setConfirm(false);
      return;
    }
    lock.current = true;
    write.current = true;
    mustRead.current = true;
    setNeedsRead(true);
    pending(true);
    setMessage("");
    try {
      const o: unknown = await api.commit(preview.id);
      if (
        o &&
        typeof o === "object" &&
        "book_id" in o &&
        "original_chapter_id" in o &&
        "new_chapter_id" in o &&
        o.book_id === target.bookId &&
        o.original_chapter_id === target.chapterId &&
        typeof o.new_chapter_id === "string" &&
        o.new_chapter_id.length > 0 &&
        o.new_chapter_id !== target.chapterId
      )
        outcome.current = o as SplitOutcome;
      else outcome.current = undefined;
    } catch (e) {
      commitError.current = errorMessage(e);
      outcome.current = undefined;
    } finally {
      candidate.current = null;
      settled.current = true;
      lock.current = false;
      write.current = false;
      if (mounted.current) {
        setPreview(null);
        setDetail(null);
        setConfirm(false);
        await reload();
      }
    }
  }
  return (
    <Dialog
      title="拆分章节"
      subtitle="按精确光标位置拆成两章，不增加或删除正文字符。"
      closeLabel="关闭拆章面板"
      className="chapter-split"
      size="md"
      dismissible={!busy && !needsRead}
      onClose={close}
      footer={
        confirm ? (
          <>
            <button
              className="ghost"
              onClick={() => setConfirm(false)}
              disabled={busy}
            >
              取消确认
            </button>
            <button className="primary" onClick={commit} disabled={busy}>
              确认拆分
            </button>
          </>
        ) : preview ? (
          <button className="primary" onClick={ask} disabled={busy}>
            拆分为两章
          </button>
        ) : needsRead && !busy ? (
          <button onClick={reload}>重新读取核对</button>
        ) : undefined
      }
    >
      <section className="ui-card">
        <p className="ui-meta">
          新章节不会继承原章写作约束。拆分后原章旧依据包因正文变化失效，请分别配置并重新冻结。
        </p>
        <p className="ui-meta">按精确光标位置拆成两章，不增加或删除正文字符。</p>
        <p className="ui-meta">
          原章关联资料仍留在原章；摘要来源会过期，旧改稿建议不能直接采纳。新章不继承关联，请手动复核。拆前全文保留在原章历史；恢复历史不会自动删除新章。
        </p>
      </section>
      <section className="ui-card">
        <label className="ui-field">
          新章名
          <input
            aria-label="新章名"
            value={title}
            disabled={needsRead || done}
            onChange={(e) => change(e.target.value)}
          />
        </label>
        <button onClick={prepare} disabled={busy || needsRead || done}>
          生成拆章预览
        </button>
      </section>
      {preview && (
        <section className="ui-card split-preview">
          <div className="split-column">
            <h3>
              原章：{preview.original_title} · {preview.before_bytes}字节
            </h3>
            <pre>{preview.before}</pre>
            {preview.before_truncated && <p className="ui-meta">仅显示前2000字符</p>}
            <button
              className="ghost"
              onClick={() => show("before")}
              disabled={busy}
            >
              查看前半全文
            </button>
          </div>
          <div className="split-column">
            <h3>
              新章：{preview.new_title} · {preview.after_bytes}字节
            </h3>
            <pre>{preview.after}</pre>
            {preview.after_truncated && <p className="ui-meta">仅显示前2000字符</p>}
            <button
              className="ghost"
              onClick={() => show("after")}
              disabled={busy}
            >
              查看后半全文
            </button>
          </div>
        </section>
      )}
      {detail !== null && (
        <textarea
          className="split-detail"
          aria-label="拆章全文预览"
          readOnly
          value={detail}
        />
      )}
      {confirm && (
        <div role="group" aria-label="确认拆章" className="ui-confirm">
          <p>
            确认保留原章前半，并在其后新增“{preview?.new_title}”？关联不会迁移。
          </p>
        </div>
      )}
      {message && (
        <p role="status" className="ui-meta">
          {message}
        </p>
      )}
    </Dialog>
  );
}
