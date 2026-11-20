import React from "react";
import {
  render,
  screen,
  fireEvent,
  cleanup,
  waitFor,
} from "@testing-library/react";
import { it, expect, vi, afterEach } from "vitest";
import App from "../src/App";
import type { API } from "../src/types";
vi.mock("../src/MaterialsPanel", () => ({
  default: ({ api, bookId, onPendingChange }: any) => {
    React.useEffect(() => {
      void api.list(bookId, false);
    }, []);
    return (
      <section aria-label="资料测试面板">
        <button onClick={() => onPendingChange(true)}>模拟资料未保存</button>
        <button onClick={() => onPendingChange(false)}>模拟资料保存成功</button>
      </section>
    );
  },
}));
afterEach(cleanup);
function setup(fail = false) {
  const order: string[] = [];
  const api: API = {
    load: async () => ({
      books: [{ id: "b", title: "资料书", author: "", deleted_at: null }],
      volumes: [
        { id: "v", book_id: "b", title: "卷", sort_order: 0, deleted_at: null },
      ],
      chapters: [
        {
          id: "c",
          volume_id: "v",
          title: "章",
          body: "旧文",
          revision: 0,
          sort_order: 0,
          deleted_at: null,
        },
      ],
    }),
    apply: vi.fn(async () => {
      order.push("save");
      if (fail) throw Error("正文保存失败");
      return {
        books: [{ id: "b", title: "资料书", author: "", deleted_at: null }],
        volumes: [{ id: "v", book_id: "b", title: "卷", sort_order: 0, deleted_at: null }],
        chapters: [
          {
            id: "c",
            volume_id: "v",
            title: "章",
            body: "新文",
            revision: 1,
            sort_order: 0,
            deleted_at: null,
          },
        ],
      };
    }),
    backup: async () => null,
    restore: async () => null,
    exportBook: async () => null,
  };
  Object.assign(api, {
    materials: {
      list: vi.fn(async () => {
        order.push("list");
        return [];
      }),
    },
  });
  render(<App api={api} />);
  return { api, order };
}
it("flushes manuscript before opening materials and blocks unload while material changes remain", async () => {
  const { order } = setup();
  fireEvent.click(await screen.findByRole("button", { name: "打开 资料书" }));
  const editor = await screen.findByRole("textbox", { name: "章节正文" });
  fireEvent.change(editor, { target: { value: "新文" } });
  fireEvent.click(screen.getByRole("button", { name: "创作资料" }));
  await screen.findByLabelText("资料测试面板");
  await waitFor(() => expect(order.slice(0, 2)).toEqual(["save", "list"]));
  fireEvent.click(screen.getByText("模拟资料未保存"));
  const event = new Event("beforeunload", { cancelable: true });
  window.dispatchEvent(event);
  expect(event.defaultPrevented).toBe(true);
  fireEvent.click(screen.getByText("模拟资料保存成功"));
  const clean = new Event("beforeunload", { cancelable: true });
  window.dispatchEvent(clean);
  expect(clean.defaultPrevented).toBe(false);
});
it("keeps manuscript and does not open materials if flush fails", async () => {
  const { order } = setup(true);
  fireEvent.click(await screen.findByRole("button", { name: "打开 资料书" }));
  const editor = await screen.findByRole("textbox", { name: "章节正文" });
  fireEvent.change(editor, { target: { value: "新文" } });
  fireEvent.click(screen.getByRole("button", { name: "创作资料" }));
  await screen.findByText("正文保存失败");
  expect(order).toEqual(["save"]);
  expect(screen.queryByLabelText("资料测试面板")).toBeNull();
  expect((editor as HTMLTextAreaElement).value).toBe("新文");
});
