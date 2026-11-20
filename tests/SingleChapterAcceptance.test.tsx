import "./writingWebCrypto";
import "@testing-library/jest-dom/vitest";
import React from "react";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import f from "../qa/acceptance/single-chapter.json";
import App from "../src/App";
import type { ExportOptions } from "../src/exportTypes";
import type { Occurrence } from "../src/foreshadowTypes";
import type { VersionSummary } from "../src/historyTypes";
import type { Material } from "../src/materialTypes";
import type { API, Chapter, Command, LibraryState } from "../src/types";
import type { ChapterVersion } from "../src/workflowTypes";
import type {
  AuthorReview,
  AuthorReviewItem,
  WritingPack,
  WritingPolicy,
  WritingProposal,
} from "../src/writingTypes";

afterEach(cleanup);
const epoch = "fiction-library";
const book = { id: "clock-book", ...f.work, deleted_at: null };
const volume = {
  id: "clock-volume",
  book_id: book.id,
  ...f.volume,
  sort_order: 0,
  deleted_at: null,
};
const prior = {
  id: "clock-prior",
  volume_id: volume.id,
  ...f.prior,
  revision: 1,
  sort_order: 0,
  deleted_at: null,
};
const target = {
  id: "clock-target",
  volume_id: volume.id,
  title: f.target.title,
  body: "",
  revision: 0,
  sort_order: 1,
  deleted_at: null,
};
const policy: WritingPolicy = {
  book_id: book.id,
  chapter_id: target.id,
  revision: 0,
  mode: "active",
  goal: f.goal,
  updated_at: 1,
  required_material_ids: f.materials.map((c) => c.key),
  context_chapter_ids: [prior.id],
};
const materials: Material[] = f.materials.map((c) => ({
  id: c.key,
  book_id: book.id,
  kind: c.kind as Material["kind"],
  title: c.title,
  fields: Object.fromEntries(
    Object.entries(c.fields).filter(
      (entry): entry is [string, string] => typeof entry[1] === "string",
    ),
  ),
  volume_id: null,
  chapter_id: c.chapter === "target" ? target.id : null,
  source_chapter_id: c.source === "prior" ? prior.id : null,
  source_revision: c.source === "prior" ? 1 : null,
  source_status: c.source === "prior" ? "current" : "none",
  tracking_status: c.tracking_status ?? null,
  updated_at: 1,
  revision: 0,
  deleted_at: null,
}));
const occurrence: Occurrence = {
  id: "clock-clue",
  book_id: book.id,
  material_id: f.occurrence.material,
  start: f.occurrence.start,
  end: f.occurrence.end,
  quote: f.occurrence.quote,
  role: f.occurrence.role as Occurrence["role"],
  note: f.occurrence.note,
  source: {
    chapter_id: prior.id,
    chapter_title: prior.title,
    chapter_revision: prior.revision,
    chapter_sort_order: prior.sort_order,
    volume_id: volume.id,
    volume_title: volume.title,
    volume_sort_order: volume.sort_order,
  },
  revision: 0,
  deleted_at: null,
  created_at: 1,
};
const packId = "clock-pack";
const acceptedId = "二响·正常";
const rejectedId = "二响·违规";
const mockPath = "/mock/第二声钟.txt";
const editorText = (body: string) => body.replaceAll("\r\n", "\n");
const unused = async () => {
  throw Error("unexpected mock bridge call");
};
const directoryEntry = ({
  id,
  title,
  volume_id,
  revision,
  sort_order,
}: Chapter) => ({
  id,
  title,
  volume_id,
  revision,
  sort_order,
});
function reviewItems(candidate: typeof f.accepted): AuthorReviewItem[] {
  return candidate.reviews.map((r) => ({
    material_id: r.material,
    verdict: r.verdict as AuthorReviewItem["verdict"],
    note: r.note,
  }));
}
function proposal(id: string, candidate: typeof f.accepted): WritingProposal {
  return {
    id,
    book_id: book.id,
    chapter_id: target.id,
    client_id: "fiction-agent",
    base_revision: 1,
    live_epoch: epoch,
    decision: "pending",
    accepted_revision: null,
    input: {
      pack_id: packId,
      new_body: candidate.body,
      idempotency_key: candidate.request_key,
      conflicts: [],
      citations: f.citations.map((c) => ({
        entry_id:
          c.entry === "prior" ? `context:${prior.id}` : `material:${c.entry}`,
        field_key:
          c.field_key === "occurrence"
            ? `occurrence:${occurrence.id}`
            : c.field_key,
        start: c.start,
        end: c.end,
        explanation: c.explanation,
      })),
    },
  };
}

// Only the native API boundary is mocked. Candidate arrival, commits, raw
// responses and file picker return values are scripted; no IPC or file I/O.
function mockBridge(
  acceptFault?: "lost" | "different-raw-body" | "wrong-receipt",
) {
  let chapter: Chapter = { ...target };
  let pack: WritingPack | undefined;
  let exportCandidateId: string | undefined;
  const proposals = new Map<string, WritingProposal>();
  const versionId = (revision: number) => `clock-version-${revision}`;
  const version = (c: Chapter, source: string): ChapterVersion => ({
    id: versionId(c.revision),
    chapter_id: c.id,
    revision: c.revision,
    body: c.body,
    source,
    created_at: 1,
  });
  const history = [version(chapter, "baseline")];
  const library = (): LibraryState => ({
    books: [book],
    volumes: [volume],
    chapters: [prior, chapter],
  });
  const get = (id: string) => {
    const stored = proposals.get(id);
    if (!stored) throw Error(`unknown fixture proposal ${id}`);
    return stored;
  };
  const packSummary = () => ({
    id: packId,
    policy_revision: 0,
    chapter_revision: 1,
    expires_at: pack!.expires_at,
    archived: false,
  });
  const api = {
    load: vi.fn(async () => library()),
    apply: vi.fn(async (cmd: Command) => {
      const body =
        chapter.revision === 0 ? f.target.original_body : f.target.dirty_body;
      expect(cmd).toEqual({
        type: "save_chapter",
        id: target.id,
        body,
        expected_revision: chapter.revision,
      });
      chapter = { ...chapter, body, revision: chapter.revision + 1 };
      history.push(version(chapter, "gui"));
      return library();
    }),
    exportBook: unused,
    backup: unused,
    restore: unused,
    workflow: {
      listProposals: vi.fn(async (b: string) => {
        expect(b).toBe(book.id);
        return [];
      }),
      decideProposal: unused,
      versionBrowser: {
        list: vi.fn(
          async (
            b: string,
            c: string,
            cursor: string | null,
            limit: number,
            marked: boolean,
          ) => {
            expect([b, c, cursor, limit, marked]).toEqual([
              book.id,
              target.id,
              null,
              50,
              false,
            ]);
            return {
              epoch,
              next_cursor: null,
              items: [...history]
                .reverse()
                .map(({ body: _body, ...metadata }): VersionSummary => ({
                  ...metadata,
                  label: null,
                  label_revision: 0,
                })),
            };
          },
        ),
        get: vi.fn(async (b: string, c: string, id: string) => {
          expect([b, c]).toEqual([book.id, target.id]);
          const row = history.find((v) => v.id === id);
          if (!row) throw Error(`unknown fixture version ${id}`);
          return { version: row, label: null, label_revision: 0, epoch };
        }),
        label: unused,
        restore: vi.fn(
          async (b: string, c: string, id: string, cr: number, e: string) => {
            expect([b, c, cr, e]).toEqual([
              book.id,
              target.id,
              chapter.revision,
              epoch,
            ]);
            const old = history.find((row) => row.id === id);
            if (!old) throw Error(`unknown fixture version ${id}`);
            chapter = {
              ...chapter,
              revision: chapter.revision + 1,
              body: old.body,
            };
            history.push(version(chapter, `history:${id}`));
            return library();
          },
        ),
      },
    },
    materials: {
      list: vi.fn(async () => materials),
      get: unused,
      create: unused,
      save: unused,
      trash: unused,
      restore: unused,
    },
    writing: {
      epoch: vi.fn(async () => epoch),
      state: vi.fn(async (b: string, c: string) => {
        expect([b, c]).toEqual([book.id, target.id]);
        return {
          epoch,
          policy,
          packs: pack ? [packSummary()] : [],
          proposals: Array.from(proposals.values(), (p) => ({
            id: p.id,
            pack_id: p.input.pack_id,
            decision: p.decision,
            archived: false,
            created_at: 1,
          })),
        };
      }),
      contextDirectory: vi.fn(async (b: string, c: string) => ({
        epoch,
        book_id: b,
        chapter_id: c,
        chapters: [directoryEntry(prior)],
        volumes: [volume],
      })),
      configure: unused,
      prepare: vi.fn(
        async (b: string, c: string, pr: number, cr: number, e: string) => {
          expect([b, c, pr, cr, e, chapter.revision, chapter.body]).toEqual([
            book.id,
            target.id,
            0,
            1,
            epoch,
            1,
            f.target.original_body,
          ]);
          pack = {
            id: packId,
            book_id: book.id,
            chapter_id: target.id,
            originating_epoch: epoch,
            handle_version: 4,
            created_at: Date.now() / 1000,
            expires_at: Date.now() / 1000 + 1800,
            policy,
            book,
            volume,
            chapter,
            materials,
            context_chapters: [{ chapter: prior, volume }],
            foreshadow_occurrences: [occurrence],
          };
          return {
            ...packSummary(),
            book_id: b,
            chapter_id: c,
            goal: f.goal,
            entries: [],
          };
        },
      ),
      pack: vi.fn(async (b: string, id: string) => {
        expect([b, id]).toEqual([book.id, packId]);
        if (!pack) throw Error("fixture pack was not frozen");
        return pack;
      }),
      proposal: vi.fn(async (b: string, id: string) => {
        expect(b).toBe(book.id);
        return get(id);
      }),
      saveReview: vi.fn(
        async (
          b: string,
          id: string,
          expected: number | null,
          e: string,
          items: AuthorReviewItem[],
        ) => {
          expect([b, expected, e]).toEqual([book.id, null, epoch]);
          const p = get(id);
          expect(p.decision).toBe("pending");
          const review: AuthorReview = {
            proposal_id: id,
            book_id: b,
            chapter_id: target.id,
            pack_id: packId,
            originating_epoch: e,
            policy_revision: 0,
            chapter_revision: 1,
            revision: 1,
            items,
          };
          proposals.set(id, { ...p, author_review: review });
          return review;
        },
      ),
      decide: vi.fn(
        async (
          b: string,
          id: string,
          approve: boolean,
          pr: number,
          cr: number,
          e: string,
          ack: boolean,
        ) => {
          expect([b, id, approve, pr, cr, e, ack]).toEqual([
            book.id,
            rejectedId,
            false,
            0,
            1,
            epoch,
            false,
          ]);
          const rejected = { ...get(id), decision: "rejected" };
          proposals.set(id, rejected);
          return rejected;
        },
      ),
      acceptReviewed: vi.fn(
        async (
          b: string,
          id: string,
          pr: number,
          cr: number,
          e: string,
          rr: number,
          ack: boolean,
        ) => {
          expect([b, id, pr, cr, e, rr, ack]).toEqual([
            book.id,
            acceptedId,
            0,
            1,
            epoch,
            1,
            false,
          ]);
          const accepted = {
            ...get(id),
            decision: "accepted",
            accepted_revision: 2,
          };
          if (acceptFault === "wrong-receipt") {
            // Neither the receipt nor the full reread may authenticate the
            // original author action: its frozen input was changed.
            const wrong = {
              ...accepted,
              input: { ...accepted.input, new_body: f.rejected.body },
            };
            proposals.set(id, wrong);
            return wrong;
          }
          proposals.set(id, accepted);
          chapter = {
            ...chapter,
            revision: 2,
            body:
              acceptFault === "different-raw-body"
                ? editorText(accepted.input.new_body)
                : accepted.input.new_body,
          };
          history.push(version(chapter, `strict:${id}`));
          if (acceptFault === "lost") throw Error("虚构桥：决定回执未送达");
          return accepted;
        },
      ),
    },
    txtExport: {
      prepare: vi.fn(async (b: string, options: ExportOptions) => {
        expect([b, options.target_id, chapter.revision, chapter.body]).toEqual([
          book.id,
          target.id,
          2,
          f.accepted.body,
        ]);
        const id = "clock-export";
        exportCandidateId = id;
        return {
          id,
          text: f.export.visible_text,
          bytes: 157,
          volumes: 1,
          chapters: 1,
          truncated: false,
          expires_at: Date.now() / 1000 + 600,
          bom: true,
          newline: "crlf",
        };
      }),
      save: vi.fn(async (id: string) => {
        expect(id).toBe(exportCandidateId);
        exportCandidateId = undefined;
        return mockPath;
      }),
      discard: vi.fn(async () => {
        exportCandidateId = undefined;
      }),
    },
  } satisfies API;
  return {
    api,
    receiveAgentCandidates() {
      expect(pack?.chapter.body).toBe(f.target.original_body);
      proposals.set(acceptedId, proposal(acceptedId, f.accepted));
      proposals.set(rejectedId, proposal(rejectedId, f.rejected));
    },
  };
}

const editor = () => screen.getByRole("textbox", { name: "章节正文" });
const button = (name: string) => screen.getByRole("button", { name });
const click = (name: string) => fireEvent.click(button(name));
const openBook = async () => {
  fireEvent.click(
    await screen.findByRole("button", { name: `打开 ${book.title}` }),
  );
  const chapterButton = await screen.findByRole("button", {
    name: `第2章 ${target.title}`,
  });
  fireEvent.click(chapterButton);
  await screen.findByRole("button", { name: `删除章节 ${target.title}` });
};
async function start(bridge: ReturnType<typeof mockBridge>) {
  render(<App api={bridge.api} />);
  await openBook();
  expect(editor()).toHaveValue("");
  fireEvent.change(editor(), { target: { value: f.target.original_body } });
  click("保存");
  await screen.findByText("已保存");
  expect(editor()).toHaveValue(f.target.original_body);
  click("写作约束");
  await screen.findByRole("dialog", { name: "章节写作约束" });
  fireEvent.click(
    await screen.findByRole("button", { name: "冻结当前依据包" }),
  );
  await screen.findByRole("button", { name: /^查看依据 clock-pack/ });
  bridge.receiveAgentCandidates();
  click("刷新依据与建议");
  await screen.findByRole("button", { name: `查看建议 ${acceptedId}` });
}
async function pickAndReview(id: string, candidate: typeof f.accepted) {
  click(`查看建议 ${id}`);
  const before = await screen.findByRole("heading", { name: "冻结原稿" });
  const after = screen.getByRole("heading", { name: "建议新稿" });
  expect(before.parentElement?.querySelector("pre")?.textContent).toBe(
    f.target.original_body,
  );
  expect(after.parentElement?.querySelector("pre")?.textContent).toBe(
    candidate.body,
  );
  expect(
    screen.getByText("未自报冲突，不等于系统验证无冲突。"),
  ).toBeInTheDocument();
  expect(button("采纳建议")).toBeDisabled();
  for (const item of candidate.reviews) {
    const title = f.materials.find((c) => c.key === item.material)!.title;
    const select = screen.getByLabelText(`作者判断：${title}`);
    const card = select.closest("details")!;
    if (!card.open) fireEvent.click(card.querySelector("summary")!);
    fireEvent.change(select, { target: { value: item.verdict } });
    fireEvent.change(screen.getByLabelText(`作者备注：${title}`), {
      target: { value: item.note },
    });
  }
  click("保存逐项审阅");
  await screen.findByText("已保存逐项作者审阅 · 版本 1");
}

it("walks the original chapter through save, author rejection and adoption, readback, reopen and TXT export controls", async () => {
  const bridge = mockBridge();
  const { api } = bridge;
  await start(bridge);
  expect(api.apply).toHaveBeenCalledOnce();
  expect(api.writing.prepare).toHaveBeenCalledExactlyOnceWith(
    book.id,
    target.id,
    0,
    1,
    epoch,
  );
  await pickAndReview(rejectedId, f.rejected);
  expect(api.writing.saveReview).toHaveBeenNthCalledWith(
    1,
    book.id,
    rejectedId,
    null,
    epoch,
    reviewItems(f.rejected),
  );
  expect(button("采纳建议")).toBeDisabled();
  click("拒绝建议");
  expect(api.writing.decide).not.toHaveBeenCalled();
  const readsBeforeRejection = api.load.mock.calls.length;
  click("确认拒绝");
  await screen.findByText(/已拒绝.*未按此决定刷新或更改正文/);
  expect(editor()).toHaveValue(f.target.original_body);
  expect(api.load).toHaveBeenCalledTimes(readsBeforeRejection);

  await pickAndReview(acceptedId, f.accepted);
  expect(api.writing.saveReview).toHaveBeenNthCalledWith(
    2,
    book.id,
    acceptedId,
    null,
    epoch,
    reviewItems(f.accepted),
  );
  expect(
    screen.getByRole("heading", { name: "冻结前文（补充上下文）" }),
  ).toBeInTheDocument();
  const clueTitle = f.materials.find(
    (c) => c.key === f.occurrence.material,
  )!.title;
  const clue = screen.getByText(new RegExp(`^${clueTitle} · 版本 0`));
  if (!clue.closest("details")!.open) fireEvent.click(clue);
  expect(
    screen.getByRole("heading", { name: "冻结伏笔出现 · 铺设" }),
  ).toBeInTheDocument();
  for (const citation of f.citations) {
    expect(
      screen
        .getByText(citation.explanation)
        .parentElement?.querySelector("blockquote")?.textContent,
    ).toBe(citation.quote);
  }
  expect(editor()).toHaveValue(f.target.original_body);
  click("采纳建议");
  expect(
    screen.getByRole("alertdialog", { name: "确认写作操作" }),
  ).toHaveTextContent("逐项作者审阅版本 1");
  expect(api.writing.acceptReviewed).not.toHaveBeenCalled();
  const confirm = button("确认采纳");
  act(() => {
    fireEvent.click(confirm);
    fireEvent.click(confirm);
  });
  await screen.findByText(/当前正文已核对/);
  expect(editor()).toHaveValue(editorText(f.accepted.body));

  // This is UI navigation in the same App, not a native process restart.
  click("关闭写作约束");
  click("返回书架");
  await openBook();
  expect(editor()).toHaveValue(editorText(f.accepted.body));
  click("写作约束");
  await screen.findByText("accepted");
  expect(screen.getByText("rejected")).toBeInTheDocument();
  click(`查看建议 ${acceptedId}`);
  const history = await screen.findByRole("heading", { name: "建议新稿" });
  expect(history.parentElement?.querySelector("pre")?.textContent).toBe(
    f.accepted.body,
  );
  expect(button("采纳建议")).toBeDisabled();
  expect(button("拒绝建议")).toBeDisabled();
  click("关闭写作约束");
  click("导出 TXT");
  const exportDialog = await screen.findByRole("dialog", { name: "导出TXT" });
  const exporting = within(exportDialog);
  fireEvent.change(exporting.getByLabelText("导出范围"), {
    target: { value: "chapter" },
  });
  fireEvent.change(exporting.getByLabelText("选择卷章"), {
    target: { value: target.id },
  });
  for (const label of ["书名和作者", "卷章标题", "UTF-8 BOM"])
    fireEvent.click(exporting.getByLabelText(label));
  click("生成预览");
  expect((await exporting.findByLabelText("导出内容预览")).textContent).toBe(
    f.export.visible_text,
  );
  expect(exporting.getByText(/157字节.*UTF-8 BOM.*CRLF/)).toBeInTheDocument();
  expect(api.txtExport.prepare).toHaveBeenCalledExactlyOnceWith(book.id, {
    scope: "chapter",
    target_id: target.id,
    include_metadata: false,
    include_headings: false,
    bom: true,
    newline: "crlf",
  });
  click("保存TXT");
  await exporting.findByText(`TXT已导出：${mockPath}`);
  expect(api.txtExport.save).toHaveBeenCalledExactlyOnceWith("clock-export");
  expect(api.writing.decide).toHaveBeenCalledExactlyOnceWith(
    book.id,
    rejectedId,
    false,
    0,
    1,
    epoch,
    false,
  );
  expect(api.writing.acceptReviewed).toHaveBeenCalledExactlyOnceWith(
    book.id,
    acceptedId,
    0,
    1,
    epoch,
    1,
    false,
  );
  expect(api.writing.saveReview).toHaveBeenCalledTimes(2);
  expect(api.apply).toHaveBeenCalledOnce();
});

it("stops read-only recovery of a lost decision receipt and retains the story author's unsaved draft", async () => {
  const bridge = mockBridge("lost");
  const { api } = bridge;
  await start(bridge);
  await pickAndReview(acceptedId, f.accepted);
  click("采纳建议");
  let release!: (value: string) => void;
  const oldEpoch = new Promise<string>((resolve) => {
    release = resolve;
  });
  const loads = api.load.mock.calls.length;
  api.writing.epoch.mockImplementationOnce(() => oldEpoch);
  click("确认采纳");
  fireEvent.click(await screen.findByRole("button", { name: "停止等待核对" }));
  expect(screen.getByText(/不表示原写入失败/)).toBeInTheDocument();
  expect(screen.queryByText(/当前正文已核对/)).not.toBeInTheDocument();
  expect(api.load).toHaveBeenCalledTimes(loads);
  click("关闭写作约束");
  expect(editor()).toHaveValue(f.target.original_body);
  // Suspend the independent autosave debounce, so any extra write would come
  // from recovery itself. This is not an IME integration test.
  fireEvent.compositionStart(editor());
  fireEvent.change(editor(), { target: { value: f.target.dirty_body } });
  fireEvent.click(screen.getByText("待核对的正文决定"));
  click(`核对正文决定 · ${book.title} / ${target.title}`);
  const dialog = await screen.findByRole("dialog", { name: "章节写作约束" });
  expect(api.apply).toHaveBeenCalledOnce();
  fireEvent.click(
    within(dialog).getByRole("button", { name: `只读核对建议 ${acceptedId}` }),
  );
  await screen.findByText(/当前正文已核对/);
  click("关闭写作约束");
  const readsAfterRecovery = api.load.mock.calls.length;
  const proposalReads = api.writing.proposal.mock.calls.length;
  await act(async () => release(epoch));
  expect(api.load).toHaveBeenCalledTimes(readsAfterRecovery);
  expect(api.writing.proposal).toHaveBeenCalledTimes(proposalReads);
  expect(editor()).toHaveValue(f.target.dirty_body);
  expect(api.apply).toHaveBeenCalledOnce();
  expect(api.writing.acceptReviewed).toHaveBeenCalledOnce();
  expect(api.writing.saveReview).toHaveBeenCalledOnce();
  expect(api.writing.decide).not.toHaveBeenCalled();
});

it.each([
  ["different-raw-body", /同版本正文不匹配/],
  ["wrong-receipt", /仍待核对.*不匹配/],
] as const)(
  "does not publish candidate text for %s",
  async (fault, diagnostic) => {
    const { api, ...controls } = mockBridge(fault);
    await start({ api, ...controls });
    await pickAndReview(acceptedId, f.accepted);
    click("采纳建议");
    const proposalReads = api.writing.proposal.mock.calls.length;
    const bodyReads = api.load.mock.calls.length;
    click("确认采纳");
    await screen.findByText(diagnostic);
    expect(screen.queryByText(/当前正文已核对/)).not.toBeInTheDocument();
    expect(api.writing.proposal.mock.calls.length).toBeGreaterThan(
      proposalReads,
    );
    if (fault === "wrong-receipt") {
      expect(
        screen.queryByText(/精确终态证据：已采纳/),
      ).not.toBeInTheDocument();
      expect(api.load).toHaveBeenCalledTimes(bodyReads);
      expect(button("采纳建议")).toBeDisabled();
    } else {
      expect(screen.getByText(/精确终态证据：已采纳/)).toBeInTheDocument();
      expect(api.load).toHaveBeenCalledTimes(bodyReads + 1);
    }
    click("关闭写作约束");
    expect(editor()).toHaveValue(f.target.original_body);
    expect(api.writing.acceptReviewed).toHaveBeenCalledOnce();
    expect(api.apply).toHaveBeenCalledOnce();
    expect(api.writing.decide).not.toHaveBeenCalled();
    expect(api.writing.saveReview).toHaveBeenCalledOnce();
  },
);

async function startHistory(bridge: ReturnType<typeof mockBridge>) {
  await start(bridge);
  await pickAndReview(acceptedId, f.accepted);
  click("采纳建议");
  click("确认采纳");
  await screen.findByText(/当前正文已核对/);
  click("关闭写作约束");
  expect(editor()).toHaveValue(editorText(f.accepted.body));
}

const historyDialog = () => screen.findByRole("dialog", { name: "改稿与历史" });
const selectHistory = async (revision: number) => {
  const list = within(await screen.findByLabelText("历史版本列表"));
  fireEvent.click(
    await list.findByRole("button", {
      name: new RegExp(`^版本 ${revision} 版本 ${revision} ·`),
    }),
  );
  return screen.findByRole("heading", { name: "历史稿件" });
};

it("lets the original author decline a history rollback, then restores only the confirmed scoped version", async () => {
  const bridge = mockBridge();
  const history = bridge.api.workflow.versionBrowser;
  await startHistory(bridge);
  click("改稿与历史");
  const dialog = await historyDialog();
  fireEvent.click(within(dialog).getByRole("button", { name: "章节历史" }));

  const adopted = await selectHistory(2);
  expect(adopted.parentElement?.querySelector("pre")?.textContent).toBe(
    f.accepted.body,
  );
  expect(history.get).toHaveBeenLastCalledWith(
    book.id,
    target.id,
    "clock-version-2",
  );

  const original = await selectHistory(1);
  expect(original.parentElement?.querySelector("pre")?.textContent).toBe(
    f.target.original_body,
  );
  expect(
    screen
      .getByRole("heading", { name: "当前稿件 · 版本 2" })
      .parentElement?.querySelector("pre")?.textContent,
  ).toBe(f.accepted.body);
  expect(history.get).toHaveBeenLastCalledWith(
    book.id,
    target.id,
    "clock-version-1",
  );
  expect(history.restore).not.toHaveBeenCalled();

  click("恢复此版本");
  expect(
    screen.getByRole("alertdialog", { name: "确认恢复历史版本" }),
  ).toHaveTextContent("恢复为当前章节的新版本");
  expect(history.restore).not.toHaveBeenCalled();
  click("取消恢复");
  expect(
    screen.queryByRole("alertdialog", { name: "确认恢复历史版本" }),
  ).toBeNull();
  expect(history.restore).not.toHaveBeenCalled();
  expect(editor()).toHaveValue(editorText(f.accepted.body));

  click("恢复此版本");
  click("确认恢复");
  await screen.findByText("已恢复为新的章节版本");
  expect(history.restore).toHaveBeenCalledExactlyOnceWith(
    book.id,
    target.id,
    "clock-version-1",
    2,
    epoch,
  );
  expect(editor()).toHaveValue(f.target.original_body);
  // New revision and old accepted revision both remain browsable. Exact raw
  // CRLF assertions are on the rendered detail, not the normalized textarea.
  const restored = await selectHistory(3);
  expect(restored.parentElement?.querySelector("pre")?.textContent).toBe(
    f.target.original_body,
  );
  expect(
    screen.getByText(/历史版本 3 · 来源 history:clock-version-1/),
  ).toBeInTheDocument();
  const stillAdopted = await selectHistory(2);
  expect(stillAdopted.parentElement?.querySelector("pre")?.textContent).toBe(
    f.accepted.body,
  );
  expect(
    screen.getByText(
      `历史版本 2 · 来源 strict:${acceptedId}。恢复会生成新版本，不抹去历史。`,
    ),
  ).toBeInTheDocument();
  click("关闭改稿面板");
  expect(editor()).toHaveValue(f.target.original_body);
  expect(bridge.api.apply).toHaveBeenCalledOnce();
});

it("blocks the original author's history navigation until the uncertain draft save is verified", async () => {
  const bridge = mockBridge();
  const api: typeof bridge.api & Pick<API, "foreshadow"> = bridge.api;
  api.foreshadow = {
    sourceEpoch: async () => epoch,
    chapter: async (_book: string, chapter: string) => {
      const c = (await api.load()).chapters.find((row) => row.id === chapter)!;
      return {
        position: {
          chapter_id: c.id,
          chapter_revision: c.revision,
          chapter_title: c.title,
          chapter_sort_order: c.sort_order,
          volume_id: c.volume_id,
          volume_title: volume.title,
          volume_sort_order: volume.sort_order,
        },
        body: c.body,
      };
    },
  } as unknown as API["foreshadow"];
  const history = api.workflow.versionBrowser;
  await startHistory(bridge);
  // Suspend default autosave so the history action itself must flush. This
  // is testing navigation, not native composition or physical device input.
  fireEvent.compositionStart(editor());
  fireEvent.change(editor(), { target: { value: f.target.dirty_body } });
  expect(editor()).toHaveValue(f.target.dirty_body);
  const apply = api.apply.getMockImplementation()!;
  api.apply.mockImplementationOnce(async (cmd) => {
    await apply(cmd);
    throw Error("虚构桥：已写入但无法收到回执");
  });
  click("改稿与历史");
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "虚构桥：已写入但无法收到回执",
  );
  const dirtySave = {
    type: "save_chapter",
    id: target.id,
    body: f.target.dirty_body,
    expected_revision: 2,
  };
  expect(api.apply).toHaveBeenLastCalledWith(dirtySave);
  expect(screen.queryByRole("dialog", { name: "改稿与历史" })).toBeNull();
  expect(history.list).not.toHaveBeenCalled();
  expect(history.get).not.toHaveBeenCalled();
  expect(history.restore).not.toHaveBeenCalled();
  expect(editor()).toHaveValue(f.target.dirty_body);
  const leaveDirty = new Event("beforeunload", { cancelable: true });
  window.dispatchEvent(leaveDirty);
  expect(leaveDirty.defaultPrevented).toBe(true);

  // Navigation must not reissue the uncertain write. Hold explicit readback
  // before allowing history; the original write, not a replay, made v3.
  click("改稿与历史");
  await act(async () => {});
  expect(api.apply).toHaveBeenCalledTimes(2);
  const load = api.load.getMockImplementation()!;
  let release!: () => void;
  const waitForReadback = new Promise<void>((resolve) => {
    release = resolve;
  });
  api.load.mockImplementationOnce(async () => {
    await waitForReadback;
    return load();
  });
  click("核对已存正文");
  await screen.findByRole("button", { name: "停止等待核对" });
  expect(screen.queryByRole("dialog", { name: "改稿与历史" })).toBeNull();
  expect(history.list).not.toHaveBeenCalled();
  expect(history.get).not.toHaveBeenCalled();
  expect(history.restore).not.toHaveBeenCalled();
  expect(editor()).toHaveValue(f.target.dirty_body);
  await act(async () => release());
  await screen.findByText("核对到这份正文已保存。");
  expect(api.apply).toHaveBeenCalledTimes(2);
  click("改稿与历史");
  const dialog = await historyDialog();
  fireEvent.click(within(dialog).getByRole("button", { name: "章节历史" }));
  await selectHistory(1);
  expect(
    screen
      .getByRole("heading", { name: "当前稿件 · 版本 3" })
      .parentElement?.querySelector("pre")?.textContent,
  ).toBe(f.target.dirty_body);
  expect(history.restore).not.toHaveBeenCalled();
  click("关闭改稿面板");
  expect(editor()).toHaveValue(f.target.dirty_body);
  const leaveSaved = new Event("beforeunload", { cancelable: true });
  window.dispatchEvent(leaveSaved);
  expect(leaveSaved.defaultPrevented).toBe(false);
});
