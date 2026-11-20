import "@testing-library/jest-dom/vitest";
import React from "react";
import {
  render,
  screen,
  fireEvent,
  waitFor,
  cleanup,
} from "@testing-library/react";
import { it, expect, vi, afterEach } from "vitest";
import App from "../src/App";
import type { API, LibraryState } from "../src/types";
afterEach(cleanup);
function mount(fail = false) {
  let state: LibraryState = {
    books: [{ id: "b", title: "书", author: "", deleted_at: null }],
    volumes: [
      { id: "v", book_id: "b", title: "卷", sort_order: 0, deleted_at: null },
    ],
    chapters: [
      {
        id: "c",
        volume_id: "v",
        title: "章",
        body: "甲🙂\r\n乙\r丙",
        revision: 0,
        sort_order: 0,
        deleted_at: null,
      },
    ],
  };
  const prepare = vi.fn(async (b, c, r, o, t) => ({
    id: "p",
    original_title: "章",
    new_title: t,
    before: "甲🙂\r\n",
    after: "乙\r丙",
    before_bytes: 9,
    after_bytes: 7,
    before_truncated: false,
    after_truncated: false,
    expires_at: Date.now() / 1000 + 600,
  }));
  const apply = vi.fn(async (cmd: any) => {
    if (fail) throw Error("保存失败");
    state = {
      ...state,
      chapters: state.chapters.map((c) =>
        c.id === cmd.id
          ? { ...c, body: cmd.body, revision: c.revision + 1 }
          : c,
      ),
    };
    return state;
  });
  const api: API = {
    load: async () => state,
    apply,
    exportBook: async () => null,
    backup: async () => null,
    restore: async () => null,
    split: {
      prepare,
      detail: async () => "",
      discard: async () => {},
      commit: async () => {
        state = {
          ...state,
          chapters: [
            {
              ...state.chapters[0],
              body: "甲🙂\r\n",
              revision: state.chapters[0].revision + 1,
            },
            {
              ...state.chapters[0],
              id: "n",
              body: "乙\r丙",
              revision: 0,
              sort_order: 1,
              title: "新章",
            },
          ],
        };
        return { book_id: "b", original_chapter_id: "c", new_chapter_id: "n" };
      },
    },
  };
  render(<App api={api} />);
  return { prepare, apply };
}
it("maps untouched CRLF cursor and uses new revision after split", async () => {
  const { prepare, apply } = mount();
  fireEvent.click(await screen.findByRole("button", { name: "打开 书" }));
  const text = (await screen.findByRole("textbox", {
    name: "章节正文",
  })) as HTMLTextAreaElement;
  expect(text.value).toBe("甲🙂\n乙\n丙");
  text.setSelectionRange(4, 4);
  fireEvent.click(screen.getByText("拆分章节"));
  await screen.findByRole("dialog", { name: "拆分章节" });
  fireEvent.click(screen.getByText("生成拆章预览"));
  await waitFor(() =>
    expect(prepare).toHaveBeenCalledWith("b", "c", 0, 5, "章（续）"),
  );
  fireEvent.click(await screen.findByText("拆分为两章"));
  fireEvent.click(screen.getByText("确认拆分"));
  await screen.findByText("拆章已完成");
  fireEvent.click(screen.getByLabelText("关闭拆章面板"));
  const next = screen.getByRole("textbox", { name: "章节正文" });
  fireEvent.change(next, { target: { value: "新后半" } });
  fireEvent.click(screen.getByRole("button", { name: "保存" }));
  await waitFor(() =>
    expect(apply).toHaveBeenCalledWith({
      type: "save_chapter",
      id: "n",
      body: "新后半",
      expected_revision: 0,
    }),
  );
});
for (const fail of [false, true])
  it("flushes dirty body before preparing, fail=" + fail, async () => {
    const { prepare } = mount(fail);
    fireEvent.click(await screen.findByRole("button", { name: "打开 书" }));
    const text = (await screen.findByRole("textbox", {
      name: "章节正文",
    })) as HTMLTextAreaElement;
    fireEvent.change(text, { target: { value: "新🙂后" } });
    text.setSelectionRange(3, 3);
    fireEvent.click(screen.getByText("拆分章节"));
    if (fail) {
      await screen.findByText("保存失败");
      expect(screen.queryByRole("dialog", { name: "拆分章节" })).toBeNull();
      expect(text.value).toBe("新🙂后");
    } else {
      await screen.findByRole("dialog", { name: "拆分章节" });
      fireEvent.click(screen.getByText("生成拆章预览"));
      await waitFor(() =>
        expect(prepare).toHaveBeenCalledWith("b", "c", 1, 3, "章（续）"),
      );
    }
  });
it("rejects selected text and IME composition without flushing", async () => {
  const { apply } = mount();
  fireEvent.click(await screen.findByRole("button", { name: "打开 书" }));
  const text = (await screen.findByRole("textbox", {
    name: "章节正文",
  })) as HTMLTextAreaElement;
  text.setSelectionRange(1, 3);
  fireEvent.click(screen.getByText("拆分章节"));
  await screen.findByText(/请取消文字选择/);
  text.setSelectionRange(3, 3);
  fireEvent.compositionStart(text);
  fireEvent.click(screen.getByText("拆分章节"));
  await screen.findByText(/请先完成输入法/);
  expect(apply).not.toHaveBeenCalled();
});
