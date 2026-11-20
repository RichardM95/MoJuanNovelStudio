// Vitest runs in Node; the browser-only project intentionally has no @types/node.
// @ts-expect-error Node filesystem is available in the test runner only.
import { readFileSync } from "node:fs";
declare const process: { cwd(): string };
const taskStyles = readFileSync(`${process.cwd()}/src/style.css`, "utf8");
import React from "react";
import {
  render,
  screen,
  fireEvent,
  waitFor,
  cleanup,
  act,
} from "@testing-library/react";
import { it, expect, vi, afterEach } from "vitest";
import TasksPanel from "../src/TasksPanel";
import type { Task, TaskDetail, TasksAPI } from "../src/taskTypes";
afterEach(() => {
  cleanup();
  vi.useRealTimers();
});
const task = (id = "t", status: Task["status"] = "completed"): Task => ({
  id,
  book_id: "b",
  kind: "context_check",
  status,
  done: 1,
  total: 3,
  created_at: 1,
  updated_at: 2,
  started_at: 1,
  finished_at: status === "completed" ? 2 : null,
  reason_code: null,
  error_message: null,
  retry_of: null,
});
const detail = (t: Task): TaskDetail => ({
  task: t,
  items: [
    {
      ordinal: 0,
      kind: "chapter",
      item_id: "c",
      title: "第一章",
      revision: 3,
      char_count: 42,
      material_kind: null,
      source_status: null,
      payoff_status: null,
      tracking_status: null,
      needs_review: false,
    },
  ],
});
function setup(status: Task["status"] = "completed") {
  const initial = task("t", status);
  const api: TasksAPI = {
    list: vi.fn(async () => [initial]),
    get: vi.fn(async (_b, id) => detail(task(id, status))),
    start: vi.fn(async () => task("new", "queued")),
    cancel: vi.fn(async () => task("t", "cancelled")),
    resume: vi.fn(async () => task("t", "running")),
    retry: vi.fn(async () => task("retry", "queued")),
  };
  const onClose = vi.fn(),
    onOpen = vi.fn();
  const view = render(
    <TasksPanel bookId="b" api={api} onClose={onClose} onOpen={onOpen} />,
  );
  return { api, onClose, onOpen, view };
}
it("shows committed progress and metadata only, and opens an item by verified kind/id", async () => {
  const { onOpen } = setup();
  await screen.findByText("已检查 1 / 3 项");
  expect(screen.getByText(/章节 · 42 字/)).toBeTruthy();
  expect(screen.getByText(/不随以后稿件修改自动更新/)).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: /第一章/ }));
  expect(onOpen).toHaveBeenCalledWith({ kind: "chapter", id: "c" });
});
it("marks interrupted reports partial and offers explicit continuation", async () => {
  const { api } = setup("interrupted");
  await screen.findByText(/部分结果/);
  fireEvent.click(screen.getByRole("button", { name: "继续检查点" }));
  await waitFor(() => expect(api.resume).toHaveBeenCalledWith("b", "t"));
});
it("reports completion rather than claiming cancellation when cancel loses the race", async () => {
  const { api } = setup("running");
  vi.mocked(api.cancel).mockResolvedValue(task("t", "completed"));
  await screen.findByRole("button", { name: "取消检查" });
  fireEvent.click(screen.getByRole("button", { name: "取消检查" }));
  await waitFor(() => expect(api.cancel).toHaveBeenCalledWith("b", "t"));
  expect(screen.queryByText("已取消")).toBeNull();
});
it("preserves action idempotency key after start failure and blocks duplicate clicks", async () => {
  const { api } = setup();
  await screen.findByText("已检查 1 / 3 项");
  vi.mocked(api.start).mockRejectedValueOnce(new Error("暂时失败"));
  fireEvent.click(screen.getByRole("button", { name: "新建检查" }));
  await screen.findByText("暂时失败");
  const key = vi.mocked(api.start).mock.calls[0][1];
  let finish!: (task: Task) => void;
  vi.mocked(api.start).mockImplementationOnce(
    () => new Promise((r) => (finish = r)),
  );
  fireEvent.click(screen.getByRole("button", { name: "新建检查" }));
  fireEvent.click(screen.getByRole("button", { name: "新建检查" }));
  expect(api.start).toHaveBeenCalledTimes(2);
  expect(vi.mocked(api.start).mock.calls[1][1]).toBe(key);
  await act(async () => finish(task("new", "queued")));
});
it("ignores a late request after changing the selected task", async () => {
  const { api } = setup();
  await screen.findByText("已检查 1 / 3 项");
  vi.mocked(api.list).mockResolvedValue([task("t"), task("other")]);
  fireEvent.click(screen.getByRole("button", { name: "刷新任务列表" }));
  await screen.findByRole("button", { name: /other/ });
  let finish!: (value: TaskDetail) => void;
  vi.mocked(api.get).mockImplementationOnce(
    () => new Promise((r) => (finish = r)),
  );
  fireEvent.click(screen.getByRole("button", { name: /other/ }));
  fireEvent.click(screen.getByRole("button", { name: /任务 t / }));
  await screen.findByText("已检查 1 / 3 项");
  await act(async () =>
    finish({
      ...detail(task("other")),
      items: [{ ...detail(task()).items[0], title: "迟到条目" }],
    }),
  );
  expect(screen.queryByText("迟到条目")).toBeNull();
});
it("stops polling terminal reports and allows closing while an action is pending", async () => {
  const { api, onClose } = setup();
  await screen.findByText("已检查 1 / 3 项");
  vi.useFakeTimers();
  await act(async () => {
    vi.advanceTimersByTime(3000);
  });
  expect(api.get).toHaveBeenCalledTimes(1);
  vi.mocked(api.start).mockImplementation(() => new Promise(() => {}));
  fireEvent.click(screen.getByRole("button", { name: "新建检查" }));
  fireEvent.click(screen.getByRole("button", { name: "关闭检查任务" }));
  expect(onClose).toHaveBeenCalled();
});
it("polls serially and stops once the server returns a terminal status", async () => {
  vi.useFakeTimers();
  const { api } = setup("running");
  await act(async () => {});
  expect(api.get).toHaveBeenCalledTimes(1);
  let finish!: (value: TaskDetail) => void;
  vi.mocked(api.get).mockImplementationOnce(
    () => new Promise((resolve) => (finish = resolve)),
  );
  await act(async () => {
    vi.advanceTimersByTime(1000);
  });
  expect(api.get).toHaveBeenCalledTimes(2);
  await act(async () => {
    vi.advanceTimersByTime(5000);
  });
  expect(api.get).toHaveBeenCalledTimes(2);
  await act(async () => finish(detail(task("t", "completed"))));
  await act(async () => {
    vi.advanceTimersByTime(5000);
  });
  expect(api.get).toHaveBeenCalledTimes(2);
  expect(screen.getByRole("heading", { name: "已完成" })).toBeTruthy();
});
it("retains report and retry key after retry failure, and creates a new task on success", async () => {
  const { api } = setup("failed");
  await screen.findByText("已检查 1 / 3 项");
  vi.mocked(api.retry).mockRejectedValueOnce(new Error("重试暂不可用"));
  fireEvent.click(screen.getByRole("button", { name: "重新检查" }));
  await screen.findByText("重试暂不可用");
  expect(screen.getByRole("button", { name: "第一章" })).toBeTruthy();
  const key = vi.mocked(api.retry).mock.calls[0][2];
  fireEvent.click(screen.getByRole("button", { name: "重新检查" }));
  await waitFor(() =>
    expect(api.retry).toHaveBeenLastCalledWith("b", "t", key),
  );
  await screen.findByRole("button", { name: /任务 retry/ });
  expect(screen.getByRole("button", { name: /任务 t / })).toBeTruthy();
});
it("does not keep polling when a pending detail resolves after unmount", async () => {
  vi.useFakeTimers();
  const { api, view } = setup("running");
  await act(async () => {});
  let finish!: (value: TaskDetail) => void;
  vi.mocked(api.get).mockImplementationOnce(
    () => new Promise((resolve) => (finish = resolve)),
  );
  await act(async () => {
    vi.advanceTimersByTime(1000);
  });
  view.unmount();
  await act(async () => finish(detail(task("t", "running"))));
  await act(async () => {
    vi.advanceTimersByTime(5000);
  });
  expect(api.get).toHaveBeenCalledTimes(2);
});
it("paginates report metadata without truncating the true summary counts", async () => {
  const { api } = setup();
  await screen.findByText("已检查 1 / 3 项");
  const items = Array.from({ length: 101 }, (_, index) => ({
    ...detail(task()).items[0],
    ordinal: index,
    item_id: "c" + index,
    title: "章节" + index,
    char_count: 2,
  }));
  vi.mocked(api.get).mockResolvedValue({
    task: { ...task(), done: 101, total: 101 },
    items,
  });
  fireEvent.click(screen.getByRole("button", { name: "刷新当前报告" }));
  await screen.findByText(/已统计章节 101/);
  expect(screen.queryByRole("button", { name: "章节100" })).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "下一页" }));
  expect(screen.getByRole("button", { name: "章节100" })).toBeTruthy();
  expect(screen.getByText(/非空白字符 202/)).toBeTruthy();
});
it("rejects cross-book task details and does not expose their report", async () => {
  const { api } = setup();
  await screen.findByText("已检查 1 / 3 项");
  vi.mocked(api.get).mockResolvedValue({
    task: { ...task(), book_id: "other" },
    items: [{ ...detail(task()).items[0], title: "别书秘密" }],
  });
  fireEvent.click(screen.getByRole("button", { name: "刷新当前报告" }));
  await screen.findByText("任务所属作品或标识不匹配");
  expect(screen.queryByText("别书秘密")).toBeNull();
  expect(screen.getByRole("button", { name: "第一章" })).toBeTruthy();
});
it("pauses polling after a runner error and retains the last committed report until manual refresh", async () => {
  vi.useFakeTimers();
  const { api } = setup("running");
  await act(async () => {});
  vi.mocked(api.get).mockRejectedValueOnce(
    new Error("后台检查已停止，请重开应用恢复"),
  );
  await act(async () => {
    vi.advanceTimersByTime(1000);
  });
  expect(screen.getByText("后台检查已停止，请重开应用恢复")).toBeTruthy();
  expect(screen.getByText(/自动刷新已暂停/)).toBeTruthy();
  expect(screen.getByRole("button", { name: "第一章" })).toBeTruthy();
  await act(async () => {
    vi.advanceTimersByTime(5000);
  });
  expect(api.get).toHaveBeenCalledTimes(2);
  vi.mocked(api.get).mockResolvedValue(detail(task()));
  fireEvent.click(screen.getByRole("button", { name: "刷新当前报告" }));
  await act(async () => {});
  expect(api.get).toHaveBeenCalledTimes(3);
  expect(screen.queryByText(/自动刷新已暂停/)).toBeNull();
});
it("immediately hides the old book report and resets selection when book scope changes", async () => {
  const { api, view, onClose, onOpen } = setup();
  await screen.findByText("已检查 1 / 3 项");
  let finish!: (value: Task[]) => void;
  vi.mocked(api.list).mockImplementationOnce(
    () => new Promise((resolve) => (finish = resolve)),
  );
  view.rerender(
    <TasksPanel
      bookId="new-book"
      api={api}
      onClose={onClose}
      onOpen={onOpen}
    />,
  );
  expect(screen.queryByRole("button", { name: "第一章" })).toBeNull();
  expect(screen.queryByText("已检查 1 / 3 项")).toBeNull();
  expect(
    vi
      .mocked(api.get)
      .mock.calls.some(([book, id]) => book === "new-book" && id === "t"),
  ).toBe(false);
  const next = { ...task("new-id"), book_id: "new-book" };
  vi.mocked(api.get).mockResolvedValue({ task: next, items: [] });
  await act(async () => finish([next]));
  await waitFor(() =>
    expect(api.get).toHaveBeenLastCalledWith("new-book", "new-id"),
  );
});
it("ignores completion of an old-book action after switching scope", async () => {
  const { api, view, onClose, onOpen } = setup();
  await screen.findByText("已检查 1 / 3 项");
  let finish!: (value: Task) => void;
  vi.mocked(api.start).mockImplementationOnce(
    () => new Promise((resolve) => (finish = resolve)),
  );
  fireEvent.click(screen.getByRole("button", { name: "新建检查" }));
  const next = { ...task("new-book-task"), book_id: "new-book" };
  vi.mocked(api.list).mockResolvedValue([next]);
  vi.mocked(api.get).mockResolvedValue({ task: next, items: [] });
  view.rerender(
    <TasksPanel
      bookId="new-book"
      api={api}
      onClose={onClose}
      onOpen={onOpen}
    />,
  );
  await screen.findByRole("button", { name: /任务 new-book-task/ });
  await act(async () => finish(task("late-old", "queued")));
  expect(screen.queryByRole("button", { name: /late-old/ })).toBeNull();
  expect(
    screen.getByRole("button", { name: /任务 new-book-task/ }),
  ).toBeTruthy();
  expect(
    (screen.getByRole("button", { name: "新建检查" }) as HTMLButtonElement)
      .disabled,
  ).toBe(false);
});
it("keeps task report flush with its grid column and renders a short task identifier", async () => {
  const { api } = setup();
  await screen.findByText("已检查 1 / 3 项");
  const id = "12345678-abcd-4321-9876-123456789abc";
  vi.mocked(api.list).mockResolvedValue([task(id)]);
  fireEvent.click(screen.getByRole("button", { name: "刷新任务列表" }));
  await screen.findByRole("button", { name: new RegExp(id) });
  expect(screen.queryByText(id)).toBeNull();
  expect(screen.getByText("#12345678")).toBeTruthy();
  expect(
    (screen.getByRole("dialog").querySelector("main") as HTMLElement).style
      .marginLeft,
  ).toBe("0px");
});

it("stacks task record lines, keeps status unbroken, and separates task action buttons", async () => {
  const style = document.createElement("style");
  style.textContent = taskStyles;
  document.head.appendChild(style);
  try {
    setup("interrupted");
    await screen.findByText("已检查 1 / 3 项");
    const record = screen.getByRole("button", { name: /任务 t / });
    expect(getComputedStyle(record).display).toBe("grid");
    const status = record.querySelector(".task-record-status")!;
    expect(getComputedStyle(status).whiteSpace).toBe("nowrap");
    const actions = screen.getByRole("button", {
      name: "继续检查点",
    }).parentElement!;
    expect(getComputedStyle(actions).gap).toBe("14px");
  } finally {
    style.remove();
  }
});
