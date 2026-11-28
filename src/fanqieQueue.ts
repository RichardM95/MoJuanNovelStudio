import type { FanqieChapter } from "./fanqieTypes";

export type QueueItem = {
  chapter: FanqieChapter;
  targetVolume: string;
  number: string;
};
export const MAX_FANQIE_BATCH = 50;

export function queueItems(
  chapters: FanqieChapter[],
  chosen: ReadonlySet<string>,
  volumes: Readonly<Record<string, string>>,
  first: Readonly<Record<string, string>>,
  overrides: Readonly<Record<string, string>>,
): QueueItem[] {
  const used = new Map<string, Set<string>>();
  const counts = new Map<string, number>();
  const items = chapters
    .filter((c) => chosen.has(c.chapter_id))
    .map((chapter) => {
      const id = chapter.volume_id;
      const offset = counts.get(id) ?? 0;
      counts.set(id, offset + 1);
      const start = first[id] ?? "";
      const suggestion = /^[1-9]\d{0,8}$/.test(start)
        ? String(Number(start) + offset)
        : "";
      return {
        chapter,
        targetVolume: (volumes[id] ?? chapter.volume_title).trim(),
        number: overrides[chapter.chapter_id] ?? suggestion,
      };
    });
  if (!items.length || items.length > MAX_FANQIE_BATCH)
    throw Error("请选择 1 至 50 章");
  for (const { number, targetVolume } of items) {
    if (!targetVolume || !/^[1-9]\d{0,8}$/.test(number))
      throw Error("请确认目标卷和每章的正整数章号");
    const inVolume = used.get(targetVolume) ?? new Set<string>();
    if (inVolume.has(number)) throw Error("同一目标卷有重复章号，请核对");
    inVolume.add(number);
    used.set(targetVolume, inVolume);
  }
  return items;
}
