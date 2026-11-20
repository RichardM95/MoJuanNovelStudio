import type { ChapterVersion } from "./workflowTypes";
import type { LibraryState } from "./types";
export type VersionSummary = {
  id: string;
  chapter_id: string;
  revision: number;
  source: string;
  created_at: number;
  label: string | null;
  label_revision: number;
};
export type VersionPage = {
  items: VersionSummary[];
  next_cursor: string | null;
  epoch: string;
};
export type VersionDetail = {
  version: ChapterVersion;
  label: string | null;
  label_revision: number;
  epoch: string;
};
export interface HistoryAPI {
  list(
    bookId: string,
    chapterId: string,
    cursor: string | null,
    limit: number,
    markedOnly: boolean,
  ): Promise<VersionPage>;
  get(bookId: string, chapterId: string, id: string): Promise<VersionDetail>;
  label(
    bookId: string,
    chapterId: string,
    id: string,
    label: string | null,
    expectedLabelRevision: number,
    expectedEpoch: string,
  ): Promise<VersionSummary>;
  restore(
    bookId: string,
    chapterId: string,
    id: string,
    expectedRevision: number,
    expectedEpoch: string,
  ): Promise<LibraryState>;
}
