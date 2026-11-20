import React from "react";
import {
  render,
  screen,
  fireEvent,
  waitFor,
  cleanup,
  act,
} from "@testing-library/react";
import { afterEach, it, expect, vi } from "vitest";
import SearchPanel from "../src/SearchPanel";
afterEach(cleanup);
const hit = {
  kind: "chapter" as const,
  id: "c",
  title: "归途",
  snippet: "<script>正文</script>",
  revision: 1,
};
it("searches the selected book and opens a literal text result", async () => {
  const search = vi.fn(async () => ({ items: [hit], truncated: true })),
    open = vi.fn();
  render(
    <SearchPanel bookId="b" search={search} onOpen={open} onClose={() => {}} />,
  );
  fireEvent.change(screen.getByRole("textbox", { name: "作品内关键词" }), {
    target: { value: "正文" },
  });
  fireEvent.click(screen.getByRole("button", { name: "搜索" }));
  await screen.findByText("<script>正文</script>");
  expect(document.querySelector("script")).toBeNull();
  expect(search).toHaveBeenCalledWith("b", "正文", 30);
  expect(screen.getByText(/仅显示前/)).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: /章节：归途/ }));
  expect(open).toHaveBeenCalledWith(hit);
});
it("discards a response after the query changes and validates empty input", async () => {
  let resolve: any;
  const search = vi.fn(() => new Promise<any>((r) => (resolve = r)));
  render(
    <SearchPanel
      bookId="b"
      search={search}
      onOpen={() => {}}
      onClose={() => {}}
    />,
  );
  fireEvent.click(screen.getByRole("button", { name: "搜索" }));
  expect(search).not.toHaveBeenCalled();
  fireEvent.change(screen.getByRole("textbox"), { target: { value: "旧" } });
  fireEvent.click(screen.getByRole("button", { name: "搜索" }));
  fireEvent.change(screen.getByRole("textbox"), { target: { value: "新" } });
  await act(async () => resolve({ items: [hit], truncated: false }));
  expect(screen.queryByText(hit.snippet)).toBeNull();
});
it("preserves query and reports failures, closes on escape", async () => {
  const close = vi.fn();
  render(
    <SearchPanel
      bookId="b"
      search={async () => {
        throw Error("搜索失败");
      }}
      onOpen={() => {}}
      onClose={close}
    />,
  );
  fireEvent.change(screen.getByRole("textbox"), { target: { value: "林溪" } });
  fireEvent.click(screen.getByRole("button", { name: "搜索" }));
  await screen.findByText("搜索失败");
  expect((screen.getByRole("textbox") as HTMLInputElement).value).toBe("林溪");
  fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
  expect(close).toHaveBeenCalledTimes(1);
});
