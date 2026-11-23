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

export type HistoryDateRange = { since: number | null; until: number | null; invalid: boolean };

function localDayBoundary(value: string, nextDay: boolean): number | null {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(value)) return null;
  const [year, month, day] = value.split("-").map(Number);
  if (year < 1 || year > 9999) return null;
  const date = new Date(0);
  date.setFullYear(year, month - 1, day);
  date.setHours(0, 0, 0, 0);
  if (date.getFullYear() !== year || date.getMonth() !== month - 1 || date.getDate() !== day) return null;
  if (nextDay) date.setDate(date.getDate() + 1);
  return Math.floor(date.getTime() / 1000) - (nextDay ? 1 : 0);
}

export function historyDateRange(from: string, to: string): HistoryDateRange {
  const since = from ? localDayBoundary(from, false) : null;
  const until = to ? localDayBoundary(to, true) : null;
  const invalid = (!!from && since === null) || (!!to && until === null)
    || (since !== null && until !== null && since > until);
  return { since: invalid ? null : since, until: invalid ? null : until, invalid };
}
