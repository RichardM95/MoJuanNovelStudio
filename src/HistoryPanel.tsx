import React, { useEffect, useRef, useState } from "react";
import type { Chapter, LibraryState } from "./types";
import type { HistoryAPI, VersionSummary, VersionDetail } from "./historyTypes";
import { errorMessage } from "./workflowTypes";
type RestoreConfirmation = {
  id: string;
  scope: string;
  revision: number;
  epoch: string;
};
export default function HistoryPanel({
  bookId,
  chapter,
  api,
  perform,
  busy,
  onPendingChange,
}: {
  bookId: string;
  chapter: Chapter;
  api: HistoryAPI;
  perform: (action: () => Promise<LibraryState>) => Promise<boolean>;
  busy: boolean;
  onPendingChange?: (pending: boolean) => void;
}) {
  const scope = `${bookId}:${chapter.id}`;
  const [items, setItems] = useState<VersionSummary[]>([]),
    [cursor, setCursor] = useState<string | null>(null),
    [marked, setMarked] = useState(false),
    [detail, setDetail] = useState<VersionDetail | null>(null),
    [label, setLabel] = useState(""),
    [error, setError] = useState(""),
    [notice, setNotice] = useState(""),
    [listing, setListing] = useState(true),
    [reading, setReading] = useState(false),
    [working, setWorking] = useState(false),
    [confirm, setConfirm] = useState<RestoreConfirmation | null>(null),
    [viewScope, setViewScope] = useState(scope);
  const scopeRef = useRef(scope),
    listGen = useRef(0),
    detailGen = useRef(0),
    actionGen = useRef(0),
    epoch = useRef<string | null>(null),
    acting = useRef(false),
    pendingCallback = useRef(onPendingChange),
    chapterRevision = useRef(chapter.revision);
  scopeRef.current = scope;
  chapterRevision.current = chapter.revision;
  pendingCallback.current = onPendingChange;
  const shown = viewScope === scope ? detail : null;
  const dirty = !!shown && label !== (shown.label ?? "");
  const blocked = dirty || working || !!confirm;
  useEffect(() => {
    pendingCallback.current?.(blocked);
  }, [blocked]);
  useEffect(() => () => pendingCallback.current?.(false), []);
  const canNavigate = () => {
    if (acting.current || busy) return false;
    if (dirty) {
      setError("请先保存名称或取消名称编辑。");
      return false;
    }
    if (confirm) {
      setError("请先完成或取消恢复确认。");
      return false;
    }
    return true;
  };
  async function load(next: string | null, filter = marked) {
    const seq = ++listGen.current;
    if (!next) {
      detailGen.current++;
      setReading(false);
      setDetail(null);
      setLabel("");
      setConfirm(null);
    }
    setListing(true);
    setError("");
    const valid = () => scopeRef.current === scope && seq === listGen.current;
    try {
      const page = await api.list(bookId, chapter.id, next, 50, filter);
      if (!valid()) return;
      if (next && epoch.current !== page.epoch)
        throw new Error("书库已变化，请刷新历史列表。");
      const rows = page.items.filter((v) => v.chapter_id === chapter.id);
      epoch.current = page.epoch;
      setViewScope(scope);
      setItems((old) =>
        next
          ? [...old, ...rows.filter((v) => !old.some((o) => o.id === v.id))]
          : rows,
      );
      setCursor(page.next_cursor);
    } catch (e) {
      if (valid()) setError(errorMessage(e));
    } finally {
      if (valid()) setListing(false);
    }
  }
  useEffect(() => {
    setViewScope(scope);
    setItems([]);
    setCursor(null);
    setDetail(null);
    setLabel("");
    setMarked(false);
    setConfirm(null);
    setWorking(false);
    setReading(false);
    setNotice("");
    acting.current = false;
    epoch.current = null;
    void load(null, false);
    return () => {
      listGen.current++;
      detailGen.current++;
      actionGen.current++;
    };
  }, [scope, api]);
  useEffect(() => {
    setConfirm(null);
  }, [chapter.revision]);
  async function select(version: VersionSummary) {
    if (!canNavigate()) return;
    const seq = ++detailGen.current;
    setReading(true);
    setError("");
    setNotice("");
    setDetail(null);
    setLabel("");
    const valid = () => scopeRef.current === scope && seq === detailGen.current;
    try {
      const value = await api.get(bookId, chapter.id, version.id);
      if (!valid()) return;
      if (
        value.version.chapter_id !== chapter.id ||
        value.version.id !== version.id
      )
        throw new Error("历史版本所属章节不匹配。");
      if (value.epoch !== epoch.current)
        throw new Error("书库已变化，请刷新历史列表。");
      setDetail(value);
      setLabel(value.label ?? "");
      setViewScope(scope);
    } catch (e) {
      if (valid()) setError(errorMessage(e));
    } finally {
      if (valid()) setReading(false);
    }
  }
  async function saveLabel() {
    if (!shown || !dirty || acting.current || busy || confirm) return;
    const text = label.trim();
    if (
      Array.from(text).length > 120 ||
      new TextEncoder().encode(text).length > 480 ||
      /[\u0000-\u001f\u007f-\u009f]/.test(label)
    ) {
      setError("名称最多120个字符、480字节，不能包含控制字符。");
      return;
    }
    const target = shown,
      seq = ++actionGen.current;
    acting.current = true;
    setWorking(true);
    setError("");
    const valid = () => scopeRef.current === scope && seq === actionGen.current;
    try {
      const result = await api.label(
        bookId,
        chapter.id,
        target.version.id,
        text || null,
        target.label_revision,
        target.epoch,
      );
      if (!valid()) return;
      if (result.id !== target.version.id || result.chapter_id !== chapter.id)
        throw new Error("历史版本所属章节不匹配。");
      setDetail({
        ...target,
        label: result.label,
        label_revision: result.label_revision,
      });
      setLabel(result.label ?? "");
      setItems((old) => old.map((v) => (v.id === result.id ? result : v)));
      setNotice("名称已保存");
    } catch (e) {
      if (valid()) setError(errorMessage(e));
    } finally {
      if (valid()) {
        acting.current = false;
        setWorking(false);
      }
    }
  }
  async function restore() {
    if (
      !confirm ||
      dirty ||
      acting.current ||
      busy ||
      confirm.scope !== scope ||
      confirm.revision !== chapterRevision.current
    )
      return;
    const target = confirm,
      seq = ++actionGen.current;
    acting.current = true;
    setWorking(true);
    setError("");
    const valid = () => scopeRef.current === scope && seq === actionGen.current;
    try {
      const ok = await perform(() =>
        api.restore(
          bookId,
          chapter.id,
          target.id,
          target.revision,
          target.epoch,
        ),
      );
      if (!valid()) return;
      if (ok) {
        setConfirm(null);
        setNotice("已恢复为新的章节版本");
        await load(null);
      } else setError("恢复未完成，请检查保存或版本冲突提示。");
    } catch (e) {
      if (valid()) setError(errorMessage(e));
    } finally {
      if (valid()) {
        acting.current = false;
        setWorking(false);
      }
    }
  }
  return (
    <section
      className="history-browser"
      aria-label="历史版本浏览"
      onKeyDown={(e) => {
        if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "s") {
          e.preventDefault();
          e.stopPropagation();
          void saveLabel();
        }
      }}
    >
      <nav>
        <label>
          <input
            type="checkbox"
            checked={marked}
            disabled={busy || working || listing}
            onChange={(e) => {
              if (canNavigate()) {
                setMarked(e.target.checked);
                void load(null, e.target.checked);
              }
            }}
          />
          仅显示已命名版本
        </label>
        <button
          disabled={busy || working || listing}
          onClick={() => {
            if (canNavigate()) void load(null);
          }}
        >
          刷新历史
        </button>
      </nav>
      {error && (
        <p role="alert" className="review-error">
          {error}
        </p>
      )}
      {notice && <p role="status">{notice}</p>}
      <div className="history-browser-grid">
        <aside aria-label="历史版本列表">
          {viewScope === scope &&
            items.map((v) => (
              <button
                key={v.id}
                className="history-version-item"
                disabled={busy || working}
                aria-pressed={shown?.version.id === v.id}
                onClick={() => void select(v)}
              >
                <span>{v.label || `版本 ${v.revision}`}</span>
                <small>
                  版本 {v.revision} ·{" "}
                  {new Date(v.created_at * 1000).toLocaleString()}
                </small>
              </button>
            ))}
          {listing && <p>读取版本列表…</p>}
          {!listing && items.length === 0 && (
            <p>当前章节还没有符合条件的历史版本。</p>
          )}
          {cursor && (
            <button
              disabled={busy || working || listing}
              onClick={() => {
                if (canNavigate()) void load(cursor);
              }}
            >
              加载下一页
            </button>
          )}
        </aside>
        <div className="history-version-detail">
          {reading ? (
            <p>读取选中版本…</p>
          ) : shown ? (
            <>
              <label>
                版本名称
                <input
                  value={label}
                  disabled={busy || working || !!confirm}
                  placeholder="例如：第一稿定稿"
                  onChange={(e) => {
                    setLabel(e.target.value);
                    setNotice("");
                    setError("");
                  }}
                />
              </label>
              <div className="history-actions">
                <button
                  className="primary"
                  disabled={!dirty || busy || working || !!confirm}
                  onClick={() => void saveLabel()}
                >
                  保存名称
                </button>
                <button
                  disabled={!dirty || busy || working || !!confirm}
                  onClick={() => {
                    setLabel(shown.label ?? "");
                    setError("");
                  }}
                >
                  取消名称编辑
                </button>
              </div>
              {dirty && <p>名称尚未保存。</p>}
              <p>
                历史版本 {shown.version.revision} · 来源 {shown.version.source}
                。恢复会生成新版本，不抹去历史。
              </p>
              <div className="diff-columns">
                <article>
                  <h3>当前稿件 · 版本 {chapter.revision}</h3>
                  <pre>{chapter.body || "（空白）"}</pre>
                </article>
                <article>
                  <h3>历史稿件</h3>
                  <pre>{shown.version.body || "（空白）"}</pre>
                </article>
              </div>
              <button
                disabled={dirty || busy || working || !!confirm}
                onClick={() =>
                  setConfirm({
                    id: shown.version.id,
                    scope,
                    revision: chapter.revision,
                    epoch: shown.epoch,
                  })
                }
              >
                恢复此版本
              </button>
              {confirm &&
                confirm.scope === scope &&
                confirm.revision === chapter.revision && (
                  <div
                    role="alertdialog"
                    aria-label="确认恢复历史版本"
                    className="review-confirm"
                  >
                    <p>
                      确认将所选历史内容恢复为当前章节的新版本？版本或书库变更时拒绝覆盖。
                    </p>
                    <button
                      disabled={busy || working}
                      onClick={() => setConfirm(null)}
                    >
                      取消恢复
                    </button>
                    <button
                      disabled={busy || working}
                      onClick={() => void restore()}
                    >
                      确认恢复
                    </button>
                  </div>
                )}
            </>
          ) : (
            <p>点击一个历史版本，按需读取正文。</p>
          )}
        </div>
      </div>
    </section>
  );
}
