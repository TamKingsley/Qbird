import assert from "node:assert/strict";
import test from "node:test";
import { recordsListState } from "../src/records_window_state.ts";
import { historyDateRange } from "../src/records_window_state.ts";
import { loadUntilFound } from "../src/search_navigation.ts";

test("shows a loading state before asynchronous history queries finish", () => {
  assert.equal(recordsListState(true, "", 0), "loading");
});

test("shows a visible error state rather than an empty list after a failed query", () => {
  assert.equal(recordsListState(false, "history query failed", 0), "error");
});

test("shows an empty state only after a successful query with no visible rows", () => {
  assert.equal(recordsListState(false, "", 0), "empty");
});

test("shows record rows when the query returns matching messages", () => {
  assert.equal(recordsListState(false, "", 1), "rows");
});

test("search navigation continues past twenty pages to find older messages", async () => {
  let loaded = 0;
  let pages = 0;
  await loadUntilFound(
    () => loaded >= 21,
    () => pages < 25,
    () => loaded,
    async () => { pages += 1; loaded += 1; },
  );
  assert.equal(pages, 21);
});

test("search navigation stops when paging makes no progress", async () => {
  let pages = 0;
  await loadUntilFound(
    () => false,
    () => true,
    () => 10,
    async () => { pages += 1; },
  );
  assert.equal(pages, 1);
});

test("date filters include the complete selected local calendar day", () => {
  const range = historyDateRange("2026-11-23", "2026-11-23");
  assert.equal(range.invalid, false);
  assert.notEqual(range.since, null);
  assert.notEqual(range.until, null);
  const start = new Date(range.since * 1000);
  const nextDay = new Date((range.until + 1) * 1000);
  assert.deepEqual([start.getFullYear(), start.getMonth(), start.getDate(), start.getHours()], [2026, 10, 23, 0]);
  assert.deepEqual([nextDay.getFullYear(), nextDay.getMonth(), nextDay.getDate(), nextDay.getHours()], [2026, 10, 24, 0]);
});

test("date filters reject invalid calendar dates and reversed ranges", () => {
  assert.equal(historyDateRange("2026-02-30", "").invalid, true);
  assert.equal(historyDateRange("2026-11-24", "2026-11-23").invalid, true);
});
