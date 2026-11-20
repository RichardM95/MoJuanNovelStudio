import { foreshadowFixture } from "./foreshadowFixture";
import {writingFixture} from "./writingFixture";
import {reviewSourceFixture} from "./reviewSourceFixture";
import {materialHistoryFixture} from "./materialHistoryFixture";
// Development-only visual fixture. Never used by the production entry point.
import React from "react";
import { createRoot } from "react-dom/client";
import App from "../src/App";
import { splitFixture } from "./splitFixture";
import { exportFixture } from "./exportFixture";
import { importFixture } from "./importFixture";
import { replaceFixture } from "./replaceFixture";
import { backupFixture } from "./backupFixture";
import { historyFixture } from "./historyFixture";
import { taskFixture } from "./taskFixture";
import "../src/style.css";
import type { API, LibraryState } from "../src/types";
import type { Material, MaterialProposal } from "../src/materialTypes";
let state: LibraryState = {
  books: [
    { id: "b1", title: "远山来信", author: "林舟", deleted_at: null },
    { id: "b2", title: "长夜行舟", author: "林舟", deleted_at: null },
  ],
  volumes: [
    { id: "v1", book_id: "b1", title: "归途", sort_order: 0, deleted_at: null },
    {
      id: "v2",
      book_id: "b1",
      title: "山海之间",
      sort_order: 1,
      deleted_at: null,
    },
  ],
  chapters: [
    {
      id: "c1",
      volume_id: "v1",
      title: "风从山那边来",
      body: "火车驶过最后一座隧道时，天色已经暗了。\n\n林溪把那封没有落款的信重新折好，放进外套口袋。窗外的群山退成深蓝的剪影，远处亮起一盏灯。\n\n她离开这座小城已经七年。七年里，河岸的树长高了，老车站的钟却依然慢着五分钟。",
      revision: 0,
      sort_order: 0,
      deleted_at: null,
    },
    {
      id: "c2",
      volume_id: "v1",
      title: "旧站台",
      body: "",
      revision: 0,
      sort_order: 1,
      deleted_at: null,
    },
    {
      id: "c3",
      volume_id: "v2",
      title: "无声的潮汐",
      body: "",
      revision: 0,
      sort_order: 0,
      deleted_at: null,
    },
  ],
};
let pending = true;
const original = state.chapters[0].body;
let materialRows: Material[] = [
  {
    id: "m-setting",
    book_id: "b1",
    kind: "setting",
    title: "远山来信 · 作品设定",
    fields: {
      synopsis: "一封无名来信，让林溪回到阔别七年的故乡。",
      genre: "现实 / 悬疑",
      world: "南方山城。老车站与河岸贯穿故事。",
      style: "克制、细腻，以场景推进。",
      rules: "不使用超自然线索；每条伏笔都应回收。",
    },
    volume_id: null,
    chapter_id: null,
    revision: 0,
    deleted_at: null,
    updated_at: 1,
  },
  {
    id: "m-character",
    book_id: "b1",
    kind: "character",
    title: "林溪",
    fields: {
      identity: "离乡七年的修复师",
      personality: "谨慎，观察敏锐",
      appearance: "常穿深蓝色外套",
      goal: "找到寄信的人",
      relationships: "与父亲疏远；旧友留在山城",
      status: "刚刚回到家乡",
    },
    volume_id: null,
    chapter_id: null,
    revision: 0,
    deleted_at: null,
    updated_at: 1,
  },
  {
    id: "m-outline",
    book_id: "b1",
    kind: "outline",
    title: "归途 · 卷纲",
    fields: { content: "抵达车站 → 遇见旧友 → 发现来信的第一条线索。" },
    volume_id: "v1",
    chapter_id: null,
    revision: 0,
    deleted_at: null,
    updated_at: 1,
  },
];
const materialLive = (m: Material) =>
  !m.deleted_at &&
  (!m.volume_id ||
    state.volumes.some((v) => v.id === m.volume_id && !v.deleted_at)) &&
  (!m.chapter_id ||
    state.chapters.some(
      (c) =>
        c.id === m.chapter_id &&
        !c.deleted_at &&
        state.volumes.some((v) => v.id === c.volume_id && !v.deleted_at),
    ));
materialRows.push({
  id: "m-summary",
  book_id: "b1",
  kind: "summary",
  title: "归乡 · 第一章摘要",
  fields: { content: "林溪带着无名来信回到故乡。" },
  volume_id: null,
  chapter_id: null,
  source_chapter_id: "c1",
  source_revision: 0,
  source_status: "current",
  revision: 0,
  deleted_at: null,
  updated_at: 1,
});
materialRows.push(
  {
    id: "event-arrival",
    book_id: "b1",
    kind: "timeline",
    title: "抵达旧站台",
    fields: { content: "林溪傍晚抵达，发现老钟仍慢五分钟。" },
    story_order: 10,
    story_time: "第一日 · 傍晚",
    tracking_status: "written",
    source_chapter_id: "c1",
    source_revision: 0,
    volume_id: null,
    chapter_id: null,
    revision: 0,
    deleted_at: null,
    updated_at: 1,
  },
  {
    id: "event-letter",
    book_id: "b1",
    kind: "timeline",
    title: "第二封信",
    fields: { content: "第二天在河岸找到新的来信。" },
    story_order: 20,
    story_time: "第二日 · 清晨",
    tracking_status: "planned",
    source_chapter_id: null,
    source_revision: null,
    volume_id: null,
    chapter_id: null,
    revision: 0,
    deleted_at: null,
    updated_at: 1,
  },
  {
    id: "thread-clock",
    book_id: "b1",
    kind: "foreshadow",
    title: "慢五分钟的钟",
    fields: { setup: "第一章提到老车站的钟慢五分钟。", resolution: "" },
    story_order: 10,
    story_time: null,
    tracking_status: "planted",
    source_chapter_id: "c1",
    source_revision: 0,
    payoff_chapter_id: null,
    payoff_revision: null,
    volume_id: null,
    chapter_id: null,
    revision: 0,
    deleted_at: null,
    updated_at: 1,
  },
);
function withSource(m: Material): Material {
  const c = state.chapters.find((c) => c.id === m.source_chapter_id);
  const payoff = state.chapters.find((c) => c.id === m.payoff_chapter_id);
  return {
    ...structuredClone(m),
    story_order: m.story_order ?? 0,
    story_time: m.story_time ?? null,
    tracking_status: m.tracking_status ?? null,
    payoff_chapter_id: m.payoff_chapter_id ?? null,
    payoff_revision: m.payoff_revision ?? null,
    payoff_status: !m.payoff_chapter_id
      ? "none"
      : !payoff || payoff.deleted_at !== null
        ? "unavailable"
        : payoff.revision === m.payoff_revision && !m.content_review_required
          ? "current"
          : "stale",
    source_status: !m.source_chapter_id
      ? "none"
      : !c || c.deleted_at !== null
        ? "unavailable"
        : c.revision === m.source_revision && !m.content_review_required
          ? "current"
          : "stale",
  };
}
const materialProposal: MaterialProposal = {
  id: "mp-demo",
  client_id: "虚构演示",
  material_id: "m-summary",
  base_revision: 0,
  old_snapshot: structuredClone(materialRows[3]),
  new_title: "归乡 · 第一章摘要",
  new_fields: {
    content:
      "林溪阔别故乡七年，因一封没有落款的信踏上归途。老站钟仍慢着五分钟。",
  },
  new_source_revision: 0,
  reason: "补充时间跨度和场景线索，供人工核对。",
  status: "pending",
  created_at: 1,
};
const materialHistory=materialHistoryFixture(()=>materialRows,id=>state.chapters.find(c=>c.id===id&&c.deleted_at===null)?.revision);
const api: API = {
  foreshadow:foreshadowFixture(()=>state,()=>materialRows,()=>api.materials!),
  writing:writingFixture(()=>state,s=>{state=s;},()=>materialRows),
  split: splitFixture(()=>state,s=>{state=s;}),
  txtExport: exportFixture(() => state),
  txtImport: importFixture(() => state, next => {state=next;}),
  replace: replaceFixture(() => state, next => {state=next;}),
  backups: backupFixture(()=>state, next=>{state=next;}),
  tasks: taskFixture(() => ({
    state,
    materials: materialRows.map(withSource),
  })),
  searchBook: async (bookId, query, limit) => {
    const items = [
      ...state.chapters
        .filter(
          (c) =>
            state.volumes.some(
              (v) =>
                v.id === c.volume_id &&
                v.book_id === bookId &&
                v.deleted_at === null,
            ) &&
            c.deleted_at === null &&
            (c.title.includes(query) || c.body.includes(query)),
        )
        .map((c) => ({
          kind: "chapter" as const,
          id: c.id,
          title: c.title,
          snippet: c.body.slice(0, 200),
          revision: c.revision,
        })),
      ...materialRows
        .filter(
          (m) =>
            m.book_id === bookId &&
            materialLive(m) &&
            (m.title.includes(query) ||
              m.story_time?.includes(query) ||
              Object.values(m.fields).some((f) => f.includes(query))),
        )
        .map((m) => ({
          kind: "material" as const,
          id: m.id,
          title: m.title,
          snippet: [m.story_time ?? "", ...Object.values(m.fields)]
            .join(" ")
            .slice(0, 200),
          revision: m.revision,
        })),
    ];
    return { items: items.slice(0, limit), truncated: items.length > limit };
  },
  materials: {
    history: materialHistory.api,
    review: {
      list: async (bookId) =>
        bookId === "b1" && materialProposal.status === "pending"
          ? [structuredClone(materialProposal)]
          : [],
      decide: async (id, approve, revision) => {
        const m = materialRows.find(
          (m) => m.id === materialProposal.material_id,
        )!;
        if (id !== materialProposal.id || materialProposal.status !== "pending")
          throw Error("提案不可用");
        if (approve) {
          if (
            m.revision !== revision ||
            m.revision !== materialProposal.base_revision ||
            state.chapters[0].revision !== materialProposal.new_source_revision
          )
            throw Error("资料或来源版本已变化，请重新提交建议");
          m.title = materialProposal.new_title;
          m.fields = structuredClone(materialProposal.new_fields);
          m.source_revision = materialProposal.new_source_revision;
          m.revision++;
        }
        materialProposal.status = approve ? "accepted" : "rejected";
        materialHistory.record(m);
      return withSource(m);
      },
    },
    list: async (bookId, includeDeleted) =>
      materialRows
        .filter(
          (m) => m.book_id === bookId && (includeDeleted || materialLive(m)),
        )
        .map((m) => {
          const { fields, ...meta } = withSource(m);
          return meta;
        }),
    get: async (id, includeDeleted) => {
      const m = materialRows.find(
        (m) => m.id === id && (includeDeleted || materialLive(m)),
      );
      if (!m) throw Error("资料不可访问");
      materialHistory.record(m);
      return withSource(m);
    },
    create: async (input) => {
      const m: Material = {
        ...input,
        id: crypto.randomUUID(),
        revision: 0,
        deleted_at: null,
        updated_at: Date.now(),
      };
      materialRows.push(m);
      materialHistory.record(m);
      return withSource(m);
    },
    save: async (input) => {
      const index = materialRows.findIndex((m) => m.id === input.id);
      const m = materialRows[index];
      if (!m || m.revision !== input.expected_revision)
        throw Error("资料版本已变化，请保留草稿并重新读取");
      const { expected_revision, ...change } = input;
      materialRows[index] = {
        ...m,
        ...change,
        revision: m.revision + 1,
        updated_at: Date.now(),
      };
      materialHistory.record(materialRows[index]);
      return withSource(materialRows[index]);
    },
    trash: async (id, revision) => {
      const m = materialRows.find((m) => m.id === id);
      if (!m || m.revision !== revision) throw Error("资料版本冲突");
      m.deleted_at = Date.now();
      m.revision++;
      materialHistory.record(m);
      return withSource(m);
    },
    restore: async (id, revision) => {
      const m = materialRows.find((m) => m.id === id);
      if (!m || m.revision !== revision) throw Error("资料版本冲突");
      m.deleted_at = null;
      m.revision++;
      materialHistory.record(m);
      return withSource(m);
    },
  },
  workflow: {
    versionBrowser: historyFixture(()=>state, next=>{state=next;}),
    listProposals: async () =>
      pending
        ? [
            {
              id: "proposal-demo",
              client_id: "demo",
              chapter_id: "c1",
              base_revision: 0,
              old_body: original,
              new_body: original + "\n\n站台尽头，有人举起了那把熟悉的蓝伞。",
              reason: "补充场景收束，保留原有叙述。",
              source: "演示客户端（非真实连接）",
              status: "pending",
              created_at: 1,
            },
          ]
        : [],
    history: async (id) =>
      id === "c1"
        ? [
            {
              id: "history-demo",
              chapter_id: "c1",
              revision: 0,
              body: original,
              source: "gui",
              created_at: 1,
            },
          ]
        : [],
    decideProposal: async (_id, approve, expected) => {
      const c = state.chapters[0];
      if (approve && c.revision !== expected)
        throw Error("章节版本已变化，不能覆盖");
      if (approve)
        state = {
          ...state,
          chapters: [
            {
              ...c,
              body: original + "\n\n站台尽头，有人举起了那把熟悉的蓝伞。",
              revision: c.revision + 1,
            },
            ...state.chapters.slice(1),
          ],
        };
      pending = false;
      return structuredClone(state);
    },
    restoreVersion: async (_id, expected) => {
      const c = state.chapters[0];
      if (c.revision !== expected) throw Error("章节版本已变化");
      state = {
        ...state,
        chapters: [
          { ...c, body: original, revision: c.revision + 1 },
          ...state.chapters.slice(1),
        ],
      };
      return structuredClone(state);
    },
  },
  load: async () => structuredClone(state),
  apply: async (cmd) => {
    if (cmd.type === "save_chapter")
      state = {
        ...state,
        chapters: state.chapters.map((c) =>
          c.id === cmd.id
            ? { ...c, body: String(cmd.body), revision: c.revision + 1 }
            : c,
        ),
      };
    const action = String(cmd.type).split("_")[0],
      kind = String(cmd.type).split("_")[1];
    if (action === "trash" || action === "restore") {
      const deleted_at = action === "trash" ? Date.now() : null;
      if (kind === "book")
        state = {
          ...state,
          books: state.books.map((b) =>
            b.id === cmd.id ? { ...b, deleted_at } : b,
          ),
        };
      if (kind === "volume")
        state = {
          ...state,
          volumes: state.volumes.map((v) =>
            v.id === cmd.id ? { ...v, deleted_at } : v,
          ),
        };
      if (kind === "chapter")
        state = {
          ...state,
          chapters: state.chapters.map((c) =>
            c.id === cmd.id ? { ...c, deleted_at } : c,
          ),
        };
    }
    if (action === "purge") {
      const removedVolumes = new Set(
        state.volumes
          .filter((v) =>
            kind === "book"
              ? v.book_id === cmd.id
              : kind === "volume" && v.id === cmd.id,
          )
          .map((v) => v.id),
      );
      state = {
        books: state.books.filter((b) => !(kind === "book" && b.id === cmd.id)),
        volumes: state.volumes.filter((v) => !removedVolumes.has(v.id)),
        chapters: state.chapters.filter(
          (c) =>
            !removedVolumes.has(c.volume_id) &&
            !(kind === "chapter" && c.id === cmd.id),
        ),
      };
    }
    return structuredClone(state);
  },
  exportBook: async () => null,
  backup: async () => null,
  restore: async () => null,
};
if (import.meta.env.DEV) {
  const review = new URLSearchParams(location.search).get("review-source");
  createRoot(document.getElementById("root")!).render(<App api={
    review === "current" || review === "stale" ? reviewSourceFixture(api, review === "stale") : api
  } />);
}
