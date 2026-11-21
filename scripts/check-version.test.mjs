import test from 'node:test';
import assert from 'node:assert/strict';
import { validateVersions } from './check-version.mjs';

test('accepts patch increments in the 0.1.X series, including prereleases', () => {
  for (const version of ['0.1.0', '0.1.1', '0.1.123', '0.1.1-beta']) {
    assert.doesNotThrow(() => validateVersions(version, [version, version, version]));
  }
});

test('rejects a new minor, major, invalid patch, and inconsistent files', () => {
  for (const version of ['0.2.0', '1.0.0', '0.1.01', '0.1.-1', '', null]) {
    assert.throws(() => validateVersions(version, [version]));
  }
  assert.throws(() => validateVersions('0.1.1', ['0.1.1', '0.1.0', '0.1.1']));
  assert.throws(() => validateVersions('0.1.1', ['0.1.1', undefined, '0.1.1']));
});
