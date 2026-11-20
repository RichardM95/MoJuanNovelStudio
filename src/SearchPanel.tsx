import React, { useEffect, useRef, useState } from "react";
import type { BookSearch, SearchHit, SearchResults } from "./searchTypes";
import { errorMessage } from "./workflowTypes";
export default function SearchPanel({
  bookId,
  search,
  onOpen,
  onClose,
}: {
  bookId: string;
  search: BookSearch;
  onOpen: (hit: SearchHit) => void;
  onClose: () => void;
}) {
  const [query, setQuery] = useState(""),
    [result, setResult] = useState<SearchResults | null>(null),
    [error, setError] = useState(""),
    [busy, setBusy] = useState(false);
  const sequence = useRef(0),
    root = useRef<HTMLElement>(null),
    closeRef = useRef(onClose);
  closeRef.current = onClose;
  useEffect(() => {
    const panel = root.current!,
      previous = document.activeElement as HTMLElement | null;
    const elements = () =>
      Array.from(
        panel.querySelectorAll<HTMLElement>(
          "button:not(:disabled),input:not(:disabled)",
        ),
      );
    const focus = () => elements()[0]?.focus();
    const guard = (e: FocusEvent) => {
      if (!panel.contains(e.target as Node)) focus();
    };
    const key = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        sequence.current++;
        closeRef.current();
      }
      if (e.key === "Tab") {
        const all = elements(),
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
    return () => {
      sequence.current++;
      document.removeEventListener("focusin", guard);
      panel.removeEventListener("keydown", key);
      if (previous?.isConnected) previous.focus();
    };
  }, []);
  useEffect(() => {
    sequence.current++;
    setResult(null);
    setBusy(false);
    setError("");
  }, [bookId]);
  async function submit(e: React.FormEvent) {
    e.preventDefault();
    if (!query.trim()) {
      setError("请输入关键词");
      return;
    }
    if (new TextEncoder().encode(query).length > 256) {
      setError("关键词不能超过256字节");
      return;
    }
    const request = ++sequence.current;
    setBusy(true);
    setError("");
    setResult(null);
    try {
      const found = await search(bookId, query, 30);
      if (request === sequence.current) setResult(found);
    } catch (e) {
      if (request === sequence.current) setError(errorMessage(e));
    } finally {
      if (request === sequence.current) setBusy(false);
    }
  }
  return (
    <div className="modal-backdrop">
      <section
        className="modal book-search"
        role="dialog"
        aria-modal="true"
        aria-label="作品内搜索"
        ref={root}
      >
        <header>
          <h2>作品内搜索</h2>
          <button
            onClick={() => {
              sequence.current++;
              onClose();
            }}
            aria-label="关闭搜索"
          >
            ×
          </button>
        </header>
        <form onSubmit={(e) => void submit(e)}>
          <input
            autoComplete="off"
            aria-label="作品内关键词"
            value={query}
            onChange={(e) => {
              sequence.current++;
              setQuery(e.target.value);
              setResult(null);
              setBusy(false);
              setError("");
            }}
          />
          <button type="submit" disabled={busy}>
            搜索
          </button>
        </form>
        {error && <p role="alert">{error}</p>}
        {busy && <p role="status">搜索中…</p>}
        {result && (
          <>
            <p role="status">
              {result.items.length
                ? `${result.items.length} 条结果`
                : "未找到匹配内容"}
              {result.truncated ? "，仅显示前30条，请缩小关键词范围。" : ""}
            </p>
            <div className="search-results">
              {result.items.map((hit) => (
                <button
                  key={`${hit.kind}:${hit.id}`}
                  onClick={() => onOpen(hit)}
                >
                  <strong>
                    {hit.kind === "chapter" ? "章节" : "资料"}：{hit.title}
                  </strong>
                  <span>{hit.snippet}</span>
                </button>
              ))}
            </div>
          </>
        )}
      </section>
    </div>
  );
}
