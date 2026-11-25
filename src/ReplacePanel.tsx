import React, { useEffect, useRef, useState } from "react";
import type {
  ReplaceAPI,
  ReplacePreview,
  ReplaceDetail,
  ReplaceOutcome,
} from "./replaceTypes";
import { errorMessage } from "./workflowTypes";
export default function ReplacePanel({
  bookId,
  api,
  onClose,
  onPendingChange,
  onCommitted,
}: {
  bookId: string;
  api: ReplaceAPI;
  onClose: () => void;
  onPendingChange: (pending: boolean) => void;
  onCommitted: () => Promise<void>;
}) {
  const [query, setQuery] = useState(""),
    [replacement, setReplacement] = useState(""),
    [candidate, setCandidate] = useState<ReplacePreview | null>(null),
    [detail, setDetail] = useState<ReplaceDetail | null>(null),
    [selected, setSelected] = useState(""),
    [confirm, setConfirm] = useState(false),
    [loading, setLoading] = useState(false),
    [detailLoading, setDetailLoading] = useState(false),
    [committing, setCommitting] = useState(false),
    [reloading, setReloading] = useState(false),
    [needsRead, setNeedsRead] = useState(false),
    [outcome, setOutcome] = useState<ReplaceOutcome | null>(null),
    [uncertain, setUncertain] = useState(false),
    [error, setError] = useState(""),
    [reloadError, setReloadError] = useState(""),
    [now, setNow] = useState(Date.now());
  const seq = useRef(0),
    detailSeq = useRef(0),
    candidateRef = useRef<ReplacePreview | null>(null),
    mounted = useRef(true),
    writeLock = useRef(false),
    readLock = useRef(false),
    root = useRef<HTMLElement>(null),
    callbacks = useRef({ onClose, onPendingChange, onCommitted });
  callbacks.current = { onClose, onPendingChange, onCommitted };
  const blocked = committing || reloading || needsRead;
  const blockedRef = useRef(blocked);
  blockedRef.current = blocked || writeLock.current;
  const expired = !!candidate && now >= candidate.expires_at * 1000;
  const forget = () => {
    const old = candidateRef.current;
    candidateRef.current = null;
    setCandidate(null);
    if (old) void api.discard(old.id).catch(() => {});
  };
  const invalidate = () => {
    seq.current++;
    detailSeq.current++;
    forget();
    setDetail(null);
    setSelected("");
    setConfirm(false);
    setLoading(false);
    setDetailLoading(false);
    setError("");
    setOutcome(null);
    setUncertain(false);
  };
  function close() {
    if (blockedRef.current) return;
    seq.current++;
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
    const focusable = () =>
      Array.from(
        panel.querySelectorAll<HTMLElement>(
          "button:not(:disabled),input:not(:disabled),textarea:not(:disabled)",
        ),
      );
    const focus = () => focusable()[0]?.focus();
    const guard = (e: FocusEvent) => {
      if (!panel.contains(e.target as Node)) focus();
    };
    const key = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        close();
      }
      if (e.key === "Tab") {
        const list = focusable(),
          first = list[0],
          last = list.at(-1);
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
      seq.current++;
      detailSeq.current++;
      clearInterval(tick);
      document.removeEventListener("focusin", guard);
      panel.removeEventListener("keydown", key);
      const old = candidateRef.current;
      candidateRef.current = null;
      if (old) void api.discard(old.id).catch(() => {});
      callbacks.current.onPendingChange(false);
      if (previous?.isConnected) previous.focus();
    };
  }, [api]);
  useEffect(() => {
    invalidate();
  }, [bookId]);
  async function preview(e: React.FormEvent) {
    e.preventDefault();
    if (writeLock.current || blocked) return;
    invalidate();
    const n = ++seq.current;
    if (
      !query ||
      new TextEncoder().encode(query).length > 256 ||
      new TextEncoder().encode(replacement).length > 1024 ||
      query === replacement
    ) {
      setError("搜索内容须1至256字节，替换内容最多1024字节且须不同。");
      return;
    }
    setLoading(true);
    try {
      const p = await api.preview(bookId, query, replacement);
      if (!mounted.current || seq.current !== n) {
        void api.discard(p.id).catch(() => {});
        return;
      }
      if (
        p.book_id !== bookId ||
        p.query !== query ||
        p.replacement !== replacement
      ) {
        void api.discard(p.id).catch(() => {});
        throw Error("预览与当前输入不符，请重新预览。");
      }
      candidateRef.current = p;
      setCandidate(p);
      setNow(Date.now());
    } catch (e) {
      if (mounted.current && seq.current === n) setError(errorMessage(e));
    } finally {
      if (mounted.current && seq.current === n) setLoading(false);
    }
  }
  async function show(chapterId: string) {
    if (!candidate || blocked) return;
    const n = ++detailSeq.current,
      id = candidate.id;
    setSelected(chapterId);
    setDetail(null);
    setDetailLoading(true);
    setError("");
    try {
      const d = await api.detail(id, chapterId);
      if (
        mounted.current &&
        detailSeq.current === n &&
        candidateRef.current?.id === id &&
        d.chapter_id === chapterId
      )
        setDetail(d);
    } catch (e) {
      if (mounted.current && detailSeq.current === n) setError(errorMessage(e));
    } finally {
      if (mounted.current && detailSeq.current === n) setDetailLoading(false);
    }
  }
  async function reload() {
    if (readLock.current) return;
    readLock.current = true;
    setReloading(true);
    setReloadError("");
    try {
      await callbacks.current.onCommitted();
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
    const p = candidateRef.current;
    if (
      writeLock.current ||
      !p ||
      !confirm ||
      Date.now() >= p.expires_at * 1000 ||
      !p.total_matches
    )
      return;
    writeLock.current = true;
    blockedRef.current = true;
    callbacks.current.onPendingChange(true);
    setCommitting(true);
    setError("");
    setConfirm(false);
    candidateRef.current = null;
    setCandidate(null);
    setDetail(null);
    detailSeq.current++;
    try {
      const result = await api.commit(p.id);
      if (!mounted.current) return;
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
        className="modal book-replace"
        role="dialog"
        aria-modal="true"
        aria-label="作品正文替换"
      >
        <header>
          <h2>作品正文替换</h2>
          <button disabled={blocked} onClick={close} aria-label="关闭替换面板">
            关闭
          </button>
        </header>
        <p>
          仅替换当前作品的活动章节正文，不改标题或资料。精确字面匹配，原稿保留在章节历史。
        </p>
        <form onSubmit={(e) => void preview(e)}>
          <label>
            查找内容
            <input
              aria-label="查找内容"
              value={query}
              disabled={blocked}
              onChange={(e) => {
                invalidate();
                setQuery(e.target.value);
              }}
            />
          </label>
          <label>
            替换为
            <input
              aria-label="替换为"
              value={replacement}
              disabled={blocked}
              onChange={(e) => {
                invalidate();
                setReplacement(e.target.value);
              }}
            />
          </label>
          <button disabled={blocked || loading} type="submit">
            {loading ? "正在预览…" : "预览替换"}
          </button>
        </form>
        <p className="replace-hint">
          替换内容可留空。一次最多500章、10万处；超限不执行部分替换。
        </p>
        {error && <p role="alert">{error}</p>}
        {uncertain && needsRead && <p role="alert">替换结果未确认，请先刷新正文核对，再决定是否重新预览。</p>}
        {candidate && (
          <>
            <p>
              将把“{candidate.query}”替换为
              {candidate.replacement ? `“${candidate.replacement}”` : "空文本"}
              ，共{candidate.chapters.length}章、{candidate.total_matches}处。
            </p>
            {expired ? (
              <p role="alert">预览已过期，请重新预览。</p>
            ) : (
              <p>
                有效至
                {new Date(candidate.expires_at * 1000).toLocaleTimeString()}
                ；任何章节变化都需重新预览。
              </p>
            )}
            {!candidate.total_matches ? (
              <p>没有匹配的正文。</p>
            ) : (
              <>
                <ul className="replace-chapters">
                  {candidate.chapters.map((c) => (
                    <li key={c.id}>
                      <button
                        aria-label={`查看 ${c.title}`}
                        aria-pressed={selected === c.id}
                        onClick={() => void show(c.id)}
                      >
                        {c.title} · {c.matches}处
                      </button>
                      <div className="replace-snippets">
                        <p>原：{c.before_snippet}</p>
                        <p>后：{c.after_snippet}</p>
                      </div>
                    </li>
                  ))}
                </ul>
                {detailLoading && <p>正在读取所选章节…</p>}
                {detail && (
                  <div className="diff-columns">
                    <article>
                      <h3>替换前</h3>
                      <pre>{detail.before}</pre>
                    </article>
                    <article>
                      <h3>替换后</h3>
                      <pre>{detail.after}</pre>
                    </article>
                  </div>
                )}
                <button
                  className="primary"
                  disabled={expired || blocked || confirm}
                  onClick={() => setConfirm(true)}
                >
                  替换这些正文
                </button>
                {confirm && (
                  <section aria-label="确认正文替换">
                    <p>
                      确认替换上述{candidate.chapters.length}章、
                      {candidate.total_matches}
                      处？整次修改同时提交，可从章节历史逐章恢复。
                    </p>
                    <button className="ghost" onClick={() => setConfirm(false)}>
                      返回预览
                    </button>
                    <button
                      className="primary"
                      disabled={blocked || expired}
                      onClick={() => void commit()}
                    >
                      确认替换
                    </button>
                  </section>
                )}
              </>
            )}
          </>
        )}
        {committing && <p role="status">正在提交，请勿关闭…</p>}
        {outcome && (
          <p role="status">
            已替换{outcome.chapters}章，共{outcome.matches}处。
          </p>
        )}
        {reloading && <p>正在重新读取正文…</p>}
        {reloadError && <p role="alert">{reloadError}</p>}
        {needsRead && !reloading && (
          <button onClick={() => void reload()}>
            {uncertain ? "刷新正文核对" : "重新加载正文"}
          </button>
        )}
        {uncertain && !needsRead && (
          <p>已重新读取正文，请先核对内容；再次修改需重新预览。</p>
        )}
      </section>
    </div>
  );
}
