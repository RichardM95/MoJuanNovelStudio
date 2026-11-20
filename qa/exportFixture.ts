// Fictional browser-only rendering. No native file dialog or disk write.
import type {
  ExportAPI,
  ExportOptions,
  ExportPreview,
} from "../src/exportTypes";
import type { LibraryState } from "../src/types";
export function exportFixture(get: () => LibraryState): ExportAPI {
  let current: ExportPreview | null = null;
  return {
    prepare: async (bookId: string, o: ExportOptions) => {
      const s = get(),
        b = s.books.find((x) => x.id === bookId)!;
      let text = o.include_metadata ? `${b.title}\n作者：${b.author}\n\n` : "";
      let volumes = 0,
        chapters = 0;
      for (const v of s.volumes
        .filter((v) => v.book_id === bookId && v.deleted_at == null)
        .sort(
          (a, b) => a.sort_order - b.sort_order || a.id.localeCompare(b.id),
        )) {
        const cs = s.chapters
          .filter(
            (c) =>
              c.volume_id === v.id &&
              c.deleted_at == null &&
              (o.scope !== "chapter" || c.id === o.target_id),
          )
          .sort(
            (a, b) => a.sort_order - b.sort_order || a.id.localeCompare(b.id),
          );
        if (
          (o.scope === "volume" && v.id !== o.target_id) ||
          (o.scope === "chapter" && !cs.length)
        )
          continue;
        volumes++;
        if (o.include_headings) text += `第${volumes}卷 ${v.title}\n\n`;
        for (const c of cs) {
          chapters++;
          if (o.include_headings) text += `第${chapters}章 ${c.title}\n\n`;
          text += c.body + "\n\n";
        }
      }
      text = text.replace(/\r\n|\r|\n/g, o.newline === "crlf" ? "\r\n" : "\n");
      const chars = Array.from(text);
      current = {
        id: crypto.randomUUID(),
        bytes: new TextEncoder().encode(text).length + (o.bom ? 3 : 0),
        volumes,
        chapters,
        text: chars.slice(0, 4000).join(""),
        truncated: chars.length > 4000,
        expires_at: Date.now() / 1000 + 600,
        bom: o.bom,
        newline: o.newline,
      };
      return current;
    },
    save: async (id) => {
      if (current?.id !== id) throw Error("预览已失效");
      current = null;
      return "浏览器虚构演示（未写入文件）";
    },
    discard: async (id) => {
      if (current?.id === id) current = null;
    },
  };
}
