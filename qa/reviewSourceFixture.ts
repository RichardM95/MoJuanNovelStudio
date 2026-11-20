// Development-only mock bridge data; the original sample also drives Rust acceptance.
import sample from "./acceptance/longrange-foreshadow.txt?raw";
import type { API, LibraryState } from "../src/types";
import type { Material } from "../src/materialTypes";
import type {
  ForeshadowSourceReader,
  Occurrence,
  SourcePosition,
} from "../src/foreshadowTypes";
import type { WritingPack, WritingProposal } from "../src/writingTypes";

export function reviewSourceFixture(api: API, stale: boolean): API {
  const book = {
    id: "review-book",
    title: "苇门背潮",
    author: "原创验收样本",
    deleted_at: null,
  };
  const volumes = ["上卷", "下卷"].map((title, sort_order) => ({
    id: "review-v" + sort_order,
    book_id: book.id,
    title,
    sort_order,
    deleted_at: null,
  }));
  const lines = sample
    .trimEnd()
    .split("\n")
    .map((line) => line.split("\t"));
  const chapters = lines.map(([title, body], i) => ({
    id: "review-ch" + (i + 1),
    volume_id: volumes[Number(i >= 12)].id,
    title: `第 ${i + 1} 章 · ${title}`,
    body: i === 20 ? "凌汐在闸墙外系好引缆。" : body,
    sort_order: i < 12 ? i : i - 12,
    revision: 1,
    deleted_at: null,
  }));
  const epoch = "review-source-qa";
  const source = chapters[1],
    target = chapters[20];
  const position: SourcePosition = {
    chapter_id: source.id,
    chapter_title: source.title,
    chapter_revision: 1,
    chapter_sort_order: source.sort_order,
    volume_id: volumes[0].id,
    volume_title: volumes[0].title,
    volume_sort_order: 0,
  };
  const card: Material = {
    id: "review-card",
    book_id: book.id,
    title: "背潮方铜牌",
    kind: "foreshadow",
    revision: 0,
    deleted_at: null,
    updated_at: 1,
    volume_id: null,
    chapter_id: null,
    fields: { setup: "留意暗号与倒钉，区分西仓的圆锡牌。" },
  };
  const quote = "两短一长，走背潮。";
  const start = [...source.body.slice(0, source.body.indexOf(quote))].length;
  const end = start + [...quote].length;
  const occurrence: Occurrence = {
    id: "review-occurrence",
    book_id: book.id,
    material_id: card.id,
    source: position,
    start,
    end,
    quote,
    role: "plant",
    note: "第2章铺设",
    revision: 0,
    deleted_at: null,
    created_at: 1,
  };
  const priors = chapters.slice(12, 20);
  const pack: WritingPack = {
    id: "review-pack",
    book_id: book.id,
    chapter_id: target.id,
    book,
    volume: volumes[1],
    chapter: target,
    originating_epoch: epoch,
    created_at: 1,
    expires_at: Date.now() / 1000 + 7200,
    policy: {
      book_id: book.id,
      chapter_id: target.id,
      revision: 0,
      mode: "active",
      updated_at: 1,
      goal: "辨清背潮门，但不要把西仓响铃当作回收。",
      required_material_ids: [card.id],
      context_chapter_ids: priors.map((ch) => ch.id),
    },
    materials: [card],
    foreshadow_occurrences: [occurrence],
    handle_version: 3,
    context_chapters: priors.map((chapter) => ({
      chapter,
      volume: volumes[1],
    })),
  };
  const proposal: WritingProposal = {
    id: "review-proposal",
    book_id: book.id,
    chapter_id: target.id,
    client_id: "固定提案",
    base_revision: 1,
    decision: "pending",
    live_epoch: epoch,
    accepted_revision: null,
    input: {
      pack_id: pack.id,
      new_body: lines[20][1],
      idempotency_key: "review-fixture",
      conflicts: [],
      citations: [
        {
          entry_id: `material:${card.id}`,
          field_key: `occurrence:${occurrence.id}`,
          start: 0,
          end: end - start,
          explanation: "核对暗号的早期出现",
        },
      ],
    },
  };
  const saved = stale
    ? { ...source, revision: 2, body: source.body + "她又把针脚收紧了一点。" }
    : source;
  const state: LibraryState = {
    books: [book],
    volumes,
    chapters: chapters.map((ch) => (ch.id === source.id ? saved : ch)),
  };
  const sourceWindow: ForeshadowSourceReader["sourceWindow"] = async (
    b,
    c,
    e,
    lo,
    hi,
  ) => {
    if (b !== book.id || c !== source.id || e !== epoch)
      throw { code: "not_found" };
    const chars = [...saved.body];
    return {
      position: { ...position, chapter_revision: saved.revision },
      before: chars.slice(Math.max(0, lo - 80), lo).join(""),
      at_range: chars.slice(lo, hi).join(""),
      after: chars.slice(hi, hi + 80).join(""),
    };
  };
  const readOnly = async (): Promise<never> => {
    throw Error("此预览只用于核对来源，未连接保存或采纳。");
  };
  return {
    ...api,
    load: async () => state,
    apply: readOnly,
    materials: { ...api.materials!, list: async () => [card] },
    foreshadow: { ...api.foreshadow!, epoch: async () => epoch, sourceWindow },
    writing: {
      ...api.writing!,
      epoch: async () => epoch,
      state: async (_b, c) => ({
        epoch,
        policy: c === target.id ? pack.policy : null,
        packs: [],
        proposals:
          c === target.id
            ? [
                {
                  id: proposal.id,
                  pack_id: pack.id,
                  decision: "pending",
                  archived: false,
                  created_at: 1,
                },
              ]
            : [],
      }),
      contextDirectory: async (b, c) => ({
        book_id: b,
        chapter_id: c,
        epoch,
        volumes,
        chapters: priors,
      }),
      pack: async () => pack,
      proposal: async () => proposal,
      configure: readOnly,
      prepare: readOnly,
      saveReview: readOnly,
      decide: readOnly,
      acceptReviewed: readOnly,
    },
  };
}
