import { useEffect, useRef, useState } from "react";
import type { WritingAPI, WritingPack, WritingPredecessorDiagnosis, WritingPredecessorMetadata } from "./writingTypes";

type Reader = NonNullable<WritingAPI["predecessorDiagnosis"]>;
type Read =
  | { kind: "idle" | "loading" | "unknown" }
  | { kind: "ready"; result: WritingPredecessorDiagnosis };

function metadata(value: WritingPredecessorMetadata | null, id: string, book: string) {
  return !!value && value.id === id && typeof value.title === "string" &&
    Number.isSafeInteger(value.revision) && Number.isSafeInteger(value.sort_order) &&
    value.volume?.book_id === book && value.volume.deleted_at === null &&
    typeof value.volume.id === "string" && !!value.volume.id &&
    typeof value.volume.title === "string" && Number.isSafeInteger(value.volume.sort_order);
}

function compareId(a: string, b: string) {
  const encoder = new TextEncoder(), l = encoder.encode(a), r = encoder.encode(b);
  for (let i = 0; i < Math.min(l.length, r.length); i++) {
    if (l[i] !== r[i]) return l[i] - r[i];
  }
  return l.length - r.length;
}

// Same total ordering as the library: equal sort numbers use IDs, not an
// assumed move. An identical full key is not a trustworthy predecessor.
function compare(a: WritingPredecessorMetadata, b: WritingPredecessorMetadata) {
  return a.volume.sort_order - b.volume.sort_order || compareId(a.volume.id, b.volume.id) ||
    a.sort_order - b.sort_order || compareId(a.id, b.id);
}

function matches(r: WritingPredecessorDiagnosis, p: WritingPack, epoch: string) {
  const ids = p.policy.context_chapter_ids ?? [];
  const frozen = p.context_chapters ?? [];
  if (!r || r.epoch !== epoch || r.book_id !== p.book_id || r.chapter_id !== p.chapter_id ||
    r.pack_id !== p.id || !metadata(r.target, p.chapter_id, p.book_id) ||
    ids.length === 0 || ids.length > 8 || new Set(ids).size !== ids.length ||
    frozen.length !== ids.length || !Array.isArray(r.entries) || r.entries.length !== ids.length ||
    new Set(frozen.map(f => f.chapter.id)).size !== ids.length) return false;

  // All metadata came from one directory read; a volume can have only one
  // position, even when it's not the target's volume.
  const volumeRanks = new Map([[r.target.volume.id, r.target.volume.sort_order]]);
  return frozen.every(({chapter, volume}, i) => {
    const id = chapter.id, e = r.entries[i];
    if (id === p.chapter_id || !ids.includes(id) || volume.book_id !== p.book_id ||
      !e || e.chapter_id !== id) return false;
    if (e.position === "not_visible") return e.current === null;
    if (!metadata(e.current, id, p.book_id) || !e.current) return false;
    const {id: currentVolume, sort_order: rank} = e.current.volume;
    const seen = volumeRanks.get(currentVolume);
    if (seen !== undefined && seen !== rank) return false;
    volumeRanks.set(currentVolume, rank);
    const order = compare(e.current, r.target);
    return e.position === "before" ? order < 0 : e.position === "after" && order > 0;
  });
}

export default function FrozenPredecessorDiagnosis({
  reader, pack, epoch, isScopeCurrent, onAfter, onChoose,
}: {
  reader: Reader;
  pack: WritingPack;
  epoch: string;
  isScopeCurrent: () => boolean;
  onAfter: () => void;
  onChoose: () => void;
}) {
  const [read, setRead] = useState<Read>({ kind: "idle" });
  const generation = useRef(0);
  const readButton = useRef<HTMLButtonElement>(null);
  const scope = useRef(isScopeCurrent);
  scope.current = isScopeCurrent;
  useEffect(() => () => { generation.current++; }, []);
  async function check() {
    const token = ++generation.current;
    const current = () => generation.current === token && scope.current();
    setRead({ kind: "loading" });
    try {
      if (!current() || await reader.epoch() !== epoch || !current()) throw Error();
      const r = await reader.read(pack.book_id, pack.chapter_id, pack.id, epoch);
      if (!current() || await reader.epoch() !== epoch || !current() || !matches(r, pack, epoch)) throw Error();
      if (r.entries.some(e => e.position === "after")) onAfter();
      setRead({ kind: "ready", result: r });
    } catch {
      if (generation.current === token && scope.current()) setRead({ kind: "unknown" });
    }
  }
  return <section aria-label="冻结前文目录核对">
    <button type="button" ref={readButton} disabled={read.kind === "loading"} onClick={() => void check()}>
      核对冻结前文的目录位置
    </button>
    {read.kind === "loading" && <>
      <p role="status">正在只读核对目录。</p>
      <button type="button" onClick={() => {
        generation.current++;
        setRead({ kind: "idle" });
        requestAnimationFrame(() => readButton.current?.focus());
      }}>停止核对前文</button>
    </>}
    {read.kind === "unknown" && <p role="status">无法核对冻结前文的当前位置；不能据此判断是否移序。</p>}
    {read.kind === "ready" && <>
      <p>按本次读取的目录位置显示，不会实时更新，也不代表故事内时间。</p>
      <ul>
        {read.result.entries.map((e, i) => {
          const frozen = pack.context_chapters![i];
          const label = `${frozen.volume.title} / ${frozen.chapter.title}`;
          const currentLabel = e.current &&
            (e.current.volume.title !== frozen.volume.title || e.current.title !== frozen.chapter.title)
            ? `冻结时：${label}；当前：${e.current.volume.title} / ${e.current.title}`
            : label;
          return <li key={e.chapter_id}>
            {currentLabel}：
            {e.position === "after" ? "已在目标章之后" : e.position === "before" ? "仍在目标章之前（只核对位置）" :
              "当前活动目录不可见；原因未确定"}
          </li>;
        })}
      </ul>
    </>}
    <button type="button" onClick={onChoose}>前往前文选择</button>
    <p>选章或移除需手动操作。如有未保存审阅，请先处理备注；可刷新依据与建议，更新选择区目录，再自行保存配置并冻结新依据。</p>
  </section>;
}
