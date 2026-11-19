import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, resolve } from 'node:path';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const read = (file) => readFileSync(resolve(root, file), 'utf8');
const frontend = JSON.parse(read('package.json')).version;
const tauri = JSON.parse(read('src-tauri/tauri.conf.json')).version;
const manifest = read('src-tauri/Cargo.toml').match(/^\[package\][\s\S]*?^version\s*=\s*"([^"]+)"/m)?.[1];
const lock = read('src-tauri/Cargo.lock').match(/^\[\[package\]\]\s*\r?\nname\s*=\s*"qbird"\s*\r?\nversion\s*=\s*"([^"]+)"/m)?.[1];
export function validateVersions(frontendVersion, otherVersions) {
  if (typeof frontendVersion !== 'string' || !/^0\.1\.(0|[1-9]\d*)$/.test(frontendVersion)) {
    throw new Error('测试阶段版本必须为 0.1.X，且 X 为非负整数');
  }
  if (otherVersions.some((version) => version !== frontendVersion)) {
    throw new Error('项目版本不一致：请同步前端、Tauri、Cargo 配置及 Cargo.lock');
  }
}
validateVersions(frontend, [tauri, manifest, lock]);
console.log(`Qbird version: ${frontend}`);
