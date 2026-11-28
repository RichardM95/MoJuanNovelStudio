import React, { useState } from "react";
import { RotateCcw, Trash2 } from "lucide-react";
import type { LibraryState, Command } from "./types";
import { Dialog } from "./ui/Dialog";
type Item = {
  id: string;
  title: string;
  kind: "book" | "volume" | "chapter";
  label: string;
  context: string;
  blocked: boolean;
};
export default function RecycleBin({
  data,
  busy,
  apply,
}: {
  data: LibraryState;
  busy: boolean;
  apply: (command: Command) => Promise<boolean | void>;
}) {
  const [pending, setPending] = useState<Item | null>(null),
    [typed, setTyped] = useState("");
  const items: Item[] = [
    ...data.books
      .filter((b) => b.deleted_at != null)
      .map((b) => ({
        id: b.id,
        title: b.title,
        kind: "book" as const,
        label: "作品",
        context: `${b.author || "未署名"} · 包括全部卷、章节和正文`,
        blocked: false,
      })),
    ...data.volumes
      .filter((v) => v.deleted_at != null)
      .map((v) => {
        const b = data.books.find((b) => b.id === v.book_id);
        return {
          id: v.id,
          title: v.title,
          kind: "volume" as const,
          label: "卷",
          context: b?.title ?? "",
          blocked: !b || b.deleted_at != null,
        };
      }),
    ...data.chapters
      .filter((c) => c.deleted_at != null)
      .map((c) => {
        const v = data.volumes.find((v) => v.id === c.volume_id),
          b = data.books.find((b) => b.id === v?.book_id);
        return {
          id: c.id,
          title: c.title,
          kind: "chapter" as const,
          label: "章节",
          context: `${b?.title ?? ""} / ${v?.title ?? ""}`,
          blocked: !v || !b || v.deleted_at != null || b.deleted_at != null,
        };
      }),
  ];
  return (
    <section className="shelf">
      <header className="shelf-header">
        <div>
          <h1>回收站</h1>
          <p>作品、卷与章节。先恢复所属作品或卷，再恢复其中单独删除的内容。</p>
        </div>
      </header>
      {items.length === 0 ? (
        <div className="empty">
          <Trash2 size={40} />
          <h2>回收站是空的</h2>
        </div>
      ) : (
        <div className="recycle-list">
          {items.map((item) => (
            <article className="recycle-item" key={`${item.kind}:${item.id}`}>
              <div>
                <small>{item.label}</small>
                <h2>{item.title}</h2>
                <p>{item.context}</p>
                {item.blocked && <small>请先恢复所属作品或卷</small>}
              </div>
              <div className="recycle-actions">
                <button
                  aria-label={`恢复${item.label} ${item.title}`}
                  disabled={busy || item.blocked}
                  onClick={() =>
                    void apply({ type: `restore_${item.kind}`, id: item.id })
                  }
                >
                  <RotateCcw size={15} />
                  恢复
                </button>
                <button
                  className="danger-text"
                  aria-label={`永久删除${item.label} ${item.title}`}
                  disabled={busy}
                  onClick={() => {
                    setPending(item);
                    setTyped("");
                  }}
                >
                  <Trash2 size={15} />
                  永久删除
                </button>
              </div>
            </article>
          ))}
        </div>
      )}
      {pending && (
        <Dialog
          title={`永久删除${pending.label}`}
          closeLabel="关闭永久删除"
          showClose={false}
          size="sm"
          dismissible={!busy}
          onClose={() => setPending(null)}
          footer={
            <>
              <button type="button" disabled={busy} onClick={() => setPending(null)}>
                取消
              </button>
              <button
                type="submit"
                form="recycle-purge-form"
                className="danger"
                disabled={busy || typed !== pending.title}
              >
                确认永久删除
              </button>
            </>
          }
        >
          <form
            id="recycle-purge-form"
            onSubmit={(e) => {
              e.preventDefault();
              if (typed !== pending.title) return;
              void apply({
                type: `purge_${pending.kind}`,
                id: pending.id,
              }).then((success) => {
                if (success === false) return;
                setPending(null);
                setTyped("");
              });
            }}
          >
            <p className="ui-meta">{pending.context}</p>
            <p className="ui-meta">
              《{pending.title}》
              {pending.kind !== "chapter"
                ? "及其全部下级内容（包括回收站中的内容）"
                : ""}
              将从当前书库永久移除，无法在回收站恢复。独立备份不会删除。
            </p>
            <label className="ui-field">
              输入名称确认
              <input
                aria-label="输入名称确认"
                value={typed}
                onChange={(e) => setTyped(e.target.value)}
              />
            </label>
          </form>
        </Dialog>
      )}
    </section>
  );
}
