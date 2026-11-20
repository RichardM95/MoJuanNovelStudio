import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import ProjectImportPanel from "../src/ProjectImportPanel";
import type {
  ProjectImportAPI,
  ProjectPreview,
  ProjectSelection,
} from "../src/projectImportTypes";

afterEach(cleanup);

const expiresAt = Math.floor(Date.now() / 1000) + 600;
const selection: ProjectSelection = {
  id: "selection",
  folder_name: "边界抄本",
  expires_at: expiresAt,
  files: [
    { name: "边界抄本.txt", bytes: 84, kind: "txt", selectable: true },
    { name: "参考.txt", bytes: 17_000_000, kind: "txt", selectable: false },
    { name: "分章细纲.md", bytes: 40, kind: "md", selectable: true },
  ],
  suggested_roles: {
    manuscript: "边界抄本.txt",
    world: null,
    outlines: "分章细纲.md",
    style: null,
  },
};

const preview: ProjectPreview = {
  id: "preview",
  book_id: "reserved-book",
  title: "边界抄本",
  author: "",
  source_name: "边界抄本.txt",
  expires_at: expiresAt,
  total_chapters: 2,
  volumes: [
    {
      title: "开篇",
      source: { number: 1, line: 3 },
      chapters: [
        {
          index: 0,
          title: "起点",
          source: { number: 1, line: 5 },
          bytes: 3,
          snippet: "甲",
        },
        {
          index: 1,
          title: "转折",
          source: { number: 2, line: 8 },
          bytes: 3,
          snippet: "乙",
        },
      ],
    },
  ],
  separator_lines: [{ first: 2, last: 2 }],
  omitted_separators: true,
  numbering: [],
  warnings: [],
  cards: [
    {
      title: "第1章 起点",
      kind: "chapter_outline",
      file: "分章细纲.md",
      first_line: 2,
      last_line: 2,
      association: {
        number: 1,
        volume_index: 0,
        status: "same_title",
        chapter_index: 0,
      },
    },
    {
      title: "第2章 旧转折",
      kind: "chapter_outline",
      file: "分章细纲.md",
      first_line: 3,
      last_line: 3,
      association: {
        number: 2,
        volume_index: 0,
        status: "different_title",
        chapter_index: 1,
      },
    },
  ],
};

function fixture() {
  return {
    choose: vi.fn<ProjectImportAPI["choose"]>().mockResolvedValue(selection),
    prepare: vi.fn<ProjectImportAPI["prepare"]>().mockResolvedValue(preview),
    detail: vi
      .fn<ProjectImportAPI["detail"]>()
      .mockImplementation(async (_, kind) => ({
        index: 1,
        body: kind === "card" ? "旧计划" : "原稿正文",
      })),
    commit: vi.fn<ProjectImportAPI["commit"]>().mockResolvedValue({
      book_id: "reserved-book",
      volumes: 1,
      chapters: 2,
      cards: 2,
    }),
    discard: vi.fn<ProjectImportAPI["discard"]>().mockResolvedValue(undefined),
  };
}

async function showPreview(
  api: ProjectImportAPI,
  onImported = vi.fn(async (_bookId: string) => true),
) {
  render(
    <ProjectImportPanel
      api={api}
      onClose={vi.fn()}
      onPendingChange={vi.fn()}
      onImported={onImported}
    />,
  );
  fireEvent.click(screen.getByRole("button", { name: "选择作品文件夹" }));
  await screen.findByText("参考.txt（超出大小限制）");
  fireEvent.click(screen.getByRole("button", { name: "生成导入预览" }));
  await screen.findByText("同号异题");
  return onImported;
}

describe("作品文件夹导入", () => {
  it("自动整理无需预分配角色，预览可排除单条资料", async () => {
    const api = { ...fixture(), automatic: true };
    render(
      <ProjectImportPanel
        api={api}
        onClose={vi.fn()}
        onPendingChange={vi.fn()}
        onImported={vi.fn(async () => true)}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "选择作品文件夹" }));
    await screen.findByRole("checkbox", { name: "按内容自动整理" });
    fireEvent.click(screen.getByRole("button", { name: "生成导入预览" }));
    fireEvent.click(
      await screen.findByRole("checkbox", { name: "导入 第2章 旧转折" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "确认导入新作品" }));
    await waitFor(() =>
      expect(api.commit).toHaveBeenCalledWith(
        "preview",
        [{ card_index: 0, chapter_index: 0 }],
        [1],
      ),
    );
  });

  it("在同一预览中比较细纲和正文，按作者确认的关联提交", async () => {
    const api = fixture();
    const check = await showPreview(api);
    expect(api.prepare).toHaveBeenCalledWith(
      "selection",
      expect.objectContaining({ roles: selection.suggested_roles }),
    );
    fireEvent.click(screen.getByRole("button", { name: "第2章 旧转折" }));
    expect(
      screen.getByLabelText<HTMLSelectElement>("第2章 旧转折关联").value,
    ).toBe("");
    await screen.findByText("旧计划");
    await screen.findByText("原稿正文");
    fireEvent.change(screen.getByLabelText("第2章 旧转折关联"), {
      target: { value: "1" },
    });
    fireEvent.click(screen.getByRole("button", { name: "确认导入新作品" }));
    await waitFor(() => expect(check).toHaveBeenCalledWith("reserved-book"));
    expect(api.commit).toHaveBeenCalledWith("preview", [
      { card_index: 0, chapter_index: 0 },
      { card_index: 1, chapter_index: 1 },
    ]);
  });

  it("回执不明时只读取预分配作品，不重复提交", async () => {
    const api = fixture();
    api.commit.mockRejectedValue(new Error("回执不可用"));
    const check = vi.fn(async (_bookId: string) => true);
    check.mockRejectedValueOnce(new Error("暂时无法刷新书架"));
    await showPreview(api, check);
    fireEvent.click(screen.getByRole("button", { name: "确认导入新作品" }));
    const retry = await screen.findByRole("button", { name: "核对书架" });
    fireEvent.click(retry);
    await screen.findByText(/已导入 1 卷、2 章和 2 张资料卡/);
    expect(check.mock.calls).toEqual([["reserved-book"], ["reserved-book"]]);
    expect(api.commit).toHaveBeenCalledTimes(1);
  });
});
