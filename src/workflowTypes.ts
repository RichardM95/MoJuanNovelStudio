import type { HistoryAPI } from "./historyTypes";
import type { LibraryState } from "./types";
export type Proposal = {
  id: string;
  chapter_id: string;
  base_revision: number;
  old_body: string;
  new_body: string;
  reason: string;
  source: string;
  client_id: string;
  status: string;
  created_at: number;
};
export type ChapterVersion = {
  id: string;
  chapter_id: string;
  revision: number;
  body: string;
  source: string;
  created_at: number;
};
export interface WorkflowAPI {
  versionBrowser?: HistoryAPI;
  listProposals(bookId: string): Promise<Proposal[]>;
  decideProposal(
    id: string,
    approve: boolean,
    expectedRevision: number,
  ): Promise<LibraryState>;
  /** Legacy fixture compatibility only; production does not expose this. */
  history?(chapterId: string): Promise<ChapterVersion[]>;
  restoreVersion?(
    versionId: string,
    expectedRevision: number,
  ): Promise<LibraryState>;
}
export function errorMessage(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (error && typeof error === "object" && "message" in error)
    return String(error.message);
  return String(error);
}
