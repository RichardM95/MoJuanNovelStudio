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
import HistoryPanel from "../src/HistoryPanel";
import type {
  HistoryAPI,
  VersionSummary,
  VersionDetail,
} from "../src/historyTypes";
import type { Chapter } from "../src/types";
afterEach(cleanup);
const chapter: Chapter = {
  id: "c",
  volume_id: "v",
  title: "章",
  body: "当前稿",
  revision: 3,
  sort_order: 0,
  deleted_at: null,
};
const meta = (id = "h", rev = 1): VersionSummary => ({
  id,
  chapter_id: "c",
  revision: rev,
  source: "gui",
  created_at: 1,
  label: null,
  label_revision: 0,
});
const body = (id = "h"): VersionDetail => ({
  version: { ...meta(id), body: "历史正文" + id },
  label: null,
  label_revision: 0,
  epoch: "epoch1",
});
function setup() {
  const api: HistoryAPI = {
    list: vi.fn(async () => ({
      items: [meta()],
      next_cursor: null,
      epoch: "epoch1",
    })),
    get: vi.fn(async (_b, _c, id) => body(id)),
    label: vi.fn(async (_b, _c, id, label) => ({
      ...meta(id),
      label,
      label_revision: 1,
    })),
    restore: vi.fn(async () => ({ books: [], volumes: [], chapters: [] })),
  };
  const perform = vi.fn(async (fn: () => Promise<unknown>) => {
      await fn();
      return true;
    }),
    onPendingChange = vi.fn();
  const view = render(
    <HistoryPanel
      bookId="b"
      chapter={chapter}
      api={api}
      perform={perform}
      busy={false}
      onPendingChange={onPendingChange}
    />,
  );
  return { api, perform, view, onPendingChange };
}
it("loads only metadata until one version is clicked", async () => {
  const { api } = setup();
  await screen.findByRole("button", { name: /版本 1/ });
  expect(api.list).toHaveBeenCalledWith("b", "c", null, 50, false);
  expect(api.get).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: /版本 1/ }));
  await screen.findByText("历史正文h");
  expect(api.get).toHaveBeenCalledWith("b", "c", "h");
});
it("fetches another metadata page without reading its bodies", async () => {
  const { api } = setup();
  await screen.findByRole("button", { name: /版本 1/ });
  vi.mocked(api.list)
    .mockResolvedValueOnce({
      items: [meta()],
      next_cursor: "next",
      epoch: "epoch1",
    })
    .mockResolvedValueOnce({
      items: [meta("h2", 2)],
      next_cursor: null,
      epoch: "epoch1",
    });
  fireEvent.click(screen.getByRole("button", { name: "刷新历史" }));
  await screen.findByRole("button", { name: "加载下一页" });
  fireEvent.click(screen.getByRole("button", { name: "加载下一页" }));
  await screen.findByRole("button", { name: /版本 2/ });
  expect(api.list).toHaveBeenLastCalledWith("b", "c", "next", 50, false);
  expect(api.get).not.toHaveBeenCalled();
});
it("retains dirty name on conflict and blocks filtering until explicit cancellation", async () => {
  const { api, onPendingChange } = setup();
  await screen.findByRole("button", { name: /版本 1/ });
  fireEvent.click(screen.getByRole("button", { name: /版本 1/ }));
  await screen.findByText("历史正文h");
  fireEvent.change(screen.getByLabelText("版本名称"), {
    target: { value: "关键转折" },
  });
  expect(onPendingChange).toHaveBeenLastCalledWith(true);
  fireEvent.click(screen.getByRole("checkbox", { name: "仅显示已命名版本" }));
  expect((screen.getByRole("checkbox") as HTMLInputElement).checked).toBe(
    false,
  );
  vi.mocked(api.label).mockRejectedValueOnce(new Error("名称版本冲突"));
  fireEvent.click(screen.getByRole("button", { name: "保存名称" }));
  await screen.findByText("名称版本冲突");
  expect(screen.getByDisplayValue("关键转折")).toBeTruthy();
  expect(api.label).toHaveBeenCalledWith(
    "b",
    "c",
    "h",
    "关键转折",
    0,
    "epoch1",
  );
  fireEvent.click(screen.getByRole("button", { name: "取消名称编辑" }));
  expect((screen.getByLabelText("版本名称") as HTMLInputElement).value).toBe(
    "",
  );
});
it("restores only after bound confirmation with detail epoch and current chapter revision", async () => {
  const { api } = setup();
  await screen.findByRole("button", { name: /版本 1/ });
  fireEvent.click(screen.getByRole("button", { name: /版本 1/ }));
  await screen.findByText("历史正文h");
  fireEvent.click(screen.getByRole("button", { name: "恢复此版本" }));
  expect(api.restore).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "确认恢复" }));
  await waitFor(() =>
    expect(api.restore).toHaveBeenCalledWith("b", "c", "h", 3, "epoch1"),
  );
});
it("rejects a detail with a different chapter and never offers restore", async () => {
  const { api } = setup();
  vi.mocked(api.get).mockResolvedValue({
    ...body(),
    version: { ...body().version, chapter_id: "other" },
  });
  await screen.findByRole("button", { name: /版本 1/ });
  fireEvent.click(screen.getByRole("button", { name: /版本 1/ }));
  await screen.findByRole("alert");
  expect(screen.queryByText("历史正文h")).toBeNull();
  expect(screen.queryByRole("button", { name: "恢复此版本" })).toBeNull();
});
it("ignores an older version body after the author selects another version", async () => {
  const { api } = setup();
  await screen.findByRole("button", { name: /版本 1/ });
  vi.mocked(api.list).mockResolvedValue({
    items: [meta(), meta("h2", 2)],
    next_cursor: null,
    epoch: "epoch1",
  });
  fireEvent.click(screen.getByRole("button", { name: "刷新历史" }));
  await screen.findByRole("button", { name: /版本 2/ });
  let finish!: (value: VersionDetail) => void;
  vi.mocked(api.get).mockImplementationOnce(
    () => new Promise((resolve) => (finish = resolve)),
  );
  fireEvent.click(screen.getByRole("button", { name: /版本 1/ }));
  fireEvent.click(screen.getByRole("button", { name: /版本 2/ }));
  await screen.findByText("历史正文h2");
  await act(async () => finish(body()));
  expect(screen.queryByText("历史正文h")).toBeNull();
  expect(screen.getByText("历史正文h2")).toBeTruthy();
});
it("rejects changed database epoch instead of mixing metadata and body", async () => {
  const { api } = setup();
  vi.mocked(api.get).mockResolvedValue({ ...body(), epoch: "changed" });
  await screen.findByRole("button", { name: /版本 1/ });
  fireEvent.click(screen.getByRole("button", { name: /版本 1/ }));
  await screen.findByText("书库已变化，请刷新历史列表。");
  expect(screen.queryByRole("button", { name: "恢复此版本" })).toBeNull();
});
it("cancels a restore confirmation when the current chapter revision changes", async () => {
  const { api, view, perform, onPendingChange } = setup();
  await screen.findByRole("button", { name: /版本 1/ });
  fireEvent.click(screen.getByRole("button", { name: /版本 1/ }));
  await screen.findByText("历史正文h");
  fireEvent.click(screen.getByRole("button", { name: "恢复此版本" }));
  view.rerender(
    <HistoryPanel
      bookId="b"
      chapter={{ ...chapter, revision: 4 }}
      api={api}
      perform={perform}
      busy={false}
      onPendingChange={onPendingChange}
    />,
  );
  expect(screen.queryByRole("button", { name: "确认恢复" })).toBeNull();
  expect(api.restore).not.toHaveBeenCalled();
});
it("saves normalized names with label revision CAS and clears a name with null", async () => {
  const { api } = setup();
  await screen.findByRole("button", { name: /版本 1/ });
  fireEvent.click(screen.getByRole("button", { name: /版本 1/ }));
  await screen.findByText("历史正文h");
  fireEvent.change(screen.getByLabelText("版本名称"), {
    target: { value: "  首稿  " },
  });
  fireEvent.click(screen.getByRole("button", { name: "保存名称" }));
  await screen.findByDisplayValue("首稿");
  expect(api.label).toHaveBeenLastCalledWith(
    "b",
    "c",
    "h",
    "首稿",
    0,
    "epoch1",
  );
  fireEvent.change(screen.getByLabelText("版本名称"), {
    target: { value: "   " },
  });
  fireEvent.click(screen.getByRole("button", { name: "保存名称" }));
  await waitFor(() =>
    expect(api.label).toHaveBeenLastCalledWith(
      "b",
      "c",
      "h",
      null,
      1,
      "epoch1",
    ),
  );
});
it("refuses overlong Unicode names without dropping the edit", async () => {
  const { api } = setup();
  await screen.findByRole("button", { name: /版本 1/ });
  fireEvent.click(screen.getByRole("button", { name: /版本 1/ }));
  await screen.findByText("历史正文h");
  const name = "📖".repeat(121);
  fireEvent.change(screen.getByLabelText("版本名称"), {
    target: { value: name },
  });
  fireEvent.click(screen.getByRole("button", { name: "保存名称" }));
  expect(api.label).not.toHaveBeenCalled();
  expect(screen.getByDisplayValue(name)).toBeTruthy();
  expect(screen.getByRole("alert").textContent).toContain("120");
});
it("hides old book content immediately on a changed scope and ignores its late mutation", async () => {
  const { api, view, perform, onPendingChange } = setup();
  await screen.findByRole("button", { name: /版本 1/ });
  fireEvent.click(screen.getByRole("button", { name: /版本 1/ }));
  await screen.findByText("历史正文h");
  let finish!: (value: VersionSummary) => void;
  vi.mocked(api.label).mockImplementationOnce(
    () => new Promise((resolve) => (finish = resolve)),
  );
  fireEvent.change(screen.getByLabelText("版本名称"), {
    target: { value: "旧书名称" },
  });
  fireEvent.click(screen.getByRole("button", { name: "保存名称" }));
  vi.mocked(api.list).mockResolvedValue({
    items: [],
    next_cursor: null,
    epoch: "epoch2",
  });
  view.rerender(
    <HistoryPanel
      bookId="b2"
      chapter={{ ...chapter, id: "c2" }}
      api={api}
      perform={perform}
      busy={false}
      onPendingChange={onPendingChange}
    />,
  );
  expect(screen.queryByText("历史正文h")).toBeNull();
  await act(async () =>
    finish({ ...meta(), label: "旧书名称", label_revision: 1 }),
  );
  expect(screen.queryByDisplayValue("旧书名称")).toBeNull();
});
it("refreshes metadata during a pending body read without leaving the detail pane stuck loading", async () => {
  const { api } = setup();
  await screen.findByRole("button", { name: /版本 1/ });
  let finish!: (value: VersionDetail) => void;
  vi.mocked(api.get).mockImplementationOnce(
    () => new Promise((resolve) => (finish = resolve)),
  );
  fireEvent.click(screen.getByRole("button", { name: /版本 1/ }));
  await screen.findByText("读取选中版本…");
  fireEvent.click(screen.getByRole("button", { name: "刷新历史" }));
  await waitFor(() => expect(screen.queryByText("读取版本列表…")).toBeNull());
  expect(screen.queryByText("读取选中版本…")).toBeNull();
  await act(async () => finish(body()));
  expect(screen.queryByText("历史正文h")).toBeNull();
});
