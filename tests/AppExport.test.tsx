import React from "react";
import {
  render,
  screen,
  fireEvent,
  waitFor,
  cleanup,
} from "@testing-library/react";
import { it, expect, afterEach } from "vitest";
import App from "../src/App";
import type { API, LibraryState } from "../src/types";
afterEach(cleanup);
for (const fails of [false, true])
  it("flushes unsaved body before export, failure=" + fails, async () => {
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
          body: "旧",
          revision: 0,
          sort_order: 0,
          deleted_at: null,
        },
      ],
    };
    let saved = "";
    const api: API = {
      load: async () => state,
      apply: async (cmd: any) => {
        if (fails) throw Error("保存失败");
        saved = cmd.body;
        state = {
          ...state,
          chapters: [{ ...state.chapters[0], body: saved, revision: 1 }],
        };
        return state;
      },
      backup: async () => null,
      restore: async () => null,
      exportBook: async () => null,
      txtExport: {
        prepare: async () => {
          throw Error("unused");
        },
        save: async () => null,
        discard: async () => {},
      },
    };
    render(<App api={api} />);
    fireEvent.click(await screen.findByRole("button", { name: "打开 书" }));
    const text = await screen.findByRole("textbox", { name: "章节正文" });
    fireEvent.change(text, { target: { value: "新稿🙂" } });
    fireEvent.click(screen.getByRole("button", { name: "导出 TXT" }));
    if (fails) {
      await screen.findByText("保存失败");
      expect(screen.queryByRole("dialog", { name: "导出TXT" })).toBeNull();
      expect(text).toHaveProperty("value", "新稿🙂");
    } else {
      await screen.findByRole("dialog", { name: "导出TXT" });
      expect(saved).toBe("新稿🙂");
    }
  });
