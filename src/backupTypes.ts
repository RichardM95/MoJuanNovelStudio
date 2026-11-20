export type RestoreSummary = {
  format: string;
  books: number;
  volumes: number;
  chapters: number;
  history: number;
  materials: number;
  material_history?: number;
  version_labels: number;
  canonical_bytes: number;
  writing_policies?:number;writing_packs?:number;writing_proposals?:number;foreshadow_proposals?:number;
};
export type BackupResult = {
  output_path: string | null;
  recovery_path: string | null;
  summary: RestoreSummary | null;
};
export type BackupJob = {
  id: string;
  kind: "export_sqlite" | "restore";
  status:
    | "running"
    | "prepared"
    | "committing"
    | "done"
    | "cancelled"
    | "failed"
    | "expired";
  progress: { phase: string; completed: number; total: number | null };
  cancel_requested: boolean;
  created_at: number;
  updated_at: number;
  expires_at: number | null;
  source_name: string | null;
  preview: RestoreSummary | null;
  confirmation_token: string | null;
  result: BackupResult | null;
  error_code: string | null;
  error_message: string | null;
};
export interface BackupAPI {
  current(): Promise<BackupJob | null>;
  startExport(): Promise<BackupJob | null>;
  startPrepare(): Promise<BackupJob | null>;
  status(id: string): Promise<BackupJob>;
  cancel(id: string): Promise<BackupJob>;
  confirm(id: string, token: string, writingAck?:boolean): Promise<BackupJob>;
  exportJson(): Promise<string | null>;
}
