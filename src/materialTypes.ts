import type {MaterialHistoryAPI} from "./materialHistoryTypes";
export type MaterialKind =
  "setting" | "character" | "outline" | "summary" | "timeline" | "foreshadow" | "document";
export type Material = {
  content_review_required?: boolean;
  id: string;
  book_id: string;
  kind: MaterialKind;
  title: string;
  fields: Record<string, string>;
  volume_id: string | null;
  chapter_id: string | null;
  source_chapter_id?: string | null;
  source_revision?: number | null;
  source_status?: "none" | "current" | "stale" | "unavailable";
  story_order?: number;
  story_time?: string | null;
  tracking_status?: string | null;
  payoff_chapter_id?: string | null;
  payoff_revision?: number | null;
  payoff_status?: "none" | "current" | "stale" | "unavailable";
  revision: number;
  deleted_at: number | null;
  updated_at: number;
};
export type MaterialSummary = Omit<Material, "fields">;
export type CreateMaterialInput = Pick<
  Material,
  | "book_id"
  | "kind"
  | "title"
  | "fields"
  | "volume_id"
  | "chapter_id"
  | "source_chapter_id"
  | "source_revision"
  | "story_order"
  | "story_time"
  | "tracking_status"
  | "payoff_chapter_id"
  | "payoff_revision"
>;
export type SaveMaterialInput = Pick<
  Material,
  | "id"
  | "title"
  | "fields"
  | "volume_id"
  | "chapter_id"
  | "source_chapter_id"
  | "source_revision"
  | "story_order"
  | "story_time"
  | "tracking_status"
  | "payoff_chapter_id"
  | "payoff_revision"
> & { expected_revision: number };
export type MaterialProposal = {
  id: string;
  client_id: string;
  material_id: string;
  base_revision: number;
  old_snapshot: Material;
  new_title: string;
  new_fields: Record<string, string>;
  new_source_revision: number | null;
  reason: string;
  status: string;
  created_at: number;
};
export interface MaterialsAPI {
  organize?: {
    preview(bookId: string): Promise<{
      id: string;
      source_count: number;
      cards: {
        title: string;
        kind: MaterialKind;
        source_files: string;
        content: string;
        previous_fields: Record<string, string> | null;
      }[];
    }>;
    apply(id: string, accepted: number[]): Promise<number>;
  };
  history?: MaterialHistoryAPI;
  review?: {
    list(bookId: string): Promise<MaterialProposal[]>;
    decide(
      id: string,
      approve: boolean,
      expectedRevision: number,
    ): Promise<Material>;
  };
  list(bookId: string, includeDeleted: boolean): Promise<MaterialSummary[]>;
  get(id: string, includeDeleted: boolean): Promise<Material>;
  create(input: CreateMaterialInput): Promise<Material>;
  save(input: SaveMaterialInput): Promise<Material>;
  trash(id: string, expectedRevision: number): Promise<Material>;
  restore(id: string, expectedRevision: number): Promise<Material>;
}
