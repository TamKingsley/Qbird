export type RecordsListState = "loading" | "error" | "empty" | "rows";

export function recordsListState(
  loading: boolean,
  error: string,
  visibleRows: number,
): RecordsListState {
  if (loading) return "loading";
  if (error) return "error";
  return visibleRows === 0 ? "empty" : "rows";
}
