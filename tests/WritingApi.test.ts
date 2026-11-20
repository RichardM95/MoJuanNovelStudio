import { expect, it, vi } from "vitest";
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(async () => null) }));
import { invoke } from "@tauri-apps/api/core";
import { api } from "../src/api";

it("reads scoped directory metadata and explicitly passes even an empty prior selection to the trusted writer", async () => {
  const writing = api.writing!;
  await writing.contextDirectory("b", "c");
  expect(invoke).toHaveBeenLastCalledWith("get_writing_context_directory", {
    bookId: "b",
    chapterId: "c",
  });
  await writing.configure("b", "c", 4, "active", "目标", ["m"], "e", []);
  expect(invoke).toHaveBeenLastCalledWith(
    "configure_writing_policy_with_context",
    {
      bookId: "b",
      chapterId: "c",
      expected: 4,
      mode: "active",
      goal: "目标",
      ids: ["m"],
      epoch: "e",
      contextChapterIds: [],
    },
  );
});

it("requests a coordinate-bounded trusted source window without reading the chapter API", async () => {
  await api.foreshadow!.sourceWindow!("book", "ch2", "epoch", 120, 140);
  expect(invoke).toHaveBeenLastCalledWith("get_foreshadow_source_window", {
    bookId: "book", chapterId: "ch2", epoch: "epoch", start: 120, end: 140,
  });
});

it("uses a dedicated desktop worker epoch command for source review", async () => {
  await api.foreshadow!.sourceEpoch!();
  expect(invoke).toHaveBeenLastCalledWith("get_foreshadow_source_epoch");
});

it("reads only the bound predecessor diagnosis via the dedicated worker bridge", async () => {
  const reader = api.writing!.predecessorDiagnosis!;
  await reader.epoch();
  expect(invoke).toHaveBeenLastCalledWith("get_foreshadow_source_epoch");
  await reader.read("book", "chapter", "pack", "epoch");
  expect(invoke).toHaveBeenLastCalledWith("diagnose_writing_predecessors", {
    bookId: "book", chapterId: "chapter", packId: "pack", epoch: "epoch",
  });
});
