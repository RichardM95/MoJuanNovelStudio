import React from "react";
import {
  render,
  screen,
  fireEvent,
  waitFor,
  cleanup,
} from "@testing-library/react";
import { describe, it, expect, vi, afterEach } from "vitest";
import App from "../src/App";
import type { API, LibraryState } from "../src/types";
afterEach(cleanup);
const initial: LibraryState = {
  books: [{ id: "b", title: "远山来信", author: "林舟", deleted_at: null }],
  volumes: [
    { id: "v", book_id: "b", title: "归途", sort_order: 0, deleted_at: null },
    { id: "v2", book_id: "b", title: "远方", sort_order: 1, deleted_at: null },
  ],
  chapters: [
    {
      id: "c",
      volume_id: "v",
      title: "初见",
      body: "山风吹过。",
      revision: 0,
      sort_order: 0,
      deleted_at: null,
    },
  ],
};
function setup(fail = false) {
  let state = structuredClone(initial);
  const api: API = {
    load: async () => state,
    apply: vi.fn(async (c) => {
      if (c.type === "save_chapter") {
        if (fail) throw Error("磁盘空间不足");
        state = {
          ...state,
          chapters: state.chapters.map((x) => ({
            ...x,
            body: String(c.body),
            revision: x.revision + 1,
          })),
        };
      }
      if (c.type === "trash_chapter")
        state = {
          ...state,
          chapters: state.chapters.map((x) =>
            x.id === c.id ? { ...x, deleted_at: 1 } : x,
          ),
        };
      if (c.type === "trash_volume")
        state = {
          ...state,
          volumes: state.volumes.map((x) =>
            x.id === c.id ? { ...x, deleted_at: 1 } : x,
          ),
        };
      return state;
    }),
    exportBook: vi.fn(async () => "/book.txt"),
    backup: async () => "/backup.json",
    restore: async () => null,
  };
  render(<App api={api} />);
  return api;
}
describe("writing workspace", () => {
  it("opens a work and flushes draft before returning to shelf", async () => {
    const api = setup();
    fireEvent.click(
      await screen.findByRole("button", { name: "打开 远山来信" }),
    );
    const editor = await screen.findByRole("textbox", { name: "章节正文" });
    fireEvent.change(editor, { target: { value: "新的正文" } });
    fireEvent.click(screen.getByRole("button", { name: "返回书架" }));
    await screen.findByRole("heading", { name: "我的书架" });
    expect(api.apply).toHaveBeenCalledWith({
      type: "save_chapter",
      id: "c",
      body: "新的正文",
      expected_revision: 0,
    });
  });
  it("keeps unsaved manuscript and prevents leaving on write failure", async () => {
    setup(true);
    fireEvent.click(
      await screen.findByRole("button", { name: "打开 远山来信" }),
    );
    fireEvent.change(await screen.findByRole("textbox", { name: "章节正文" }), {
      target: { value: "不能丢" },
    });
    fireEvent.click(screen.getByRole("button", { name: "返回书架" }));
    await waitFor(() =>
      expect(screen.getByRole("alert").textContent).toContain("磁盘空间不足"),
    );
    expect(
      (screen.getByRole("textbox", { name: "章节正文" }) as HTMLTextAreaElement)
        .value,
    ).toBe("不能丢");
  });
  it("flushes the current draft before exporting TXT", async () => {
    const api = setup();
    fireEvent.click(
      await screen.findByRole("button", { name: "打开 远山来信" }),
    );
    fireEvent.change(await screen.findByRole("textbox", { name: "章节正文" }), {
      target: { value: "导出最新稿" },
    });
    fireEvent.click(screen.getByRole("button", { name: "导出 TXT" }));
    await waitFor(() => expect(api.exportBook).toHaveBeenCalledWith("b"));
    expect(vi.mocked(api.apply).mock.invocationCallOrder[0]).toBeLessThan(
      vi.mocked(api.exportBook).mock.invocationCallOrder[0],
    );
  });
  it("saves the draft before moving a chapter to another volume", async () => {
    const api = setup();
    fireEvent.click(
      await screen.findByRole("button", { name: "打开 远山来信" }),
    );
    fireEvent.change(await screen.findByRole("textbox", { name: "章节正文" }), {
      target: { value: "移动前的稿" },
    });
    fireEvent.change(screen.getByLabelText("移动章节到卷"), {
      target: { value: "v2" },
    });
    await waitFor(() =>
      expect(api.apply).toHaveBeenCalledWith({
        type: "transfer_chapter",
        id: "c",
        volume_id: "v2",
      }),
    );
    expect(vi.mocked(api.apply).mock.calls.map((x) => x[0].type)).toEqual([
      "save_chapter",
      "transfer_chapter",
    ]);
  });
  it("saves before recycling a chapter and shows it in the recycle bin", async () => {
    const api = setup();
    fireEvent.click(
      await screen.findByRole("button", { name: "打开 远山来信" }),
    );
    fireEvent.change(await screen.findByRole("textbox", { name: "章节正文" }), {
      target: { value: "删除前保存" },
    });
    fireEvent.click(screen.getByRole("button", { name: "删除章节 初见" }));
    fireEvent.click(screen.getByRole("button", { name: "确认" }));
    await waitFor(() =>
      expect(screen.queryByRole("textbox", { name: "章节正文" })).toBeNull(),
    );
    expect(vi.mocked(api.apply).mock.calls.map((x) => x[0].type)).toEqual([
      "save_chapter",
      "trash_chapter",
    ]);
    fireEvent.click(screen.getByRole("button", { name: "回收站" }));
    await screen.findByRole("button", { name: "恢复章节 初见" });
  });
  it("recycling the selected volume hides its chapter editor", async () => {
    setup();
    fireEvent.click(
      await screen.findByRole("button", { name: "打开 远山来信" }),
    );
    await screen.findByRole("textbox", { name: "章节正文" });
    fireEvent.click(screen.getByRole("button", { name: "删除卷 归途" }));
    fireEvent.click(screen.getByRole("button", { name: "确认" }));
    await waitFor(() =>
      expect(screen.queryByRole("textbox", { name: "章节正文" })).toBeNull(),
    );
    expect(screen.queryByRole("button", { name: "第1章 初见" })).toBeNull();
  });
});
