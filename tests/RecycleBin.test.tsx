import React from "react";
import {
  render,
  screen,
  fireEvent,
  cleanup,
  waitFor,
} from "@testing-library/react";
import { afterEach, describe, it, expect, vi } from "vitest";
import RecycleBin from "../src/RecycleBin";
import type { LibraryState } from "../src/types";
afterEach(cleanup);
const state: LibraryState = {
  books: [{ id: "b", title: "测试作品", author: "测试", deleted_at: 1 }],
  volumes: [
    { id: "v", book_id: "b", title: "测试卷", sort_order: 0, deleted_at: 1 },
  ],
  chapters: [
    {
      id: "c",
      volume_id: "v",
      title: "测试章",
      body: "仅测试",
      revision: 0,
      sort_order: 0,
      deleted_at: 1,
    },
  ],
};
describe("three-level recycle bin", () => {
  it("shows all three levels and blocks child restore until parents are restored", () => {
    const apply = vi.fn(async () => {});
    render(<RecycleBin data={state} busy={false} apply={apply} />);
    expect(
      screen
        .getByRole("button", { name: "恢复作品 测试作品" })
        .hasAttribute("disabled"),
    ).toBe(false);
    expect(
      screen
        .getByRole("button", { name: "恢复卷 测试卷" })
        .hasAttribute("disabled"),
    ).toBe(true);
    expect(
      screen
        .getByRole("button", { name: "恢复章节 测试章" })
        .hasAttribute("disabled"),
    ).toBe(true);
  });
  it("requires exact title before irreversible purge and cancel does not apply", () => {
    const apply = vi.fn(async () => {});
    render(<RecycleBin data={state} busy={false} apply={apply} />);
    fireEvent.click(
      screen.getByRole("button", { name: "永久删除章节 测试章" }),
    );
    const confirm = screen.getByRole("button", { name: "确认永久删除" });
    expect(confirm.hasAttribute("disabled")).toBe(true);
    fireEvent.change(screen.getByLabelText("输入名称确认"), {
      target: { value: "测试章" },
    });
    fireEvent.click(confirm);
    expect(apply).toHaveBeenCalledWith({ type: "purge_chapter", id: "c" });
  });
  it("canceling permanent deletion makes no mutation", () => {
    const apply = vi.fn(async () => {});
    render(<RecycleBin data={state} busy={false} apply={apply} />);
    fireEvent.click(
      screen.getByRole("button", { name: "永久删除作品 测试作品" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "取消" }));
    expect(apply).not.toHaveBeenCalled();
  });
  it("keeps the confirmation open when deletion fails", async () => {
    const apply = vi.fn(async () => false);
    render(<RecycleBin data={state} busy={false} apply={apply} />);
    fireEvent.click(
      screen.getByRole("button", { name: "永久删除章节 测试章" }),
    );
    fireEvent.change(screen.getByLabelText("输入名称确认"), {
      target: { value: "测试章" },
    });
    fireEvent.click(screen.getByRole("button", { name: "确认永久删除" }));
    await waitFor(() => expect(apply).toHaveBeenCalled());
    expect(screen.getByRole("button", { name: "确认永久删除" })).toBeTruthy();
  });
});
