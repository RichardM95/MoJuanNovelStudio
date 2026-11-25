import React, { useEffect, useRef, useState } from "react";
import type {
  ImportAPI,
  ImportMode,
  ImportPreview,
  ImportDetail,
  ImportOutcome,
} from "./importTypes";
import { errorMessage } from "./workflowTypes";
export default function ImportPanel({
  api,
  onClose,
  onPendingChange,
  onImported,
}: {
  api: ImportAPI;
  onClose: () => void;
  onPendingChange: (pending: boolean) => void;
  onImported: (outcome?: ImportOutcome) => Promise<void>;
}) {
  const [title, setTitle] = useState(""),
    [author, setAuthor] = useState(""),
    [mode, setMode] = useState<ImportMode>("headings"),
    [source, setSource] = useState<ImportPreview | null>(null),
    [preview, setPreview] = useState<ImportPreview | null>(null),
    [detail, setDetail] = useState<ImportDetail | null>(null),
    [selected, setSelected] = useState<number | null>(null),
    [confirm, setConfirm] = useState(false),
    [loading, setLoading] = useState(false),
    [choosing, setChoosing] = useState(false),
    [detailLoading, setDetailLoading] = useState(false),
    [committing, setCommitting] = useState(false),
    [reloading, setReloading] = useState(false),
    [needsRead, setNeedsRead] = useState(false),
    [uncertain, setUncertain] = useState(false),
    [outcome, setOutcome] = useState<ImportOutcome | undefined>(),
    [error, setError] = useState(""),
    [reloadError, setReloadError] = useState(""),
    [now, setNow] = useState(Date.now());
  const sourceRef = useRef<ImportPreview | null>(null),
    previewRef = useRef<ImportPreview | null>(null),
    outcomeRef = useRef<ImportOutcome | undefined>(undefined),
    opSeq = useRef(0),
    draftSeq = useRef(0),
    detailSeq = useRef(0),
    mounted = useRef(true),
    operation = useRef(false),
    chooseLock = useRef(false),
    writeLock = useRef(false),
    readLock = useRef(false),
    root = useRef<HTMLElement>(null),
    callbacks = useRef({ onClose, onPendingChange, onImported });
  callbacks.current = { onClose, onPendingChange, onImported };
  const blocked = committing || reloading || needsRead,
    blockedRef = useRef(false);
  blockedRef.current = blocked || writeLock.current;
  const expired = !!preview && now >= preview.expires_at * 1000;
  function discard(id: string) {
    void api.discard(id).catch(() => {});
  }
  function invalidate() {
    draftSeq.current++;
    detailSeq.current++;
    previewRef.current = null;
    setPreview(null);
    setDetail(null);
    setSelected(null);
    setDetailLoading(false);
    setConfirm(false);
    setError("");
    setOutcome(undefined);
    outcomeRef.current = undefined;
    setUncertain(false);
  }
  function forget() {
    const old = sourceRef.current;
    sourceRef.current = null;
    previewRef.current = null;
    setSource(null);
    setPreview(null);
    if (old) discard(old.id);
  }
  function edit(change: () => void) {
    if (blockedRef.current || chooseLock.current) return;
    invalidate();
    change();
  }
  function close() {
    if (blockedRef.current) return;
    opSeq.current++;
    detailSeq.current++;
    forget();
    callbacks.current.onClose();
  }
  useEffect(() => {
    callbacks.current.onPendingChange(blocked);
  }, [blocked]);
  useEffect(() => {
    mounted.current = true;
    const panel = root.current!,
      previous = document.activeElement as HTMLElement | null;
    const controls = () =>
      Array.from(
        panel.querySelectorAll<HTMLElement>(
          "button:not(:disabled),input:not(:disabled),select:not(:disabled),textarea:not(:disabled)",
        ),
      );
    const focus = () => controls()[0]?.focus();
    const guard = (e: FocusEvent) => {
      if (!panel.contains(e.target as Node)) focus();
    };
    const key = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        close();
      }
      if (e.key === "Tab") {
        const all = controls(),
          first = all[0],
          last = all.at(-1);
        if (e.shiftKey && document.activeElement === first) {
          e.preventDefault();
          last?.focus();
        } else if (!e.shiftKey && document.activeElement === last) {
          e.preventDefault();
          first?.focus();
        }
      }
    };
    focus();
    document.addEventListener("focusin", guard);
    panel.addEventListener("keydown", key);
    const tick = setInterval(() => setNow(Date.now()), 1000);
    return () => {
      mounted.current = false;
      opSeq.current++;
      detailSeq.current++;
      clearInterval(tick);
      document.removeEventListener("focusin", guard);
      panel.removeEventListener("keydown", key);
      const old = sourceRef.current;
      sourceRef.current = null;
      if (old) discard(old.id);
      callbacks.current.onPendingChange(false);
      if (previous?.isConnected) previous.focus();
    };
  }, [api]);
  async function choose() {
    if (operation.current || blockedRef.current) return;
    operation.current = true;
    chooseLock.current = true;
    invalidate();
    forget();
    const n = ++opSeq.current;
    setLoading(true);
    setChoosing(true);
    try {
      const p = await api.choose(mode);
      if (!mounted.current || n !== opSeq.current) {
        if (p) discard(p.id);
        return;
      }
      if (p) {
        sourceRef.current = p;
        previewRef.current = p;
        setSource(p);
        setPreview(p);
        setTitle(p.title);
        setAuthor(p.author);
        setMode(p.mode);
        setNow(Date.now());
      }
    } catch (e) {
      if (mounted.current && n === opSeq.current) setError(errorMessage(e));
    } finally {
      operation.current = false;
      chooseLock.current = false;
      if (mounted.current && n === opSeq.current) {
        setLoading(false);
        setChoosing(false);
      }
    }
  }
  async function revise() {
    const old = sourceRef.current;
    if (!old || operation.current || blockedRef.current) return;
    invalidate();
    if (
      !title.trim() ||
      new TextEncoder().encode(title).length > 4096 ||
      new TextEncoder().encode(author).length > 4096
    ) {
      setError("书名不能为空，书名和作者各最多4096字节。");
      return;
    }
    operation.current = true;
    const n = ++opSeq.current,
      g = draftSeq.current;
    setLoading(true);
    try {
      const p = await api.revise(old.id, title, author, mode);
      if (!mounted.current || n !== opSeq.current) {
        discard(p.id);
        return;
      }
      sourceRef.current = p;
      setSource(p);
      if (g === draftSeq.current) {
        if (p.title !== title || p.author !== author || p.mode !== mode)
          throw Error("预览配置与输入不符，请更新预览。");
        previewRef.current = p;
        setPreview(p);
        setNow(Date.now());
      }
    } catch (e) {
      if (mounted.current && n === opSeq.current) setError(errorMessage(e));
    } finally {
      operation.current = false;
      if (mounted.current && n === opSeq.current) setLoading(false);
    }
  }
  async function show(index: number) {
    const p = previewRef.current;
    if (!p || blockedRef.current || Date.now() >= p.expires_at * 1000) return;
    const n = ++detailSeq.current;
    setSelected(index);
    setDetail(null);
    setDetailLoading(true);
    setError("");
    try {
      const d = await api.detail(p.id, index);
      if (
        mounted.current &&
        n === detailSeq.current &&
        previewRef.current?.id === p.id &&
        d.index === index
      )
        setDetail(d);
    } catch (e) {
      if (mounted.current && n === detailSeq.current) setError(errorMessage(e));
    } finally {
      if (mounted.current && n === detailSeq.current) setDetailLoading(false);
    }
  }
  async function reload() {
    if (readLock.current) return;
    readLock.current = true;
    setReloading(true);
    setReloadError("");
    try {
      await callbacks.current.onImported(outcomeRef.current);
      if (mounted.current) setNeedsRead(false);
    } catch (e) {
      if (mounted.current) {
        setNeedsRead(true);
        setReloadError(errorMessage(e));
      }
    } finally {
      readLock.current = false;
      if (mounted.current) setReloading(false);
    }
  }
  async function commit() {
    const p = previewRef.current;
    if (
      writeLock.current ||
      !p ||
      !confirm ||
      !p.total_chapters ||
      Date.now() >= p.expires_at * 1000
    )
      return;
    writeLock.current = true;
    blockedRef.current = true;
    callbacks.current.onPendingChange(true);
    setCommitting(true);
    setConfirm(false);
    setError("");
    sourceRef.current = null;
    previewRef.current = null;
    setSource(null);
    setPreview(null);
    setDetail(null);
    detailSeq.current++;
    try {
      const result = await api.commit(p.id);
      if (!mounted.current) return;
      outcomeRef.current = result;
      setOutcome(result);
      setNeedsRead(true);
      await reload();
    } catch (e) {
      if (mounted.current) {
        setUncertain(true);
        setNeedsRead(true);
        setError(errorMessage(e));
      }
    } finally {
      writeLock.current = false;
      if (mounted.current) setCommitting(false);
    }
  }
  return (
    <div className="modal-backdrop">
      <section
        ref={root}
        className="modal txt-import"
        role="dialog"
        aria-modal="true"
        aria-label="导入TXT新作品"
      >
        <header>
          <h2>导入TXT新作品</h2>
          <button
            className="ghost"
            aria-label="关闭导入面板"
            disabled={blocked}
            onClick={close}
          >
            关闭
          </button>
        </header>
        <p>始终创建新作品，不覆盖同名作品，原文件不修改。</p>
        <div className="import-controls">
          <label>
            分章模式
            <select
              aria-label="分章模式"
              value={mode}
              disabled={blocked || choosing}
              onChange={(e) =>
                edit(() => setMode(e.target.value as ImportMode))
              }
            >
              <option value="headings">按标题分章</option>
              <option value="single">全文单章</option>
            </select>
          </label>
          <button disabled={blocked || loading} onClick={() => void choose()}>
            {choosing ? "正在读取…" : "选择TXT文件"}
          </button>
        </div>
        <p className="import-hint">
          仅UTF-8（可带BOM），文件最多16 MiB、单章最多10 MiB；每次导入都会新建作品，不会自动合并。按标题分章识别独立的“第…卷/部/篇”“第…章/回/节/集/话”行（可带“正文”前缀，标题文字可用空白或冒号分隔），以及序章、楔子、前言、番外等标题行；标题行转为名称，其余正文和换行原样保留。若识别不合适可改为全文单章；超过10MiB仍需合理分章。
        </p>
        <div className="import-controls">
          <label>
            书名
            <input
              aria-label="书名"
              value={title}
              disabled={blocked || !source || choosing}
              onChange={(e) => edit(() => setTitle(e.target.value))}
            />
          </label>
          <label>
            作者
            <input
              aria-label="作者"
              value={author}
              disabled={blocked || !source || choosing}
              onChange={(e) => edit(() => setAuthor(e.target.value))}
            />
          </label>
          <button
            disabled={blocked || loading || !source}
            onClick={() => void revise()}
          >
            {loading && !choosing ? "正在预览…" : "更新预览"}
          </button>
        </div>
        {source && <p>{source.source_name}</p>}
        {source && !preview && !loading && (
          <p>配置已改变或未通过校验，请更新预览后再导入。</p>
        )}
        {error && <p role="alert">{error}</p>}
        {preview && (
          <>
            <p>
              《{preview.title}》 · {preview.author || "未填写作者"} ·{" "}
              {preview.mode === "single" ? "全文单章" : "按标题分章"} ·{" "}
              {preview.volumes.length}卷、{preview.total_chapters}章 · 正文
              {preview.total_bytes}字节
            </p>
            {expired ? (
              <p role="alert">预览已过期，请重新选择文件。</p>
            ) : (
              <p>
                有效至{new Date(preview.expires_at * 1000).toLocaleTimeString()}
                。请核查目录和正文。
              </p>
            )}
            <div className="import-chapters">
              {preview.volumes.map((v, vi) => (
                <section key={vi}>
                  <h3>{v.title}</h3>
                  <ul>
                    {v.chapters.map((c) => (
                      <li key={c.index}>
                        <button
                          aria-label={`查看 ${c.title}`}
                          aria-pressed={selected === c.index}
                          disabled={expired || blocked}
                          onClick={() => void show(c.index)}
                        >
                          {c.title} · {c.bytes}字节
                        </button>
                        <p>{c.snippet}</p>
                      </li>
                    ))}
                  </ul>
                </section>
              ))}
            </div>
            {detailLoading && <p>正在读取所选章节…</p>}
            {detail && (
              <section className="import-detail">
                <h3>章节正文预览</h3>
                <pre>{detail.body}</pre>
              </section>
            )}
            <button
              className="primary"
              disabled={
                blocked || expired || !preview.total_chapters || confirm
              }
              onClick={() => setConfirm(true)}
            >
              导入为新作品
            </button>
            {confirm && (
              <section aria-label="确认TXT导入">
                <p>
                  确认创建《{preview.title}》（{preview.author || "未填写作者"}
                  ），{preview.volumes.length}卷、{preview.total_chapters}
                  章？不会覆盖已有作品。
                </p>
                <button className="ghost" onClick={() => setConfirm(false)}>
                  返回预览
                </button>
                <button
                  className="primary"
                  disabled={blocked || expired}
                  onClick={() => void commit()}
                >
                  确认导入
                </button>
              </section>
            )}
          </>
        )}
        {committing && <p role="status">正在导入，请勿关闭…</p>}
        {outcome && (
          <p role="status">
            已导入新作品：{outcome.volumes}卷、{outcome.chapters}章。
          </p>
        )}
        {uncertain && needsRead && (
          <p role="alert">导入结果未确认，请先刷新书架核对，不要重复导入。</p>
        )}
        {reloading && <p>正在重新读取书架…</p>}
        {reloadError && <p role="alert">{reloadError}</p>}
        {needsRead && !reloading && (
          <button onClick={() => void reload()}>
            {uncertain ? "刷新书架核对" : "重新加载书架"}
          </button>
        )}
        {uncertain && !needsRead && (
          <p>已重新读取书架，请核对是否已有新作品，再决定是否重新选择文件。</p>
        )}
      </section>
    </div>
  );
}
