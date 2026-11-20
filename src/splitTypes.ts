export type SplitTarget = {
  bookId: string;
  chapterId: string;
  revision: number;
  offset: number;
  title: string;
};
export type SplitPreview = {
  id: string;
  original_title: string;
  new_title: string;
  before: string;
  after: string;
  before_bytes: number;
  after_bytes: number;
  before_truncated: boolean;
  after_truncated: boolean;
  expires_at: number;
};
export type SplitOutcome = {
  book_id: string;
  original_chapter_id: string;
  new_chapter_id: string;
};
export interface SplitAPI {
  prepare(
    bookId: string,
    chapterId: string,
    revision: number,
    offset: number,
    title: string,
  ): Promise<SplitPreview>;
  detail(id: string, side: "before" | "after"): Promise<string>;
  commit(id: string): Promise<SplitOutcome>;
  discard(id: string): Promise<void>;
}
/** textarea exposes LF-normalized text/UTF16 offsets, while untouched DB text may contain CRLF/CR. */
export function rawCursorOffset(
  raw: string,
  visible: string,
  start: number,
  end: number,
): number {
  if (start !== end) throw Error("请取消文字选择，将光标放在拆分位置");
  if (!Number.isInteger(start) || start <= 0 || start >= visible.length)
    throw Error("请将光标放在正文内部");
  if (raw.replace(/\r\n?/g, "\n") !== visible)
    throw Error("编辑器内容已变化，请重新放置光标");
  let normalized = 0,
    found = -1;
  for (let i = 0; i < raw.length;) {
    if (normalized === start) found = i;
    const point = raw.codePointAt(i)!;
    const length = point > 0xffff ? 2 : 1;
    if (raw[i] === "\r") {
      i += raw[i + 1] === "\n" ? 2 : 1;
      normalized++;
    } else {
      i += length;
      normalized += length;
    }
    if (normalized > start && found < 0) throw Error("不能拆开Unicode代理对");
  }
  if (found <= 0 || found >= raw.length) throw Error("请重新放置光标");
  return found;
}
