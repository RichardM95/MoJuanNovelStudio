import React from "react";
import { render, screen, fireEvent, cleanup } from "@testing-library/react";
import { afterEach, it, expect, vi } from "vitest";
import App from "../src/App";
import type { API, LibraryState } from "../src/types";
vi.mock("../src/TasksPanel", () => ({
  default: ({ api, bookId, onOpen }: any) => {
    React.useEffect(() => {
      void api.list(bookId);
    }, []);
    return (
      <section aria-label="检查任务面板">
        <button onClick={() => onOpen({ kind: "chapter", id: "other" })}>
          打开异书报告目标
        </button>
      </section>
    );
  },
}));
afterEach(cleanup);
function setup(fail = false) {
  const order: string[] = [];
  const data: LibraryState = {
    books: [{ id: "b", title: "书", author: "", deleted_at: null }],
    volumes: [
      { id: "v", book_id: "b", title: "卷", sort_order: 0, deleted_at: null },
    ],
    chapters: [
      {
        id: "c",
        volume_id: "v",
        title: "章",
        body: "旧",
        revision: 0,
        sort_order: 0,
        deleted_at: null,
      },
    ],
  };
  const api: API = {
    load: async () => structuredClone(data),
    apply: async () => {
      order.push("save");
      if (fail) throw Error("保存失败");
      data.chapters[0].body = "新";
      data.chapters[0].revision++;
      return structuredClone(data);
    },
    backup: async () => null,
    restore: async () => null,
    exportBook: async () => null,
  };
  Object.assign(api, {
    tasks: {
      list: async () => {
        order.push("list");
        return [];
      },
    },
  });
  render(<App api={api} />);
  return order;
}
it("flushes current chapter before opening tasks and guards report navigation", async () => {
  const order = setup();
  fireEvent.click(await screen.findByRole("button", { name: "打开 书" }));
  fireEvent.change(await screen.findByRole("textbox", { name: "章节正文" }), {
    target: { value: "新" },
  });
  fireEvent.click(screen.getByRole("button", { name: "检查任务" }));
  await screen.findByLabelText("检查任务面板");
  expect(order).toEqual(["save", "list"]);
  fireEvent.click(screen.getByRole("button", { name: "打开异书报告目标" }));
  expect(await screen.findByText("章节已不存在或不属于当前作品")).toBeTruthy();
  expect(
    (screen.getByRole("textbox", { name: "章节正文" }) as HTMLTextAreaElement)
      .value,
  ).toBe("新");
});
it("retains manuscript and does not open task actions after failed save", async () => {
  const order = setup(true);
  fireEvent.click(await screen.findByRole("button", { name: "打开 书" }));
  fireEvent.change(await screen.findByRole("textbox", { name: "章节正文" }), {
    target: { value: "新" },
  });
  fireEvent.click(screen.getByRole("button", { name: "检查任务" }));
  await screen.findByText("保存失败");
  expect(screen.queryByLabelText("检查任务面板")).toBeNull();
  expect(order).toEqual(["save"]);
});
