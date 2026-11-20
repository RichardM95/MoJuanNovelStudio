// Browser-only, in-memory QA fixture. Production uses SQLite and TaskRunner.
import type { TasksAPI, TaskDetail, Task, TaskItem } from "../src/taskTypes";
import type { LibraryState } from "../src/types";
import type { Material } from "../src/materialTypes";
export function taskFixture(
  source: () => { state: LibraryState; materials: Material[] },
): TasksAPI {
  type Work = {
    detail: TaskDetail;
    plan: {
      kind: "chapter" | "material";
      id: string;
      revision: number;
      title: string;
    }[];
  };
  const jobs = new Map<string, Work>(),
    keys = new Map<string, string>();
  const now = () => Math.floor(Date.now() / 1000);
  const get = (book: string, id: string) => {
    const w = jobs.get(id);
    if (!w || w.detail.task.book_id !== book) throw Error("任务不可访问");
    return w;
  };
  function make(book: string, retry_of: string | null): Work {
    const { state, materials } = source();
    const plan = [
      ...state.chapters
        .filter(
          (c) =>
            c.deleted_at === null &&
            state.volumes.some(
              (v) =>
                v.id === c.volume_id &&
                v.book_id === book &&
                v.deleted_at === null,
            ),
        )
        .map((c) => ({
          kind: "chapter" as const,
          id: c.id,
          revision: c.revision,
          title: c.title,
        })),
      ...materials
        .filter((m) => m.book_id === book && m.deleted_at === null)
        .map((m) => ({
          kind: "material" as const,
          id: m.id,
          revision: m.revision,
          title: m.title,
        })),
    ];
    const task: Task = {
      id: crypto.randomUUID(),
      book_id: book,
      kind: "context_check",
      status: "queued",
      done: 0,
      total: plan.length,
      created_at: now(),
      updated_at: now(),
      started_at: null,
      finished_at: null,
      reason_code: null,
      error_message: null,
      retry_of,
    };
    const w = { detail: { task, items: [] }, plan };
    jobs.set(task.id, w);
    return w;
  }
  function step(w: Work) {
    const t = w.detail.task;
    if (!["queued", "running"].includes(t.status)) return;
    t.status = "running";
    t.started_at ??= now();
    const p = w.plan[t.done];
    if (!p) {
      t.status = "completed";
      t.finished_at = now();
      return;
    }
    const { state, materials } = source();
    const c = state.chapters.find((c) => c.id === p.id),
      m = materials.find((m) => m.id === p.id);
    if ((p.kind === "chapter" ? c?.revision : m?.revision) !== p.revision) {
      t.status = "interrupted";
      t.reason_code = "input_changed";
      t.error_message = "检查期间作品已变化，请重新检查";
      return;
    }
    const item: TaskItem = {
      ordinal: t.done,
      kind: p.kind,
      item_id: p.id,
      title: p.title,
      revision: p.revision,
      char_count:
        p.kind === "chapter"
          ? Array.from(c!.body).filter((x) => !/\s/u.test(x)).length
          : null,
      material_kind: m?.kind ?? null,
      source_status: m?.source_status ?? null,
      payoff_status: m?.payoff_status ?? null,
      tracking_status: m?.tracking_status ?? null,
      needs_review:
        !!m &&
        (["stale", "unavailable"].includes(m.source_status ?? "") ||
          ["stale", "unavailable"].includes(m.payoff_status ?? "") ||
          (m.kind === "foreshadow" &&
            ["planned", "planted"].includes(m.tracking_status ?? ""))),
    };
    w.detail.items.push(item);
    t.done++;
    t.updated_at = now();
    if (t.done === t.total) {
      t.status = "completed";
      t.finished_at = now();
    }
  }
  function pump(w: Work) {
    queueMicrotask(() => {
      step(w);
      if (["queued", "running"].includes(w.detail.task.status)) pump(w);
    });
  }
  let seeded = false;
  return {
    list: async (book) => {
      if (!seeded && book === "b1") {
        seeded = true;
        const w = make(book, null);
        step(w);
        w.detail.task.status = "interrupted";
        w.detail.task.reason_code = "restart";
        w.detail.task.error_message = "虚构示例：上次检查已中断";
      }
      return [...jobs.values()]
        .map((w) => w.detail.task)
        .filter((t) => t.book_id === book)
        .map((t) => structuredClone(t));
    },
    get: async (book, id) => structuredClone(get(book, id).detail),
    start: async (book, key) => {
      const old = keys.get(`start:${book}:${key}`);
      if (old) return structuredClone(get(book, old).detail.task);
      const w = make(book, null);
      keys.set(`start:${book}:${key}`, w.detail.task.id);
      pump(w);
      return structuredClone(w.detail.task);
    },
    cancel: async (book, id) => {
      const t = get(book, id).detail.task;
      if (["queued", "running", "interrupted"].includes(t.status)) {
        t.status = "cancelled";
        t.finished_at = now();
      }
      return structuredClone(t);
    },
    resume: async (book, id) => {
      const w = get(book, id);
      if (
        w.detail.task.status === "interrupted" &&
        w.detail.task.reason_code === "restart"
      ) {
        w.detail.task.status = "queued";
        w.detail.task.reason_code = null;
        w.detail.task.error_message = null;
        pump(w);
      } else throw Error("输入已变化，请重新检查");
      return structuredClone(w.detail.task);
    },
    retry: async (book, id, key) => {
      get(book, id);
      const old = keys.get(`retry:${id}:${key}`);
      if (old) return structuredClone(get(book, old).detail.task);
      const w = make(book, id);
      keys.set(`retry:${id}:${key}`, w.detail.task.id);
      pump(w);
      return structuredClone(w.detail.task);
    },
  };
}
