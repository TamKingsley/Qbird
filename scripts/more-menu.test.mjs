import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { MORE_MENU_ENTRIES, moreMenuHtml } from "../src/more_menu.ts";

const stub = (key) => key;
const labels = (html) => [...html.matchAll(/role="menuitem">([^<]+)</g)].map((match) => match[1]);

test("more menu hides unfinished identity backup and restore placeholders", () => {
  const html = moreMenuHtml(stub);
  assert.equal(html.includes("menu.userBackupTodo"), false);
  assert.equal(html.includes("menu.userRestoreTodo"), false);
  assert.equal(labels(html).length, 4);
});

test("more menu keeps chat backup and chat restore entries visible in order", () => {
  assert.deepEqual(labels(moreMenuHtml(stub)), [
    "menu.settings",
    "menu.chatBackup",
    "menu.chatRestore",
    "menu.about",
  ]);
});

test("chat backup and chat restore entries open the privacy settings tab", () => {
  const html = moreMenuHtml(stub);
  const backup = html.indexOf('data-settings-tab="privacy" role="menuitem">menu.chatBackup');
  const restore = html.indexOf('data-settings-tab="privacy" role="menuitem">menu.chatRestore');
  assert.ok(backup >= 0, "chat backup entry should target the privacy tab");
  assert.ok(restore > backup, "chat restore should follow chat backup");
});

test("menu definition carries no unfinished placeholder entries", () => {
  for (const entry of MORE_MENU_ENTRIES) {
    assert.equal(/todo$/i.test(entry.labelKey), false, entry.labelKey);
  }
});

test("localized copy no longer defines the unfinished placeholders", () => {
  const source = readFileSync(new URL("../src/i18n.ts", import.meta.url), "utf8");
  assert.equal(source.includes("menu.userBackupTodo"), false);
  assert.equal(source.includes("menu.userRestoreTodo"), false);
});
