import { invoke } from '@tauri-apps/api/core';
import { t } from './i18n';

/// Shared chat display preference: whether timestamps include seconds.
/// Loaded once per window; the settings page updates it live.

let showSeconds = false;
let loaded = false;

export async function loadChatDisplay(): Promise<void> {
  try {
    const settings = await invoke<{ show_seconds: boolean }>('v1_chat_display_settings');
    showSeconds = settings.show_seconds;
  } catch { showSeconds = false; }
  loaded = true;
}

export function isLoaded(): boolean { return loaded; }

export function setShowSeconds(value: boolean): void { showSeconds = value; }

export function getShowSeconds(): boolean { return showSeconds; }

/// Formats a unix-seconds stamp: HH:mm[:ss] today, otherwise with the date.
export function formatStamp(seconds: number): string {
  const date = new Date(seconds * 1000);
  const pad = (n: number) => `${n}`.padStart(2, '0');
  const time = getShowSeconds()
    ? `${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`
    : `${pad(date.getHours())}:${pad(date.getMinutes())}`;
  const now = new Date();
  const sameDay = date.getFullYear() === now.getFullYear() && date.getMonth() === now.getMonth() && date.getDate() === now.getDate();
  if (sameDay) return time;
  const sameYear = date.getFullYear() === now.getFullYear();
  const day = sameYear ? `${pad(date.getMonth() + 1)}-${pad(date.getDate())}` : `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
  return `${day} ${time}`;
}
