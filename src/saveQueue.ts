/** A failed reply is not proof that the transaction failed. */
export type SaveAttempt = Readonly<{
  revision: number;
  baseBody: string;
  body: string;
  generation: number;
  epoch: string | null;
}>;

export const SAVE_UNKNOWN = "保存结果未确认，稿件保留在编辑器";
export const SAVE_EXPLICIT = "请手动保存当前稿件，再继续其他操作";

export class SaveQueue {
  private generation = 0;
  private saved = 0;
  private body = "";
  private pending: Promise<void> | null = null;
  private unknown: SaveAttempt | null = null;
  private manual = false;
  private epoch: string | null = null;

  constructor(
    readonly id: string,
    private revision: number,
    private save: (
      id: string,
      body: string,
      revision: number,
    ) => Promise<number>,
    private baseBody = "",
  ) {}

  get baseline() {
    return { revision: this.revision, body: this.baseBody };
  }
  get currentRevision() {
    return this.revision;
  }
  get dirty() {
    return this.generation !== this.saved;
  }
  get unknownAttempt() {
    return this.unknown;
  }
  get manualSaveRequired() {
    return this.manual && this.dirty;
  }

  observeEpoch(epoch: unknown) {
    if (typeof epoch === "string" && epoch.length > 0) this.epoch = epoch;
  }

  editRecovered(body: string) {
    this.manual = true;
    this.edit(body);
  }

  edit(body: string) {
    this.body = body;
    this.generation++;
  }

  reconcile(attempt: SaveAttempt, stored: { revision: number; body: string }) {
    if (this.unknown !== attempt) return "mismatch";
    if (
      stored.revision === attempt.revision + 1 &&
      stored.body === attempt.body
    ) {
      this.revision = stored.revision;
      this.baseBody = stored.body;
      this.saved = attempt.generation;
      this.unknown = null;
      this.manual = this.dirty;
      return "committed";
    }
    if (
      stored.revision === attempt.revision &&
      stored.body === attempt.baseBody
    ) {
      // A generic bridge rejection cannot prove the request is terminal:
      // the original write could still arrive after this observation.
      return "base";
    }
    return "mismatch";
  }

  flush(explicit = false): Promise<void> {
    if (this.pending) return this.pending;
    if (this.unknown) return Promise.reject(Error(SAVE_UNKNOWN));
    if (this.manual && !explicit) return Promise.reject(Error(SAVE_EXPLICIT));
    if (!this.dirty) return Promise.resolve();

    const run = async () => {
      while (this.dirty) {
        const attempt: SaveAttempt = {
          generation: this.generation,
          body: this.body,
          baseBody: this.baseBody,
          revision: this.revision,
          epoch: this.epoch,
        };
        try {
          const revision = await this.save(
            this.id,
            attempt.body,
            attempt.revision,
          );
          if (
            !Number.isSafeInteger(revision) ||
            revision !== attempt.revision + 1
          )
            throw Error("保存应答中的版本不匹配");
          this.revision = revision;
          this.baseBody = attempt.body;
          this.saved = attempt.generation;
        } catch (error) {
          this.unknown = attempt;
          this.manual = true;
          throw error;
        }
        // After resolving an unknown result only one explicit attempt is
        // authorized. Later edits remain dirty instead of being sent in a loop.
        if (this.manual) {
          this.manual = this.dirty;
          return;
        }
      }
    };
    this.pending = run().finally(() => {
      this.pending = null;
    });
    return this.pending;
  }
}
