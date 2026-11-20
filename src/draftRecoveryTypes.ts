export type DraftScope = { epoch: string; book_id: string; chapter_id: string };
export type DraftState = "matches_base" | "stale" | "already_saved";
export type DraftRecord = {
  token: string;
  base_revision: number;
  base_body: string;
  draft_body: string;
  state: DraftState;
};
export type DraftView = DraftScope & {
  current_revision: number;
  current_body: string;
  record: DraftRecord | null;
};
export type DraftSession = { session_id: string; epoch: string };
export type DraftCheckpoint = DraftScope & {
  session_id: string;
  sequence: number;
  stream_id: string;
  expected_token: string | null;
  base_revision: number;
  base_body: string;
  draft_body: string;
};
export interface DraftRecoveryAPI {
  start(): Promise<DraftSession>;
  read(scope: DraftScope): Promise<DraftView>;
  checkpoint(
    input: DraftCheckpoint,
  ): Promise<{ token: string; state: DraftState }>;
  dismiss?(
    input: DraftScope & { session_id: string; sequence: number; token: string },
  ): Promise<void>;
}
