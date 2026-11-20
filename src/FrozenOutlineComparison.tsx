import { useMemo, useState } from "react";
import type { WritingPack, WritingProposal } from "./writingTypes";

type Citation = WritingProposal["input"]["citations"][number];
type Option = {
  id: string;
  title: string;
  text: string;
  start: number;
  end: number;
  entry: string;
  field: string;
};

function offsets(text: string, requested: number[]): Map<number, number> {
  const endpoints = [...new Set(requested)].sort((a, b) => a - b);
  const found = new Map<number, number>();
  let scalar = 0;
  let offset = 0;
  for (const end of endpoints) {
    while (scalar < end && offset < text.length) {
      const value = text.codePointAt(offset)!;
      offset += value > 0xffff ? 2 : 1;
      scalar++;
    }
    if (scalar === end) found.set(end, offset);
  }
  return found;
}

// Never resolve a citation to the first of multiple records with the same handle.
function unique<T>(values: T[], matches: (value: T) => boolean): T | undefined {
  const found = values.filter(matches);
  return found.length === 1 ? found[0] : undefined;
}

function choices(pack: WritingPack, citations: Citation[]) {
  const outline: Option[] = [];
  const prior: Option[] = [];
  const groups = new Map<
    string,
    {
      text: string;
      title: string;
      list: { q: Citation; index: number }[];
      type: "outline" | "prior";
    }
  >();
  citations.forEach((q, index) => {
    if (
      !Number.isSafeInteger(q.start) ||
      !Number.isSafeInteger(q.end) ||
      q.start < 0 ||
      q.end <= q.start
    )
      return;
    const key = JSON.stringify([q.entry_id, q.field_key]);
    let group = groups.get(key);
    if (!group) {
      const m = unique(
        pack.materials,
        (m) =>
          ((pack.handle_version ?? 1) === 1 ? m.id : `material:${m.id}`) ===
          q.entry_id,
      );
      const context = unique(
        pack.context_chapters ?? [],
        ({ chapter }) => `context:${chapter.id}` === q.entry_id,
      );
      if (
        m?.kind === "outline" &&
        m.book_id === pack.book_id &&
        pack.policy.required_material_ids.includes(m.id) &&
        q.field_key !== "$title" &&
        !q.field_key.startsWith("occurrence:") &&
        Object.prototype.hasOwnProperty.call(m.fields, q.field_key) &&
        typeof m.fields[q.field_key] === "string"
      ) {
        group = {
          text: m.fields[q.field_key],
          title: `大纲 · ${m.title} · 版本 ${m.revision} · ${q.field_key}`,
          type: "outline",
          list: [],
        };
      } else if (
        context &&
        q.field_key === "body" &&
        context.chapter.id !== pack.chapter_id &&
        context.chapter.volume_id === context.volume.id &&
        context.volume.book_id === pack.book_id &&
        pack.policy.context_chapter_ids?.includes(context.chapter.id)
      ) {
        group = {
          text: context.chapter.body,
          title: `前文 · ${context.volume.title} / ${context.chapter.title} · 版本 ${context.chapter.revision}`,
          type: "prior",
          list: [],
        };
      } else return;
      groups.set(key, group);
    }
    if (q.end <= group.text.length) group.list.push({ q, index });
  });
  for (const { text, title, list, type } of groups.values()) {
    const positions = offsets(
      text,
      list.flatMap(({ q }) => [q.start, q.end]),
    );
    for (const { q, index } of list) {
      const start = positions.get(q.start);
      const end = positions.get(q.end);
      if (start === undefined || end === undefined) continue;
      (type === "outline" ? outline : prior).push({
        id: String(index),
        title: `${title} · [${q.start},${q.end})`,
        text,
        start,
        end,
        entry: q.entry_id,
        field: q.field_key,
      });
    }
  }
  return { outline, prior };
}

export default function FrozenOutlineComparison({
  pack,
  proposal,
}: {
  pack: WritingPack;
  proposal: WritingProposal;
}) {
  // WritingPanel already validates selection scope and guards late loads. Keep this
  // read-only view bound to that exact proposal and pack, never a live source.
  const { outline, prior } = useMemo(() => {
    if (
      proposal.input.pack_id !== pack.id ||
      proposal.book_id !== pack.book_id ||
      proposal.chapter_id !== pack.chapter_id ||
      (proposal.live_epoch !== null &&
        proposal.live_epoch !== pack.originating_epoch) ||
      pack.policy.book_id !== pack.book_id ||
      pack.policy.chapter_id !== pack.chapter_id ||
      pack.chapter.id !== pack.chapter_id ||
      proposal.base_revision !== pack.chapter.revision
    )
      return { outline: [], prior: [] };
    return choices(pack, proposal.input.citations);
  }, [
    pack,
    proposal.input.citations,
    proposal.input.pack_id,
    proposal.book_id,
    proposal.chapter_id,
    proposal.base_revision,
    proposal.live_epoch,
  ]);
  const [outlineId, setOutlineId] = useState("");
  const [priorId, setPriorId] = useState("");
  if (!outline.length || !prior.length) return null;
  const a = outline.find((x) => x.id === outlineId);
  const b = prior.find((x) => x.id === priorId);
  return (
    <details className="writing-outline-comparison">
      <summary>对照大纲与前文引文</summary>
      <p>
        只显示本建议的冻结引文。请各选一条；系统不判断冲突或决定以哪条为准。
      </p>
      {proposal.decision === "pending" && proposal.live_epoch !== null && (
        <p>
          尚未决定时，可在大纲卡的逐项作者审阅中选择「有疑问」或「有冲突」，保存备注而不采纳。
        </p>
      )}
      <div className="history-comparison">
        <label>
          选择大纲引文
          <select
            value={outlineId}
            onChange={(e) => setOutlineId(e.target.value)}
          >
            <option value="">请选择</option>
            {outline.map((x) => (
              <option key={x.id} value={x.id}>
                {x.title} · 引用 {Number(x.id) + 1}
              </option>
            ))}
          </select>
        </label>
        <label>
          选择前文引文
          <select value={priorId} onChange={(e) => setPriorId(e.target.value)}>
            <option value="">请选择</option>
            {prior.map((x) => (
              <option key={x.id} value={x.id}>
                {x.title} · 引用 {Number(x.id) + 1}
              </option>
            ))}
          </select>
        </label>
      </div>
      {a && b && (
        <div className="history-comparison">
          {[a, b].map((x) => (
            <article key={x.id}>
              <h5>{x.title}</h5>
              <small>
                {x.entry} / {x.field}
              </small>
              <blockquote className="foreshadow-quote">
                {x.text.slice(x.start, x.end)}
              </blockquote>
            </article>
          ))}
        </div>
      )}
    </details>
  );
}
