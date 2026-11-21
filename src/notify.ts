import { invoke, isTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import './notify.css';
import { t } from './i18n';

type Notice = { kind: 'direct' | 'group'; id: string; preview: string; ts: number; name?: string };

const card = document.querySelector<HTMLButtonElement>('#notice-card');
const avatar = document.querySelector<HTMLElement>('#notice-avatar');
const title = document.querySelector<HTMLElement>('#notice-title');
const preview = document.querySelector<HTMLElement>('#notice-preview');
const count = document.querySelector<HTMLElement>('#notice-count');

let current: Notice | null = null;
let pending = 0;
let hideTimer: number | null = null;

function paint(payload: Notice): void {
  current = payload;
  const name = payload.name || (payload.kind === 'group' ? t('群消息') : t('新的消息'));
  if (title) title.textContent = name;
  if (avatar) avatar.textContent = name.slice(0, 1).toUpperCase();
  if (preview) preview.textContent = payload.preview || t('发来一条消息');
  if (count) { count.hidden = pending < 2; count.textContent = pending > 1 ? String(pending) : ''; }
}

function scheduleHide(): void {
  if (hideTimer) window.clearTimeout(hideTimer);
  hideTimer = window.setTimeout(() => { void invoke('v1_hide_notice'); pending = 0; current = null; }, 6000);
}

if (isTauri()) {
  void listen<Notice>('qbird://notice', (event) => {
    pending += 1;
    const payload = { ...(event.payload as Notice), name: (event.payload as Notice & { name?: string }).name };
    paint(payload);
    scheduleHide();
  });
  card?.addEventListener('click', () => {
    if (current) void invoke('v1_open_notice', { payload: current });
    if (hideTimer) window.clearTimeout(hideTimer);
    pending = 0;
  });
}
