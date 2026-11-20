// Fictional in-memory UI only. No SQLite, files, model or native IPC.
import type { SplitAPI, SplitPreview } from "../src/splitTypes";
import type { LibraryState } from "../src/types";
export function splitFixture(
  get: () => LibraryState,
  set: (s: LibraryState) => void,
): SplitAPI {
  let current: {
    p: SplitPreview;
    book: string;
    chapter: string;
    revision: number;
    left: string;
    right: string;
  } | null = null;
  return {
    prepare: async (book, chapter, revision, offset, title) => {
      current = null;
      const c = get().chapters.find((c) => c.id === chapter);
      if (
        !c ||
        c.revision !== revision ||
        offset <= 0 ||
        offset >= c.body.length
      )
        throw Error("预览失效");
      const left = c.body.slice(0, offset),
        right = c.body.slice(offset),
        enc = new TextEncoder();
      const p: SplitPreview = {
        id: crypto.randomUUID(),
        original_title: c.title,
        new_title: title,
        before: Array.from(left).slice(0, 2000).join(""),
        after: Array.from(right).slice(0, 2000).join(""),
        before_bytes: enc.encode(left).length,
        after_bytes: enc.encode(right).length,
        before_truncated: Array.from(left).length > 2000,
        after_truncated: Array.from(right).length > 2000,
        expires_at: Date.now() / 1000 + 600,
      };
      current = { p, book, chapter, revision, left, right };
      return p;
    },
    detail: async (id, side) => {
      if (current?.p.id !== id) throw Error("预览失效");
      return side === "before" ? current.left : current.right;
    },
    discard: async (id) => {
      if (current?.p.id === id) current = null;
    },
    commit: async (id) => {
      if (current?.p.id !== id) throw Error("预览失效");
      const x = current;
      current = null;
      const s = structuredClone(get()),
        c = s.chapters.find((c) => c.id === x.chapter)!;
      if (c.revision !== x.revision) throw Error("正文冲突");
      const n = {
        ...c,
        id: crypto.randomUUID(),
        title: x.p.new_title,
        body: x.right,
        revision: 0,
      };
      const active = s.chapters
        .filter((a) => a.volume_id === c.volume_id && a.deleted_at == null)
        .sort(
          (a, b) => a.sort_order - b.sort_order || a.id.localeCompare(b.id),
        );
      const reserved = new Set(
        s.chapters
          .filter((a) => a.volume_id === c.volume_id && a.deleted_at != null)
          .map((a) => a.sort_order),
      );
      let order = 0;
      const nextOrder = () => {
        while (reserved.has(order)) order++;
        return order++;
      };
      for (const a of active) {
        a.sort_order = nextOrder();
        if (a.id === c.id) n.sort_order = nextOrder();
      }
      c.body = x.left;
      c.revision++;
      s.chapters.push(n);
      set(s);
      return {
        book_id: x.book,
        original_chapter_id: c.id,
        new_chapter_id: n.id,
      };
    },
  };
}
