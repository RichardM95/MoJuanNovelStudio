import React from "react";
import { render, screen, fireEvent, cleanup } from "@testing-library/react";
import { afterEach, it, expect, vi } from "vitest";
import App from "../src/App";
import type { API, LibraryState } from "../src/types";
afterEach(cleanup);
it("saves chapter before search and opens chapter result", async () => {
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
      {
        id: "d",
        volume_id: "v",
        title: "后章",
        body: "后章正文",
        revision: 0,
        sort_order: 1,
        deleted_at: null,
      },
    ],
  };
  const order: string[] = [];
  const api: API = {
    load: async () => structuredClone(data),
    apply: async () => {
      order.push("save");
      data.chapters[0].body = "新";
      data.chapters[0].revision++;
      return structuredClone(data);
    },
    backup: async () => null,
    restore: async () => null,
    exportBook: async () => null,
  };
  Object.assign(api, {
    searchBook: async () => {
      order.push("search");
      return {
        items: [
          {
            kind: "chapter",
            id: "d",
            title: "后章",
            snippet: "后章正文",
            revision: 0,
          },
        ],
        truncated: false,
      };
    },
  });
  render(<App api={api} />);
  fireEvent.click(await screen.findByRole("button", { name: "打开 书" }));
  fireEvent.change(await screen.findByRole("textbox", { name: "章节正文" }), {
    target: { value: "新" },
  });
  fireEvent.click(screen.getByRole("button", { name: "书内搜索" }));
  fireEvent.change(
    await screen.findByRole("textbox", { name: "作品内关键词" }),
    { target: { value: "后章" } },
  );
  fireEvent.click(screen.getByRole("button", { name: "搜索" }));
  fireEvent.click(await screen.findByRole("button", { name: /章节：后章/ }));
  expect(
    (
      (await screen.findByRole("textbox", {
        name: "章节正文",
      })) as HTMLTextAreaElement
    ).value,
  ).toBe("后章正文");
  expect(order).toEqual(["save", "search"]);
});
