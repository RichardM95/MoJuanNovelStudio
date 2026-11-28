export type FanqieChapter = {
  chapter_id: string;
  volume_id: string;
  volume_title: string;
  volume_ordinal: number;
  title: string;
  revision: number;
  ordinal: number;
};
export type FanqieDirectory = {
  book_id: string;
  book_title: string;
  epoch: string;
  chapters: FanqieChapter[];
};
export type FanqiePacket = FanqieChapter & { body: string };
export interface FanqieAPI {
  directory(bookId: string): Promise<FanqieDirectory>;
  packet(
    bookId: string,
    epoch: string,
    expected: FanqieChapter,
  ): Promise<FanqiePacket>;
}
