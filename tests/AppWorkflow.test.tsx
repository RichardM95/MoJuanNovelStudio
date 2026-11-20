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
function fixture(fail = false) {
  let state: LibraryState = {
    books: [{ id: "b", title: "测试书", author: "作者", deleted_at: null }],
    volumes: [
      {
        id: "v",
        book_id: "b",
        title: "第一卷",
        sort_order: 0,
        deleted_at: null,
      },
    ],
    chapters: [
      {
        id: "c",
        volume_id: "v",
        title: "第一章",
        body: "旧稿",
        revision: 0,
        sort_order: 0,
        deleted_at: null,
      },
    ],
  };
  const order: string[] = [];
  const api: API = {
    load: async () => structuredClone(state),
    apply: vi.fn(async (c) => {
      order.push("save");
      if (fail) throw Error("保存失败");
      state = {
        ...state,
        chapters: [
          {
            ...state.chapters[0],
            body: String(c.body),
            revision: state.chapters[0].revision + 1,
          },
        ],
      };
      return structuredClone(state);
    }),
    exportBook: async () => null,
    backup: async () => null,
    restore: async () => null,
    workflow: {
      listProposals: vi.fn(async () => {
        order.push("list");
        return [
          {
            id: "p",
            client_id: "x",
            chapter_id: "c",
            base_revision: 1,
            old_body: "先保存",
            new_body: "建议稿",
            reason: "测试",
            source: "x",
            status: "pending",
            created_at: 1,
          },
        ];
      }),
      history: async () => [],
      decideProposal: vi.fn(async () => {
        state = {
          ...state,
          chapters: [{ ...state.chapters[0], body: "建议稿", revision: 2 }],
        };
        return structuredClone(state);
      }),
      restoreVersion: async () => state,
    },
  };
  render(<App api={api} />);
  return { api, order };
}
it("flushes before review and resets save revision after acceptance", async () => {
  const { api, order } = fixture();
  fireEvent.click(await screen.findByRole("button", { name: "打开 测试书" }));
  const editor = await screen.findByRole("textbox", { name: "章节正文" });
  fireEvent.change(editor, { target: { value: "先保存" } });
  fireEvent.click(screen.getByRole("button", { name: "改稿与历史" }));
  await screen.findByRole("button", { name: "采纳此提案" });
  expect(order.slice(0, 2)).toEqual(["save", "list"]);
  fireEvent.click(screen.getByRole("button", { name: "采纳此提案" }));
  fireEvent.click(screen.getByRole("button", { name: "确认采纳" }));
  await waitFor(() =>
    expect((editor as HTMLTextAreaElement).value).toBe("建议稿"),
  );
  await waitFor(() =>
    expect(
      (
        screen.getByRole("button", {
          name: "关闭改稿面板",
        }) as HTMLButtonElement
      ).disabled,
    ).toBe(false),
  );
  fireEvent.click(screen.getByRole("button", { name: "关闭改稿面板" }));
  fireEvent.change(editor, { target: { value: "继续写作" } });
  fireEvent.click(screen.getByRole("button", { name: "返回书架" }));
  await screen.findByRole("heading", { name: "我的书架" });
  expect(api.apply).toHaveBeenLastCalledWith({
    type: "save_chapter",
    id: "c",
    body: "继续写作",
    expected_revision: 2,
  });
});
it("does not open review or read proposals if unsaved draft fails", async () => {
  const { api } = fixture(true);
  fireEvent.click(await screen.findByRole("button", { name: "打开 测试书" }));
  const editor = await screen.findByRole("textbox", { name: "章节正文" });
  fireEvent.change(editor, { target: { value: "不能丢失" } });
  fireEvent.click(screen.getByRole("button", { name: "改稿与历史" }));
  await screen.findByText("保存失败");
  expect(api.workflow!.listProposals).not.toHaveBeenCalled();
  expect(screen.queryByRole("dialog", { name: "改稿与历史" })).toBeNull();
  expect((editor as HTMLTextAreaElement).value).toBe("不能丢失");
});
