export type ImportMode = "headings" | "single";
export type ImportSourceHeading = {
  line: number;
  number: number | null;
  unit?: string | null;
};
export type ImportChapter = {
  index: number;
  title: string;
  bytes: number;
  snippet: string;
  source?: ImportSourceHeading;
};
export type ImportVolume = { title: string; chapters: ImportChapter[]; source?: ImportSourceHeading };
export type ImportPreview = {
  id: string;
  source_name: string;
  title: string;
  author: string;
  mode: ImportMode;
  expires_at: number;
  volumes: ImportVolume[];
  total_chapters: number;
  total_bytes: number;
};
export type ImportDetail = { index: number; body: string };
export type ImportOutcome = {
  book_id: string;
  volumes: number;
  chapters: number;
};
export interface ImportAPI {
  choose(mode: ImportMode): Promise<ImportPreview | null>;
  revise(
    id: string,
    title: string,
    author: string,
    mode: ImportMode,
  ): Promise<ImportPreview>;
  detail(id: string, index: number): Promise<ImportDetail>;
  commit(id: string): Promise<ImportOutcome>;
  discard(id: string): Promise<void>;
}
