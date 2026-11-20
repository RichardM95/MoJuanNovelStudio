export type ExportOptions = {
  scope: "book" | "volume" | "chapter";
  target_id: string | null;
  include_metadata: boolean;
  include_headings: boolean;
  newline: "lf" | "crlf";
  bom: boolean;
};
export type ExportPreview = {
  id: string;
  bytes: number;
  volumes: number;
  chapters: number;
  text: string;
  truncated: boolean;
  expires_at: number;
  bom: boolean;
  newline: string;
};
export interface ExportAPI {
  prepare(bookId: string, options: ExportOptions): Promise<ExportPreview>;
  save(id: string): Promise<string | null>;
  discard(id: string): Promise<void>;
}
