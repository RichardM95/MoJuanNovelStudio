import React, { useEffect, useRef, useState } from "react";
import type { BookSearch, SearchHit, SearchResults } from "./searchTypes";
import { errorMessage } from "./workflowTypes";
import { Dialog } from "./ui/Dialog";
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
    closeRef = useRef(onClose);
  closeRef.current = onClose;
  const close = () => {
    sequence.current++;
    closeRef.current();
  };
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
    <Dialog
      title="作品内搜索"
      subtitle="搜索当前作品的活动章节与创作资料。"
      closeLabel="关闭搜索"
      className="book-search"
      size="lg"
      onClose={close}
    >
      <section className="ui-card">
        <form className="search-form" onSubmit={(e) => void submit(e)}>
          <input
            autoComplete="off"
            aria-label="作品内关键词"
            placeholder="输入关键词"
            value={query}
            onChange={(e) => {
              sequence.current++;
              setQuery(e.target.value);
              setResult(null);
              setBusy(false);
              setError("");
            }}
          />
          <button className="primary" type="submit" disabled={busy}>
            搜索
          </button>
        </form>
      </section>
      {error && <p role="alert">{error}</p>}
      {busy && (
        <p role="status" className="ui-meta">
          搜索中…
        </p>
      )}
      {result && (
        <section className="ui-card">
          <p role="status" className="ui-meta">
            {result.items.length
              ? `${result.items.length} 条结果`
              : "未找到匹配内容"}
            {result.truncated ? "，仅显示前30条，请缩小关键词范围。" : ""}
          </p>
          <div className="search-results">
            {result.items.map((hit) => (
              <button key={`${hit.kind}:${hit.id}`} onClick={() => onOpen(hit)}>
                <strong>
                  {hit.kind === "chapter" ? "章节" : "资料"}：{hit.title}
                </strong>
                <span>{hit.snippet}</span>
              </button>
            ))}
          </div>
        </section>
      )}
    </Dialog>
  );
}
