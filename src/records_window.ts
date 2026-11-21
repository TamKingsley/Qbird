import './style.css';
import { invoke } from '@tauri-apps/api/core';
import { t } from './i18n';
import { loadChatDisplay, getShowSeconds } from './chat_prefs';

/// Standalone chat-records window served from records.html. The conversation
/// arrives via the query string (?kind=&id=). Offers search, type
/// filters, single-select and multi-select deletion of local messages.

type RecRow = {
  id: string;
  outgoing: boolean;
  senderLabel: string;
  body: string;
  createdAt: number;
  isFile: boolean;
};

type Contact = { fingerprint: string; nickname: string; username: string | null };
type GroupSummary = { id: string; name: string };

let kind = 'self';
let peerId = '';
let title = t('聊天记录');
let filter = '';
let typeFilter: 'all' | 'file' = 'all';
const selected = new Set<string>();
let rows: RecRow[] = [];

function escapeText(value: string): string {
  return value.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;').replace(/'/g, '&#39;');
}

function formatTime(seconds: number): string {
  const date = new Date(seconds * 1000);
  const pad = (n: number) => `${n}`.padStart(2, '0');
  const base = `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
  const time = getShowSeconds() ? `${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}` : `${pad(date.getHours())}:${pad(date.getMinutes())}`;
  return `${base} ${time}`;
}

function parseConversation(): boolean {
  const params = new URLSearchParams(window.location.search);
  kind = params.get('kind') ?? 'self';
  peerId = params.get('id') ?? '';
  return ['self', 'direct', 'group'].includes(kind);
}

function fail(message: string): void {
  const app = document.getElementById('app');
  if (app) app.innerHTML = `<p class="records-empty">${escapeText(message)}</p>`;
}

async function resolveTitle(): Promise<void> {
  try {
    if (kind === 'self') { title = t('自己的聊天记录'); return; }
    if (kind === 'direct') {
      const contacts = await invoke<Contact[]>('v1_list_contacts');
      const hit = contacts.find((c) => c.fingerprint === peerId);
      title = `${t('与「')}${hit ? hit.nickname : peerId.slice(0, 8)}${t('」的聊天记录')}`;
      return;
    }
    const groups = await invoke<GroupSummary[]>('v1_list_groups');
    const hit = groups.find((g) => g.id === peerId);
    title = `${t('与「')}${hit ? hit.name : peerId.slice(0, 8)}${t('」的聊天记录')}`;
  } catch { title = t('聊天记录'); }
}

async function loadRows(): Promise<void> {
  rows = [];
  if (kind === 'self') {
    type SelfMsg = { id: string; sender: string; body: string; created_at: number };
    const batch = await invoke<SelfMsg[]>('v1_list_self_messages', { limit: 200, offset: 0 });
    rows = batch.map((m) => ({ id: m.id, outgoing: true, senderLabel: t('我'), body: m.body, createdAt: m.created_at, isFile: false }));
  } else if (kind === 'direct') {
    type DirectMsg = { id: string; body: string; direction: string; created_at: number };
    const batch = await invoke<DirectMsg[]>('v1_list_messages', { peer: peerId, limit: 200, offset: 0 });
    rows = batch.map((m) => ({ id: m.id, outgoing: m.direction === 'outgoing', senderLabel: m.direction === 'outgoing' ? t('我') : t('对方'), body: m.body, createdAt: m.created_at, isFile: false }));
  } else {
    type GroupMsg = { id: string; sender: string; sender_name: string; body: string; created_at: number; kind: string; file_name: string | null; recalled_at: number | null };
    const [batch, fingerprint] = await Promise.all([
      invoke<GroupMsg[]>('v1_list_group', { groupId: peerId, limit: 200, before: null }),
      invoke<string>('v1_identity_fingerprint'),
    ]);
    rows = batch.map((m) => ({
      id: m.id,
      outgoing: m.sender === fingerprint,
      senderLabel: m.sender === fingerprint ? t('我') : (m.sender_name || m.sender.slice(0, 8)),
      body: m.recalled_at ? t('[消息已撤回]') : (m.kind === 'file' ? `[${t('文件')}] ${m.file_name ?? ''}` : m.body),
      createdAt: m.created_at,
      isFile: m.kind === 'file',
    }));
  }
  rows.reverse();
}

function visibleRows(): RecRow[] {
  let list = rows;
  if (typeFilter === 'file') list = list.filter((r) => r.isFile);
  if (filter) {
    const needle = filter.toLowerCase();
    list = list.filter((r) => r.body.toLowerCase().includes(needle));
  }
  return list;
}

function render(): void {
  document.title = title;
  const list = visibleRows();
  const app = document.getElementById('app')!;
  const fileTab = kind === 'group'
    ? `<button class="records-tab${typeFilter === 'file' ? ' active' : ''}" id="tab-file">${t('文件')}</button>`
    : '';
  app.innerHTML = `
    <div class="records-window">
      <header class="records-titlebar" data-tauri-drag-region>
        <strong data-tauri-drag-region>${escapeText(title)}</strong>
        <button class="records-close" id="records-close" aria-label="${t('关闭')}" title="${t('关闭')}">✕</button>
      </header>
      <div class="records-toolbar">
        <input id="records-search" type="search" maxlength="200" placeholder="${t('搜索聊天记录')}" value="${escapeText(filter)}">
        <button class="records-tab${typeFilter === 'all' ? ' active' : ''}" id="tab-all">${t('全部')}</button>
        ${fileTab}
        <label class="records-select-all"><input type="checkbox" id="records-all" ${list.length > 0 && list.every((r) => selected.has(r.id)) ? 'checked' : ''}>${t('全选')}</label>
        <button class="outline danger" id="records-delete" ${selected.size ? '' : 'disabled'}>${t('删除所选')}${selected.size ? `（${selected.size}）` : ''}</button>
      </div>
      <div class="records-list" id="records-list">
        ${list.length === 0 ? `${t('没有匹配的聊天记录。')}` : list.map((r) => `
          <label class="records-row${selected.has(r.id) ? ' selected' : ''}">
            <input type="checkbox" data-record-id="${escapeText(r.id)}" ${selected.has(r.id) ? 'checked' : ''}>
            <span class="records-meta"><b>${escapeText(r.senderLabel)}</b><time>${formatTime(r.createdAt)}</time></span>
            <span class="records-body">${escapeText(r.body)}</span>
          </label>`).join('')}
      </div>
      <footer class="records-foot">${t('已选')}${selected.size}${t('条 · 删除仅影响本机记录')}</footer>
    </div>`;
  document.getElementById('records-close')!.addEventListener('click', () => {
    void invoke('v1_close_current_window').catch((error) => {
      console.error('close failed', error);
      window.alert(`${t('关闭窗口失败：')}${String(error)}`);
    });
  });
  const search = document.getElementById('records-search') as HTMLInputElement;
  search.addEventListener('input', () => { filter = search.value.trim(); render(); search.focus(); });
  const caret = search.value.length;
  search.setSelectionRange(caret, caret);
  document.getElementById('tab-all')!.addEventListener('click', () => { typeFilter = 'all'; render(); });
  const fileButton = document.getElementById('tab-file');
  if (fileButton) fileButton.addEventListener('click', () => { typeFilter = 'file'; render(); });
  document.getElementById('records-all')!.addEventListener('change', (event) => {
    const checked = (event.target as HTMLInputElement).checked;
    if (checked) for (const r of visibleRows()) selected.add(r.id);
    else for (const r of visibleRows()) selected.delete(r.id);
    render();
  });
  document.getElementById('records-delete')!.addEventListener('click', () => { void deleteSelected(); });
  for (const box of app.querySelectorAll<HTMLInputElement>('input[data-record-id]')) {
    box.addEventListener('change', () => {
      if (box.checked) selected.add(box.dataset.recordId!); else selected.delete(box.dataset.recordId!);
      render();
    });
  }
}

async function deleteSelected(): Promise<void> {
  if (!selected.size) return;
  if (!window.confirm(`${t('确定删除所选的')}${selected.size}${t('条聊天记录吗？删除后不可恢复。')}`)) return;
  try {
    await invoke('v1_delete_messages', { kind, id: peerId, ids: [...selected] });
    selected.clear();
    await loadRows();
    render();
  } catch (error) {
    window.alert(`${t('删除失败：')}${String(error)}`);
  }
}

export async function runRecordsWindow(): Promise<void> {
  console.info('[records] boot', window.location.href);
  window.addEventListener('error', (event) => { console.error('[records] error', event); fail(`${t('页面出错：')}${event.message}`); });
  window.addEventListener('unhandledrejection', (event) => fail(`${t('页面出错：')}${String(event.reason)}`));
  if (!parseConversation()) { fail(`${t('聊天记录窗口参数无效。')} ${window.location.search}`); return; }
  await loadChatDisplay();
  await resolveTitle();
  try { await loadRows(); }
  catch (error) { fail(`${t('无法加载聊天记录：')}${String(error)}`); return; }
  render();
}
