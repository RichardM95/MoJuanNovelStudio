export type ReplaceChapter = {
  id: string;
  title: string;
  revision: number;
  matches: number;
  before_snippet: string;
  after_snippet: string;
};
export type ReplacePreview = {
  id: string;
  book_id: string;
  query: string;
  replacement: string;
  expires_at: number;
  chapters: ReplaceChapter[];
  total_matches: number;
};
export type ReplaceDetail = {
  chapter_id: string;
  before: string;
  after: string;
};
export type ReplaceOutcome = { chapters: number; matches: number };
export interface ReplaceAPI {
  preview(
    bookId: string,
    query: string,
    replacement: string,
  ): Promise<ReplacePreview>;
  detail(id: string, chapterId: string): Promise<ReplaceDetail>;
  commit(id: string): Promise<ReplaceOutcome>;
  discard(id: string): Promise<void>;
}
