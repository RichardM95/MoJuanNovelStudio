export type SearchHit = {
  kind: "chapter" | "material";
  id: string;
  title: string;
  snippet: string;
  revision: number;
};
export type SearchResults = { items: SearchHit[]; truncated: boolean };
export type BookSearch = (
  bookId: string,
  query: string,
  limit: number,
) => Promise<SearchResults>;
