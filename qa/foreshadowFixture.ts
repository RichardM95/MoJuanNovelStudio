// Fictional, in-memory UI fixture only. The real app never uses this adapter.
import type {
  ForeshadowAPI,
  ForeshadowProposal,
  Occurrence,
  OccurrenceInput,
  OccurrenceView,
  SourcePosition,
} from "../src/foreshadowTypes";
import type { Material, MaterialsAPI } from "../src/materialTypes";
import type { LibraryState } from "../src/types";

export function foreshadowFixture(
  state: () => LibraryState,
  materials: () => Material[],
  editor: () => MaterialsAPI,
): ForeshadowAPI {
  const epoch = "demo-foreshadow-epoch";
  const entries: Occurrence[] = [];
  const requests = new Map<string, { input: OccurrenceInput; id: string }>();
  function validateEpoch(e: string) {
    if (e !== epoch) throw { code: "epoch_changed", message: "演示书库已更换" };
  }
  function card(book: string, id: string) {
    const row = materials().find(
      (m) =>
        m.book_id === book &&
        m.id === id &&
        m.kind === "foreshadow" &&
        m.deleted_at === null,
    );
    if (!row) throw { code: "not_found", message: "伏笔卡不存在" };
    return row;
  }
  const key = (o: SourcePosition) =>
    [
      o.volume_sort_order,
      o.volume_id,
      o.chapter_sort_order,
      o.chapter_id,
    ] as const;
  function compare(a: SourcePosition, b: SourcePosition) {
    const x = key(a),
      y = key(b);
    return (
      x[0] - y[0] ||
      x[1].localeCompare(y[1]) ||
      x[2] - y[2] ||
      x[3].localeCompare(y[3])
    );
  }
  function directory(book: string): SourcePosition[] {
    const s = state();
    if (!s.books.some((b) => b.id === book && b.deleted_at === null)) return [];
    return s.chapters
      .flatMap((c) => {
        const v = s.volumes.find(
          (v) =>
            v.id === c.volume_id && v.book_id === book && v.deleted_at === null,
        );
        return c.deleted_at === null && v
          ? [
              {
                chapter_id: c.id,
                chapter_title: c.title,
                chapter_revision: c.revision,
                chapter_sort_order: c.sort_order,
                volume_id: v.id,
                volume_title: v.title,
                volume_sort_order: v.sort_order,
              },
            ]
          : [];
      })
      .sort(compare);
  }
  function view(occurrence: Occurrence): OccurrenceView {
    const current =
      directory(occurrence.book_id).find(
        (c) => c.chapter_id === occurrence.source.chapter_id,
      ) ?? null;
    return {
      occurrence: structuredClone(occurrence),
      current,
      source_status: !current
        ? "unavailable"
        : JSON.stringify(current) === JSON.stringify(occurrence.source)
          ? "current"
          : "stale",
    };
  }
  const initial = directory("b1").find((c) => c.chapter_id === "c1");
  const initialBody = state().chapters.find((c) => c.id === "c1")?.body ?? "";
  const quote = "老车站的钟却依然慢着五分钟";
  const offset = initialBody.indexOf(quote);
  let suggestion: ForeshadowProposal | undefined;
  if (initial && offset >= 0) {
    const start = Array.from(initialBody.slice(0, offset)).length;
    entries.push({
      id: "fictional-clock-excerpt",
      book_id: "b1",
      material_id: "thread-clock",
      source: initial,
      start,
      end: start + Array.from(quote).length,
      quote,
      role: "plant",
      note: "第一次露出钟表线索。",
      revision: 0,
      deleted_at: null,
      created_at: 1,
    });
  }
  const a = entries[0];
  const baseCard = materials().find(m=>m.id===a?.material_id);
  if (a && baseCard) suggestion = {
    receipt: {id:"demo-review-clock",book_id:"b1",material_id:a.material_id,material_revision:baseCard.revision,
      chapter_id:a.source.chapter_id,chapter_revision:a.source.chapter_revision,status:"pending",created_at:1,decided_at:null,accepted_occurrence_id:null},
    material_title:baseCard.title, archived:false, source:a.source, source_status:"current",card_current:true,
    start:a.start,end:a.end,quote:a.quote,role:"hint",note:"以原文中的慢钟作为提示。",
    reason:"演示：可对照现有出现，再由作者判断是否是同一个片段。",accepted_occurrence:null,
  };
  const detail = () => {
    if (!suggestion) throw {code:"not_found",message:"建议不存在"};
    const p = structuredClone(suggestion);
    const r = p.receipt;
    const m = materials().find(x=>x.id===r.material_id && x.book_id===r.book_id);
    const live = directory(r.book_id).find(x=>x.chapter_id===r.chapter_id);
    p.card_current = !!m && m.deleted_at===null && !m.content_review_required && m.revision===r.material_revision;
    p.source_status = !live ? "unavailable" : JSON.stringify(live)===JSON.stringify(p.source) ? "current" : "stale";
    const occurrence = entries.find(x=>x.id===r.accepted_occurrence_id);
    p.accepted_occurrence = occurrence ? view(occurrence) : null;
    return p;
  };
  const find = (book: string, id: string, oid: string) =>
    entries.find(
      (o) => o.id === oid && o.material_id === id && o.book_id === book,
    );
  const adapter: ForeshadowAPI = {
    proposals: {
      list: async (book,id,pending,after,e) => {
        validateEpoch(e);
        const r = suggestion?.receipt;
        const visible = r && !after && r.book_id===book && (id===null || r.material_id===id) && (!pending || r.status==="pending");
        const items = visible ? [{receipt:{...r},material_title:suggestion!.material_title,archived:false}] : [];
        return {items,pending_count:r?.book_id===book && (id===null || r.material_id===id) && r.status==="pending" ? 1 : 0,next_after_id:null};
      },
      get: async (book,id,proposal,e) => {
        validateEpoch(e);
        const p = detail(), r = p.receipt;
        if(r.book_id!==book || r.material_id!==id || r.id!==proposal) throw {code:"not_found",message:"建议不存在"};
        return p;
      },
      decide: async input => {
        const p = await adapter.proposals!.get(input.book_id,input.material_id,input.proposal_id,input.expected_epoch);
        const r = suggestion!.receipt;
        if (r.material_revision!==input.expected_material_revision || r.status!=="pending") throw {code:"invalid_state",message:"建议已过期"};
        if (input.action==="accept") {
          if (!p.card_current || p.source_status!=="current") throw {code:"revision_conflict",message:"来源或卡片已变化"};
          const o = await adapter.create({book_id:r.book_id,material_id:r.material_id,chapter_id:r.chapter_id,
            chapter_revision:r.chapter_revision,start:p.start,end:p.end,quote:p.quote,role:p.role,note:p.note,
            epoch:input.expected_epoch,request_key:"demo-proposal:"+r.id});
          r.accepted_occurrence_id=o.occurrence.id;
        }
        r.status=input.action==="accept" ? "accepted" : "rejected";
        r.decided_at=Date.now();
        return detail();
      },
    },
    epoch: async () => epoch,
    overview: async (book, e) => {
      validateEpoch(e);
      return (await editor().list(book, false))
        .filter((m) => m.kind === "foreshadow")
        .map((material) => {
          const live = entries.filter(
            (o) =>
              o.material_id === material.id &&
              o.book_id === book &&
              o.deleted_at === null,
          );
          return {
            material,
            occurrence_count: live.length,
            review_count: live.filter(
              (o) => view(o).source_status !== "current",
            ).length,
          };
        });
    },
    directory: async (book, e) => {
      validateEpoch(e);
      return directory(book);
    },
    chapter: async (book, id, e) => {
      validateEpoch(e);
      const position = directory(book).find((p) => p.chapter_id === id);
      const c = state().chapters.find((c) => c.id === id);
      if (!position || !c) throw { code: "not_found", message: "找不到来源章" };
      return { position, body: c.body };
    },
    list: async (book, id, e) => {
      validateEpoch(e);
      card(book, id);
      return entries
        .filter((o) => o.book_id === book && o.material_id === id)
        .map(view)
        .sort(
          (a, b) =>
            Number(!a.current) - Number(!b.current) ||
            compare(
              a.current ?? a.occurrence.source,
              b.current ?? b.occurrence.source,
            ),
        );
    },
    get: async (book, id, oid, e) => {
      validateEpoch(e);
      card(book, id);
      const o = find(book, id, oid);
      if (!o) throw { code: "not_found", message: "找不到出现记录" };
      return view(o);
    },
    lookup: async (book, id, k, e) => {
      validateEpoch(e);
      card(book, id);
      const q = requests.get(k);
      const o = q && find(book, id, q.id);
      return o ? view(o) : null;
    },
    create: async (input) => {
      validateEpoch(input.epoch);
      const m = card(input.book_id, input.material_id),
        prior = requests.get(input.request_key);
      if (prior) {
        const o = find(input.book_id, input.material_id, prior.id);
        if (!o || JSON.stringify(prior.input) !== JSON.stringify(input))
          throw { code: "idempotency_conflict", message: "请求不同" };
        return view(o);
      }
      const source = directory(input.book_id).find(
        (p) => p.chapter_id === input.chapter_id,
      );
      const c = state().chapters.find((c) => c.id === input.chapter_id);
      if (!source || !c || source.chapter_revision !== input.chapter_revision)
        throw { code: "revision_conflict", message: "来源已更新" };
      if (
        !input.quote ||
        Array.from(c.body).slice(input.start, input.end).join("") !==
          input.quote
      )
        throw { code: "invalid_input", message: "引文不匹配" };
      const o: Occurrence = {
        id: crypto.randomUUID(),
        book_id: input.book_id,
        material_id: input.material_id,
        source,
        start: input.start,
        end: input.end,
        quote: input.quote,
        role: input.role,
        note: input.note,
        revision: 0,
        deleted_at: null,
        created_at: Date.now(),
      };
      entries.push(o);
      requests.set(input.request_key, {
        input: structuredClone(input),
        id: o.id,
      });
      m.revision++;
      return view(o);
    },
    setDeleted: async (book, id, oid, revision, e, deleted) => {
      validateEpoch(e);
      const m = card(book, id),
        o = find(book, id, oid);
      if (!o || o.revision !== revision || (o.deleted_at !== null) === deleted)
        throw { code: "revision_conflict", message: "记录已变化" };
      o.revision++;
      o.deleted_at = deleted ? Date.now() : null;
      m.revision++;
      return view(o);
    },
    material: async (book, id, e) => {
      validateEpoch(e);
      card(book, id);
      return editor().get(id, false);
    },
    createCard: async (input, e) => {
      validateEpoch(e);
      return editor().create(input);
    },
    saveCard: async (book, input, e) => {
      validateEpoch(e);
      card(book, input.id);
      return editor().save(input);
    },
  };
  return adapter;
}
