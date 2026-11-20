// Fictional in-memory UI fixture only. Not a model, transport or native validation.
import type { LibraryState } from "../src/types";
import type { Material } from "../src/materialTypes";
import type {
  AuthorReview,
  WritingAPI,
  WritingPolicy,
  WritingPack,
  WritingProposal,
  WritingManifest,
} from "../src/writingTypes";
export function writingFixture(
  get: () => LibraryState,
  set: (s: LibraryState) => void,
  materials: () => Material[],
): WritingAPI {
  const policies = new Map<string, WritingPolicy>(),
    packs = new Map<string, WritingPack>(),
    proposals = new Map<string, WritingProposal>();
  let next = 0;
  const epoch = "fictional-writing-fixture";
  const manifest = (p: WritingPack): WritingManifest => ({
    id: p.id,
    book_id: p.book_id,
    chapter_id: p.chapter_id,
    policy_revision: p.policy.revision,
    chapter_revision: p.chapter.revision,
    goal: p.policy.goal,
    expires_at: p.expires_at,
    archived: false,
    entries: [],
  });
  const compareId = (a: string, b: string) => (a < b ? -1 : a > b ? 1 : 0);
  function contextDirectory(b: string, c: string) {
    const s = get();
    const volumes = s.volumes
      .filter((v) => v.book_id === b && v.deleted_at === null)
      .sort((a, b) => a.sort_order - b.sort_order || compareId(a.id, b.id));
    const byVolume = new Map(volumes.map((v, i) => [v.id, i]));
    const chapters = s.chapters
      .filter((ch) => ch.deleted_at === null && byVolume.has(ch.volume_id))
      .sort(
        (a, b) =>
          byVolume.get(a.volume_id)! - byVolume.get(b.volume_id)! ||
          a.sort_order - b.sort_order ||
          compareId(a.id, b.id),
      );
    const index = chapters.findIndex((ch) => ch.id === c);
    if (index < 0) throw Error("演示章节不可用");
    const earlier = chapters
      .slice(0, index)
      .map(({ body: _body, deleted_at: _deleted, ...row }) => row);
    return { epoch, book_id: b, chapter_id: c, volumes, chapters: earlier };
  }
  function snapshotContexts(b: string, c: string, ids: string[]) {
    const directory = contextDirectory(b, c);
    const s = get();
    const selected = directory.chapters.filter((ch) => ids.includes(ch.id));
    if (selected.length !== ids.length || ids.length > 8)
      throw Error("演示前文不可用或超限");
    return selected.map((ch) => ({
      chapter: s.chapters.find((row) => row.id === ch.id)!,
      volume: directory.volumes.find((v) => v.id === ch.volume_id)!,
    }));
  }
  function sameContexts(pack: WritingPack) {
    try {
      return (
        JSON.stringify(
          snapshotContexts(
            pack.book_id,
            pack.chapter_id,
            pack.policy.context_chapter_ids ?? [],
          ),
        ) === JSON.stringify(pack.context_chapters ?? [])
      );
    } catch {
      return false;
    }
  }
  return {
    epoch: async () => epoch,
    contextDirectory: async (b, c) => contextDirectory(b, c),
    state: async (b, c) => ({
      epoch,
      policy: policies.get(c) ?? null,
      packs: [...packs.values()]
        .filter((p) => p.book_id === b && p.chapter_id === c)
        .map(manifest),
      proposals: [...proposals.values()]
        .filter((p) => p.book_id === b && p.chapter_id === c)
        .map((p) => ({
          id: p.id,
          pack_id: p.input.pack_id,
          decision: p.decision,
          archived: false,
          created_at: 1,
        })),
    }),
    configure: async (b, c, r, mode, goal, ids, e, contextIds) => {
      if (e !== epoch || (policies.get(c)?.revision ?? null) !== r)
        throw Error("演示配置版本改变");
      if (
        contextIds.length > 8 ||
        new Set(contextIds).size !== contextIds.length
      )
        throw Error("演示前文选择无效或超限");
      const prior = policies.get(c)?.context_chapter_ids ?? [];
      if (mode === "active") snapshotContexts(b, c, contextIds);
      else
        snapshotContexts(
          b,
          c,
          contextIds.filter((id) => !prior.includes(id)),
        );
      const p: WritingPolicy = {
        book_id: b,
        chapter_id: c,
        revision: (r ?? -1) + 1,
        mode,
        goal,
        required_material_ids: ids,
        context_chapter_ids: contextIds,
        updated_at: Date.now() / 1000,
      };
      policies.set(c, p);
      for (const q of proposals.values())
        if (q.chapter_id === c && q.decision === "pending")
          q.decision = "cancelled";
      return structuredClone(p);
    },
    prepare: async (b, c, pr, cr, e) => {
      const policy = policies.get(c),
        s = get(),
        chapter = s.chapters.find((x) => x.id === c);
      if (
        !policy ||
        !chapter ||
        policy.mode !== "active" ||
        policy.revision !== pr ||
        chapter.revision !== cr ||
        e !== epoch
      )
        throw Error("演示配置已改变");
      const cards = policy.required_material_ids.map((id) =>
        materials().find((m) => m.id === id),
      );
      if (
        cards.some(
          (m) => !m || m.deleted_at !== null || m.content_review_required,
        )
      )
        throw Error("演示资料待复核或不可用");
      const pack: WritingPack = structuredClone({
        handle_version: 3,
        id: `fixture-pack-${++next}`,
        book_id: b,
        chapter_id: c,
        originating_epoch: epoch,
        created_at: Date.now() / 1000,
        expires_at: Date.now() / 1000 + 1800,
        policy,
        book: s.books.find((x) => x.id === b)!,
        volume: s.volumes.find((x) => x.id === chapter.volume_id)!,
        chapter,
        materials: cards as Material[],
        context_chapters: snapshotContexts(
          b,
          c,
          policy.context_chapter_ids ?? [],
        ),
      });
      packs.set(pack.id, pack);
      const q: WritingProposal = {
        id: `虚构建议-${next}`,
        book_id: b,
        chapter_id: c,
        client_id: "虚构演示（未连接智能体）",
        base_revision: cr,
        decision: "pending",
        live_epoch: epoch,
        accepted_revision: null,
        input: {
          pack_id: pack.id,
          new_body:
            chapter.body + "\n\n【虚构演示建议，请人工核对设定与大纲。】",
          idempotency_key: String(next),
          citations: pack.materials.map((m) => {
            const key =
                Object.keys(m.fields).find((k) => m.fields[k].length > 0) ??
                "$title",
              v = key === "$title" ? m.title : m.fields[key];
            return {
              entry_id: `material:${m.id}`,
              field_key: key,
              start: 0,
              end: Math.min(16, Array.from(v).length),
              explanation: "虚构演示引用，不代表语义核验。",
            };
          }),
          conflicts: [
            {
              entry_ids: policy.required_material_ids,
              explanation:
                "演示建议未自动验证人物、情节与约束一致性，请作者判断。",
            },
          ],
        },
      };
      proposals.set(q.id, q);
      return manifest(pack);
    },
    pack: async (b, id) => {
      const p = packs.get(id);
      if (!p || p.book_id !== b) throw Error("不存在");
      return structuredClone(p);
    },
    proposal: async (b, id) => {
      const p = proposals.get(id);
      if (!p || p.book_id !== b) throw Error("不存在");
      return structuredClone(p);
    },
    saveReview: async (b, id, expected, e, items) => {
      const q = proposals.get(id);
      const pack = q && packs.get(q.input.pack_id);
      const p = q && policies.get(q.chapter_id);
      const chapter = q && get().chapters.find((x) => x.id === q.chapter_id);
      if (
        !q ||
        !pack ||
        !p ||
        q.book_id !== b ||
        q.decision !== "pending" ||
        e !== epoch ||
        q.live_epoch !== epoch ||
        pack.expires_at * 1000 <= Date.now() ||
        p.mode !== "active" ||
        p.revision !== pack.policy.revision ||
        chapter?.revision !== q.base_revision ||
        (q.author_review?.revision ?? null) !== expected ||
        items.length !== pack.materials.length ||
        !pack.materials.every((m) =>
          items.some((i) => i.material_id === m.id),
        ) ||
        items.some((i) => new TextEncoder().encode(i.note).length > 4096) ||
        !sameContexts(pack) ||
        pack.materials.some(
          (m) =>
            JSON.stringify(materials().find((x) => x.id === m.id)) !==
            JSON.stringify(m),
        )
      )
        throw Error("请重新核对演示依据和审阅版本");
      const r: AuthorReview = {
        proposal_id: q.id,
        pack_id: pack.id,
        book_id: b,
        chapter_id: q.chapter_id,
        originating_epoch: e,
        policy_revision: p.revision,
        chapter_revision: q.base_revision,
        revision: (expected ?? 0) + 1,
        items,
      };
      if (new TextEncoder().encode(JSON.stringify(r)).length > 64 * 1024)
        throw Error("演示审阅编码超64KiB");
      q.author_review = structuredClone(r);
      return structuredClone(r);
    },
    acceptReviewed: async (b, id, pr, cr, e, expected, ack) => {
      const q = proposals.get(id);
      const pack = q && packs.get(q.input.pack_id);
      const p = q && policies.get(q.chapter_id);
      const s = get();
      const chapter = q && s.chapters.find((c) => c.id === q.chapter_id);
      if (
        !q ||
        !pack ||
        q.book_id !== b ||
        q.decision !== "pending" ||
        e !== epoch ||
        q.live_epoch !== e ||
        pack.expires_at * 1000 <= Date.now() ||
        (!!q.input.conflicts.length && !ack) ||
        p?.mode !== "active" ||
        p.revision !== pr ||
        pack.policy.revision !== pr ||
        chapter?.revision !== cr ||
        cr !== q.base_revision ||
        !q.author_review ||
        q.author_review.revision !== expected ||
        !q.author_review.items.every((i) => i.verdict === "conforms") ||
        !sameContexts(pack) ||
        pack.materials.some(
          (m) =>
            JSON.stringify(materials().find((x) => x.id === m.id)) !==
            JSON.stringify(m),
        )
      )
        throw Error("请重新核对演示依据、冲突和逐项审阅");
      set({
        ...s,
        chapters: s.chapters.map((c) =>
          c.id === q.chapter_id
            ? { ...c, body: q.input.new_body, revision: c.revision + 1 }
            : c,
        ),
      });
      q.accepted_revision = cr + 1;
      q.decision = "accepted";
      return structuredClone(q);
    },
    decide: async (b, id, yes, _pr, _cr, e) => {
      const q = proposals.get(id);
      if (!q || q.book_id !== b || q.decision !== "pending" || e !== epoch)
        throw Error("演示提案已失效");
      if (yes) throw Error("采纳必须提供已保存的逐项审阅");
      q.decision = "rejected";
      return structuredClone(q);
    },
  };
}
