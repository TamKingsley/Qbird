import { invoke, isTauri } from '@tauri-apps/api/core';

type Entry = { url: string | null; next: number; queued: boolean; generation: number };
const entries = new Map<string, Entry>();
const queue: string[] = [];
const INTERVAL = 60_000;
let running = 0;

function paint(key: string): void {
  const url = entries.get(key)?.url;
  document.querySelectorAll<HTMLElement>('.message-avatar').forEach(node => {
    if (node.dataset.avatarKey !== key) return;
    node.replaceChildren();
    if (url) {
      const image = document.createElement('img');
      image.src = url;
      image.alt = '';
      node.append(image);
    } else {
      node.textContent = node.dataset.initial ?? '?';
    }
  });
}

function update(key: string, bytes: number[] | null): void {
  const entry = entries.get(key);
  if (!entry) return;
  if (entry.url) URL.revokeObjectURL(entry.url);
  entry.url = bytes?.length ? URL.createObjectURL(new Blob([new Uint8Array(bytes)], { type: 'image/png' })) : null;
  paint(key);
}

async function load(key: string, generation: number): Promise<void> {
  try {
    const command = key === 'self' ? 'v1_avatar' : 'v1_peer_avatar';
    const args = key === 'self' ? undefined : { peer: key };
    const cached = await invoke<number[] | null>(command, args);
    if (entries.get(key)?.generation !== generation) return;
    update(key, cached);
    if (key !== 'self' && await invoke<boolean>('v1_fetch_peer_avatar', { peer: key })) {
      const fresh = await invoke<number[] | null>('v1_peer_avatar', { peer: key });
      if (entries.get(key)?.generation === generation) update(key, fresh);
    }
  } catch { /* Legacy or offline peers leave their last cache untouched. */ }
}

function drain(): void {
  while (running < 2 && queue.length) {
    const key = queue.shift()!;
    const generation = entries.get(key)?.generation ?? 0;
    running++;
    void load(key, generation).finally(() => {
      const entry = entries.get(key);
      if (entry) entry.queued = false;
      running--;
      if (entry && entry.generation !== generation) schedule(key);
      drain();
    });
  }
}

function schedule(key: string): void {
  if (!isTauri()) return;
  let entry = entries.get(key);
  if (!entry) { entry = { url: null, next: 0, queued: false, generation: 0 }; entries.set(key, entry); }
  if (entry.queued || Date.now() < entry.next) return;
  entry.queued = true;
  entry.next = Date.now() + INTERVAL;
  queue.push(key);
  drain();
}

export function invalidatePeerAvatars(): void {
  for (const [key, entry] of entries) {
    if (key === 'self') continue;
    entry.generation++;
    entry.next = 0;
    update(key, null);
    if (document.querySelector(`.message-avatar[data-avatar-key="${CSS.escape(key)}"]`)) schedule(key);
  }
}

export function messageAvatar(key: string, name: string): HTMLElement {
  const avatar = document.createElement('span');
  avatar.className = 'message-avatar';
  avatar.dataset.avatarKey = key;
  avatar.dataset.initial = Array.from(name.trim())[0]?.toUpperCase() ?? '?';
  const url = entries.get(key)?.url;
  if (url) {
    const image = document.createElement('img'); image.src = url; image.alt = '';
    avatar.append(image);
  } else avatar.textContent = avatar.dataset.initial;
  schedule(key);
  return avatar;
}

export function groupedMessage(previous: { sender: string; created_at: number; recalled_at: number | null } | null,
  current: { sender: string; created_at: number; recalled_at: number | null }): boolean {
  const delta = previous ? current.created_at - previous.created_at : -1;
  return !!previous && !previous.recalled_at && !current.recalled_at
    && previous.sender === current.sender && delta >= 0 && delta < 90;
}

window.setInterval(() => {
  if (document.hidden) return;
  const keys = new Set<string>();
  document.querySelectorAll<HTMLElement>('.message-avatar').forEach(node => {
    if (node.dataset.avatarKey) keys.add(node.dataset.avatarKey);
  });
  keys.forEach(schedule);
}, INTERVAL);
