export type TaskStatus =
  "queued" | "running" | "completed" | "cancelled" | "failed" | "interrupted";
export type Task = {
  id: string;
  book_id: string;
  kind: "context_check";
  status: TaskStatus;
  done: number;
  total: number;
  created_at: number;
  updated_at: number;
  started_at: number | null;
  finished_at: number | null;
  reason_code: string | null;
  error_message: string | null;
  retry_of: string | null;
};
export type TaskItem = {
  ordinal: number;
  kind: "chapter" | "material";
  item_id: string;
  title: string;
  revision: number;
  char_count: number | null;
  material_kind: string | null;
  source_status: string | null;
  payoff_status: string | null;
  tracking_status: string | null;
  needs_review: boolean;
};
export type TaskDetail = { task: Task; items: TaskItem[] };
export interface TasksAPI {
  start(bookId: string, key: string): Promise<Task>;
  list(bookId: string): Promise<Task[]>;
  get(bookId: string, id: string): Promise<TaskDetail>;
  cancel(bookId: string, id: string): Promise<Task>;
  resume(bookId: string, id: string): Promise<Task>;
  retry(bookId: string, id: string, key: string): Promise<Task>;
}
