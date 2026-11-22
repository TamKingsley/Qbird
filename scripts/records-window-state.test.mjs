import assert from "node:assert/strict";
import test from "node:test";
import { recordsListState } from "../src/records_window_state.ts";

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
