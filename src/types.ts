import type { SplitAPI } from "./splitTypes";
import type { ExportAPI } from "./exportTypes";
import type { ImportAPI } from "./importTypes";
import type { ProjectImportAPI } from "./projectImportTypes";
import type { AgentAccessAPI } from "./AgentAccessPanel";
import type { ReplaceAPI } from "./replaceTypes";
import type { BackupAPI } from "./backupTypes";
import type { TasksAPI } from "./taskTypes";
import type { BookSearch } from "./searchTypes";
import type { MaterialsAPI } from "./materialTypes";
import type { WorkflowAPI } from "./workflowTypes";
import type { FanqieAPI } from "./fanqieTypes";
export type Book = {
  id: string;
  title: string;
  author: string;
  deleted_at: number | null;
};
export type Volume = {
  id: string;
  book_id: string;
  title: string;
  sort_order: number;
  deleted_at: number | null;
};
export type Chapter = {
  id: string;
  volume_id: string;
  title: string;
  body: string;
  sort_order: number;
  deleted_at: number | null;
  revision: number;
};
export type LibraryState = {
  books: Book[];
  volumes: Volume[];
  chapters: Chapter[];
};
export type Command = { type: string; [key: string]: unknown };
export interface API {
  draftRecovery?: import("./draftRecoveryTypes").DraftRecoveryAPI;
  foreshadow?: import("./foreshadowTypes").ForeshadowAPI;
  writing?: import("./writingTypes").WritingAPI;
  split?: SplitAPI;
  txtExport?: ExportAPI;
  txtImport?: ImportAPI;
  projectImport?: ProjectImportAPI;
  agentAccess?: AgentAccessAPI;
  replace?: ReplaceAPI;
  backups?: BackupAPI;
  tasks?: TasksAPI;
  materials?: MaterialsAPI;
  searchBook?: BookSearch;
  workflow?: WorkflowAPI;
  fanqie?: FanqieAPI;
  load(): Promise<LibraryState>;
  apply(command: Command): Promise<LibraryState>;
  exportBook(id: string): Promise<string | null>;
  backup(): Promise<string | null>;
  restore(): Promise<LibraryState | null>;
}
