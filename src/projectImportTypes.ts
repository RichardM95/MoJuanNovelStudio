import type { ImportDetail, ImportOutcome, ImportVolume } from "./importTypes";

export type ProjectRoles = {
  manuscript: string | null;
  world: string | null;
  outlines: string | null;
  style: string | null;
};
export type ProjectSelection = {
  id: string;
  folder_name: string;
  expires_at: number;
  files: {
    name: string;
    bytes: number;
    kind: "txt" | "md";
    selectable: boolean;
  }[];
  suggested_roles: ProjectRoles;
};
export type ProjectOptions = {
  roles: ProjectRoles;
  title: string | null;
  author: string;
  omit_separators: boolean;
  automatic?: boolean;
  manuscript_override?: string | null;
};
type LineRange = { first: number; last: number };
export type ProjectAssociation = {
  number: number;
  volume_index: number | null;
  status: "same_title" | "different_title" | "missing_chapter" | "ambiguous";
  chapter_index: number | null;
  volume_number?: number;
};
export type ProjectCard = {
  title: string;
  kind:
    "original" | "volume_outline" | "chapter_outline" | "setting" | "character";
  file: string;
  first_line: number;
  last_line: number;
  association: ProjectAssociation | null;
  volume_index?: number;
};
export type ProjectPreview = {
  id: string;
  book_id: string;
  title: string;
  author: string;
  source_name: string;
  expires_at: number;
  volumes: ImportVolume[];
  total_chapters: number;
  separator_lines: LineRange[];
  omitted_separators: boolean;
  numbering: (LineRange & { volume: number; kind: "missing" | "duplicate" })[];
  cards: ProjectCard[];
  warnings: string[];
};
export type ProjectLink = { card_index: number; chapter_index: number };
export type ProjectOutcome = ImportOutcome & { cards: number };

export interface ProjectImportAPI {
  automatic?: boolean;
  choose(): Promise<ProjectSelection | null>;
  prepare(id: string, options: ProjectOptions): Promise<ProjectPreview>;
  detail(
    id: string,
    kind: "chapter" | "card",
    index: number,
  ): Promise<ImportDetail>;
  commit(
    id: string,
    links: ProjectLink[],
    excluded?: number[],
  ): Promise<ProjectOutcome>;
  discard(id: string): Promise<void>;
}
