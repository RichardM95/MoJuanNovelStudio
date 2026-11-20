import React from "react";
import {
  render,
  screen,
  fireEvent,
  waitFor,
  cleanup,
  act,
} from "@testing-library/react";
import { it, expect, vi, afterEach } from "vitest";
import ReviewPanel from "../src/ReviewPanel";
import type { WorkflowAPI, Proposal } from "../src/workflowTypes";
import type { Chapter } from "../src/types";
afterEach(cleanup);
const chapter: Chapter = {
  id: "c",
  volume_id: "v",
  title: "初见",
  body: "人工新稿",
  revision: 3,
  sort_order: 0,
  deleted_at: null,
};
const proposal: Proposal = {
  id: "p",
  chapter_id: "c",
  base_revision: 2,
  old_body: "旧稿",
  new_body: "建议稿",
  reason: "理顺场景",
  source: "agent",
  client_id: "agent",
  status: "pending",
  created_at: 1,
};
function setup(failed = false) {
  const api: WorkflowAPI = {
    versionBrowser: {
      list: vi.fn(async () => ({
        items: [
          {
            id: "h",
            chapter_id: "c",
            revision: 1,
            source: "gui",
            created_at: 1,
            label: null,
            label_revision: 0,
          },
        ],
        next_cursor: null,
        epoch: "e",
      })),
      get: vi.fn(async () => ({
        version: {
          id: "h",
          chapter_id: "c",
          revision: 1,
          source: "gui",
          created_at: 1,
          body: "历史正文",
        },
        label: null,
        label_revision: 0,
        epoch: "e",
      })),
      label: vi.fn(),
      restore: vi.fn(async () => ({ books: [], volumes: [], chapters: [] })),
    },
    listProposals: vi.fn(async () => [proposal]),
    decideProposal: vi.fn(async () => ({
      books: [],
      volumes: [],
      chapters: [],
    })),
    history: vi.fn(async () => [
      {
        id: "h",
        chapter_id: "c",
        revision: 1,
        body: "历史正文",
        source: "gui",
        created_at: 1,
      },
    ]),
    restoreVersion: vi.fn(async () => ({
      books: [],
      volumes: [],
      chapters: [],
    })),
  };
  const perform = vi.fn(async (fn: () => Promise<unknown>) => {
    if (failed) return false;
    await fn();
    return true;
  });
  const view = render(
    <ReviewPanel
      bookId="b"
      chapter={chapter}
      api={api}
      perform={perform}
      onClose={() => {}}
      busy={false}
    />,
  );
  return { api, perform, view };
}
it("shows immutable before/after and requires explicit acceptance confirmation", async () => {
  const { api } = setup();
  await screen.findByRole("button", { name: "采纳此提案" });
  expect(screen.getByText("旧稿")).toBeTruthy();
  expect(api.decideProposal).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "采纳此提案" }));
  expect(api.decideProposal).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "确认采纳" }));
  await waitFor(() =>
    expect(api.decideProposal).toHaveBeenCalledWith("p", true, 2),
  );
});
it("keeps confirmation when save/conflict handling fails", async () => {
  const { api } = setup(true);
  await screen.findByRole("button", { name: "采纳此提案" });
  fireEvent.click(screen.getByRole("button", { name: "采纳此提案" }));
  fireEvent.click(screen.getByRole("button", { name: "确认采纳" }));
  await waitFor(() =>
    expect(screen.getByRole("button", { name: "确认采纳" })).toBeTruthy(),
  );
  expect(api.decideProposal).not.toHaveBeenCalled();
});
it("restores old content as a new version using the current revision only after confirmation", async () => {
  const { api } = setup();
  fireEvent.click(screen.getByRole("button", { name: "章节历史" }));
  await screen.findByRole("button", { name: /版本 1/ });
  fireEvent.click(screen.getByRole("button", { name: /版本 1/ }));
  await screen.findByText("历史正文");
  fireEvent.click(screen.getByRole("button", { name: "恢复此版本" }));
  expect(api.versionBrowser!.restore).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "确认恢复" }));
  await waitFor(() =>
    expect(api.versionBrowser!.restore).toHaveBeenCalledWith(
      "b",
      "c",
      "h",
      3,
      "e",
    ),
  );
});

it("disables refresh while a concrete proposal is awaiting confirmation", async () => {
  setup();
  await screen.findByRole("button", { name: "采纳此提案" });
  fireEvent.click(screen.getByRole("button", { name: "采纳此提案" }));
  expect(
    (screen.getByRole("button", { name: "刷新" }) as HTMLButtonElement)
      .disabled,
  ).toBe(true);
});
it("invalidates confirmation when the current chapter revision changes", async () => {
  const { api, perform, view } = setup();
  await screen.findByRole("button", { name: "采纳此提案" });
  fireEvent.click(screen.getByRole("button", { name: "采纳此提案" }));
  view.rerender(
    <ReviewPanel
      bookId="b"
      chapter={{ ...chapter, revision: 4 }}
      api={api}
      perform={perform}
      onClose={() => {}}
      busy={false}
    />,
  );
  expect(screen.queryByRole("button", { name: "确认采纳" })).toBeNull();
});
it("does not fetch any history on proposal tab and never falls back to the legacy full-body method", async () => {
  const { api } = setup();
  await screen.findByRole("button", { name: "采纳此提案" });
  expect(api.history).not.toHaveBeenCalled();
  expect(api.versionBrowser!.list).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "章节历史" }));
  await screen.findByRole("button", { name: /版本 1/ });
  expect(api.versionBrowser!.get).not.toHaveBeenCalled();
  expect(api.history).not.toHaveBeenCalled();
});
it("blocks closing and switching tabs while a version name is unsaved", async () => {
  const { api, perform, view } = setup();
  const onClose = vi.fn(),
    pending = vi.fn();
  view.rerender(
    <ReviewPanel
      bookId="b"
      chapter={chapter}
      api={api}
      perform={perform}
      onClose={onClose}
      busy={false}
      onPendingChange={pending}
    />,
  );
  fireEvent.click(screen.getByRole("button", { name: "章节历史" }));
  await screen.findByRole("button", { name: /版本 1/ });
  fireEvent.click(screen.getByRole("button", { name: /版本 1/ }));
  await screen.findByText("历史正文");
  fireEvent.change(screen.getByLabelText("版本名称"), {
    target: { value: "未保存名称" },
  });
  fireEvent.click(screen.getByRole("button", { name: "关闭改稿面板" }));
  fireEvent.click(screen.getByRole("button", { name: "待采纳提案" }));
  expect(onClose).not.toHaveBeenCalled();
  expect(screen.getByDisplayValue("未保存名称")).toBeTruthy();
  expect(pending).toHaveBeenLastCalledWith(true);
});
it("keeps keyboard focus inside the modal and restores it when closed", async () => {
  const outside = document.createElement("button");
  document.body.appendChild(outside);
  outside.focus();
  const { view } = setup();
  await screen.findByRole("button", { name: "采纳此提案" });
  const dialog = screen.getByRole("dialog");
  expect(dialog.getAttribute("aria-modal")).toBe("true");
  outside.focus();
  expect(dialog.contains(document.activeElement)).toBe(true);
  const first = screen.getByRole("button", { name: "关闭改稿面板" });
  const last = screen.getByRole("button", { name: "采纳此提案" });
  last.focus();
  fireEvent.keyDown(last, { key: "Tab" });
  expect(document.activeElement).toBe(first);
  first.focus();
  fireEvent.keyDown(first, { key: "Tab", shiftKey: true });
  expect(document.activeElement).toBe(last);
  view.unmount();
  expect(document.activeElement).toBe(outside);
  outside.remove();
});
it("clears only the navigation warning after a pending version name is saved", async () => {
  const { api } = setup();
  vi.mocked(api.versionBrowser!.label).mockResolvedValue({
    id: "h",
    chapter_id: "c",
    revision: 1,
    source: "gui",
    created_at: 1,
    label: "定稿",
    label_revision: 1,
  });
  fireEvent.click(screen.getByRole("button", { name: "章节历史" }));
  await screen.findByRole("button", { name: /版本 1/ });
  fireEvent.click(screen.getByRole("button", { name: /版本 1/ }));
  await screen.findByText("历史正文");
  fireEvent.change(screen.getByLabelText("版本名称"), {
    target: { value: "定稿" },
  });
  fireEvent.click(screen.getByRole("button", { name: "待采纳提案" }));
  await screen.findByText("请先保存名称或取消编辑、恢复确认。");
  fireEvent.click(screen.getByRole("button", { name: "保存名称" }));
  await screen.findByText("名称已保存");
  await waitFor(() =>
    expect(screen.queryByText("请先保存名称或取消编辑、恢复确认。")).toBeNull(),
  );
});
