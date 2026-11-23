import './style.css';
import { invoke } from '@tauri-apps/api/core';
import { save as saveFileDialog } from '@tauri-apps/plugin-dialog';
import { t } from './i18n';
import { loadChatDisplay, getShowSeconds } from './chat_prefs';
import { historyDateRange, recordsListState } from './records_window_state';

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
type HistoryHit = {
  id: string; sender: string; sender_name: string; body: string; created_at: number; logical: number;
  kind: string; file_name: string | null; recalled_at: number | null;
};

let kind = 'self';
let peerId = '';
let title = t('聊天记录');
let filter = '';
let typeFilter: 'all' | 'file' = 'all';
let fromDate = '';
let toDate = '';
let rowOffset = 0;
let rowHasMore = false;
const rowPageSize = 100;
let loadSequence = 0;
let searchTimer: number | null = null;
let loading = true;
let loadError = '';
const selected = new Set<string>();
let rows: RecRow[] = [];
let exportBusy = false;
let exportNotice = '';
let exportPassword = '';
let exportIncludeText = true;
let exportIncludeImages = true;
let exportIncludeFiles = true;

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
  loading = false;
  loadError = message;
  render();
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

function scheduleLoadRows(): void {
  if (searchTimer !== null) window.clearTimeout(searchTimer);
  loadSequence += 1;
  rowOffset = 0;
  rowHasMore = false;
  selected.clear();
  loadError = '';
  const range = historyDateRange(fromDate, toDate);
  if (range.invalid) {
    rows = [];
    loading = false;
    loadError = t('开始日期不能晚于结束日期');
    render();
    return;
  }
  loading = true;
  render();
  searchTimer = window.setTimeout(() => {
    searchTimer = null;
    void loadRows();
  }, 250);
}

async function loadRows(): Promise<void> {
  const sequence = ++loadSequence;
  const range = historyDateRange(fromDate, toDate);
  if (range.invalid) {
    rows = [];
    rowHasMore = false;
    loading = false;
    loadError = t('开始日期不能晚于结束日期');
    render();
    return;
  }
  loading = true;
  loadError = '';
  render();
  try {
    const [hits, fingerprint] = await Promise.all([
      invoke<HistoryHit[]>('v1_search_chat_history', {
        kind, target: peerId, query: filter.trim(), category: typeFilter,
        since: range.since, until: range.until, recalledLabel: t('[消息已撤回]'),
        limit: rowPageSize + 1, offset: rowOffset,
      }),
      invoke<string>('identity_fingerprint'),
    ]);
    if (sequence !== loadSequence) return;
    rowHasMore = hits.length > rowPageSize;
    rows = hits.slice(0, rowPageSize).map((message) => ({
      id: message.id,
      outgoing: kind === 'self' || message.sender === fingerprint,
      senderLabel: kind === 'self' || message.sender === fingerprint ? t('我') : kind === 'direct' ? t('对方') : message.sender_name,
      body: message.recalled_at ? t('[消息已撤回]') : (message.kind === 'file' ? `[${t('文件')}] ${message.file_name ?? message.body}` : message.body),
      createdAt: message.created_at,
      isFile: message.kind === 'file',
    })).reverse();
  } catch (error) {
    if (sequence === loadSequence) {
      rows = [];
      rowHasMore = false;
      loadError = `${t('无法加载聊天记录：')}${String(error)}`;
    }
  } finally {
    if (sequence === loadSequence) {
      loading = false;
      render();
    }
  }
}

function visibleRows(): RecRow[] { return rows; }

function render(): void {
  document.title = title;
  const list = visibleRows();
  const listState = recordsListState(loading, loadError, list.length);
  const app = document.getElementById('app')!;
  const focusedInput = document.activeElement instanceof HTMLInputElement ? document.activeElement : null;
  const focusedId = focusedInput?.id ?? '';
  const searchCaret = focusedId === 'records-search' ? focusedInput?.selectionStart ?? null : null;
  let listContent: string;
  switch (listState) {
    case 'loading': listContent = t('读取中…'); break;
    case 'error': listContent = escapeText(loadError); break;
    case 'empty': listContent = t('没有匹配的聊天记录。'); break;
    case 'rows': listContent = list.map((r) => `
          <label class="records-row${selected.has(r.id) ? ' selected' : ''}">
            <input type="checkbox" data-record-id="${escapeText(r.id)}" ${selected.has(r.id) ? 'checked' : ''}>
            <span class="records-meta"><b>${escapeText(r.senderLabel)}</b><time>${formatTime(r.createdAt)}</time></span>
            <span class="records-body">${escapeText(r.body)}</span>
          </label>`).join(''); break;
  }
  const fileTab = kind === 'group'
    ? `<button class="records-tab${typeFilter === 'file' ? ' active' : ''}" id="tab-file">${t('文件')}</button>`
    : '';
  const pagination = rowHasMore || rowOffset > 0 ? `
    <nav class="records-pagination" aria-label="${t('搜索结果分页')}">
      <button class="outline" id="records-prev" ${rowOffset <= 0 || loading ? 'disabled' : ''}>${t('上一页')}</button>
      <span>${t('side.searchPage', { n: Math.floor(rowOffset / rowPageSize) + 1 })}</span>
      <button class="outline" id="records-next" ${!rowHasMore || loading ? 'disabled' : ''}>${t('下一页')}</button>
    </nav>` : '';
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
      <div class="records-date-filter">
        <label><span>${t('开始日期')}</span><input id="records-date-from" type="date" value="${escapeText(fromDate)}"></label>
        <label><span>${t('结束日期')}</span><input id="records-date-to" type="date" value="${escapeText(toDate)}"></label>
        <small>${t('记录窗口日期筛选提示')}</small>
      </div>
      <div class="records-list" id="records-list">
        ${listContent}
      </div>
      ${pagination}
      <section class="records-export" aria-label="${t('导出聊天记录')}">
        <input id="records-archive-password" type="password" maxlength="256" autocomplete="new-password" placeholder="${t('聊天记录密码（至少 8 个字符）')}" aria-label="${t('聊天记录密码')}" value="${escapeText(exportPassword)}">
        <div class="records-export-options">
          <label><input type="checkbox" id="records-export-text" ${exportIncludeText ? 'checked' : ''}>${t('文字消息')}</label>
          <label><input type="checkbox" id="records-export-images" ${exportIncludeImages ? 'checked' : ''}>${t('图片')}</label>
          <label><input type="checkbox" id="records-export-files" ${exportIncludeFiles ? 'checked' : ''}>${t('其他文件')}</label>
        </div>
        <div class="records-export-actions">
          <button class="outline" id="records-export-selected" ${exportBusy || !selected.size ? 'disabled' : ''}>${t('导出所选')}（${selected.size}）</button>
          <button class="outline" id="records-export-all" ${exportBusy || loading || !!loadError ? 'disabled' : ''}>${t('导出全部会话')}</button>
        </div>
        <p id="records-export-status" role="status">${escapeText(exportNotice)}</p>
      </section>
      <footer class="records-foot">${t('已选')}${selected.size}${t('条 · 删除仅影响本机记录')}</footer>
    </div>`;
  if (focusedId) {
    const nextFocus = document.getElementById(focusedId);
    nextFocus?.focus();
    if (nextFocus instanceof HTMLInputElement && nextFocus.type === 'search' && searchCaret !== null) {
      nextFocus.setSelectionRange(searchCaret, searchCaret);
    }
  }
  document.getElementById('records-close')!.addEventListener('click', () => {
    void invoke('v1_close_current_window').catch((error) => {
      console.error('close failed', error);
      window.alert(`${t('关闭窗口失败：')}${String(error)}`);
    });
  });
  const search = document.getElementById('records-search') as HTMLInputElement;
  search.addEventListener('input', () => { filter = search.value.trim(); scheduleLoadRows(); });
  document.getElementById('records-date-from')!.addEventListener('change', (event) => {
    fromDate = (event.target as HTMLInputElement).value;
    scheduleLoadRows();
  });
  document.getElementById('records-date-to')!.addEventListener('change', (event) => {
    toDate = (event.target as HTMLInputElement).value;
    scheduleLoadRows();
  });
  document.getElementById('tab-all')!.addEventListener('click', () => { typeFilter = 'all'; scheduleLoadRows(); });
  const fileButton = document.getElementById('tab-file');
  if (fileButton) fileButton.addEventListener('click', () => { typeFilter = 'file'; scheduleLoadRows(); });
  document.getElementById('records-prev')?.addEventListener('click', () => {
    if (rowOffset <= 0 || loading) return;
    rowOffset = Math.max(0, rowOffset - rowPageSize);
    void loadRows();
  });
  document.getElementById('records-next')?.addEventListener('click', () => {
    if (!rowHasMore || loading) return;
    rowOffset += rowPageSize;
    void loadRows();
  });
  document.getElementById('records-all')!.addEventListener('change', (event) => {
    const checked = (event.target as HTMLInputElement).checked;
    if (checked) for (const r of visibleRows()) selected.add(r.id);
    else for (const r of visibleRows()) selected.delete(r.id);
    render();
  });
  document.getElementById('records-delete')!.addEventListener('click', () => { void deleteSelected(); });
  document.getElementById('records-export-selected')!.addEventListener('click', () => { void exportArchive(true); });
  document.getElementById('records-export-all')!.addEventListener('click', () => { void exportArchive(false); });
  document.getElementById('records-archive-password')!.addEventListener('input', (event) => {
    exportPassword = (event.target as HTMLInputElement).value;
  });
  for (const [id, setValue] of [
    ['records-export-text', (value: boolean) => { exportIncludeText = value; }],
    ['records-export-images', (value: boolean) => { exportIncludeImages = value; }],
    ['records-export-files', (value: boolean) => { exportIncludeFiles = value; }],
  ] as const) {
    document.getElementById(id)!.addEventListener('change', (event) => setValue((event.target as HTMLInputElement).checked));
  }
  for (const box of app.querySelectorAll<HTMLInputElement>('input[data-record-id]')) {
    box.addEventListener('change', () => {
      if (box.checked) selected.add(box.dataset.recordId!); else selected.delete(box.dataset.recordId!);
      render();
    });
  }
}

async function exportArchive(selectedOnly: boolean): Promise<void> {
  if (exportBusy || (selectedOnly && !selected.size)) return;
  const range = historyDateRange(fromDate, toDate);
  if (range.invalid) { exportNotice = t('开始日期不能晚于结束日期'); render(); return; }
  if (exportPassword.length < 8) { exportNotice = t('聊天记录密码至少 8 个字符'); render(); return; }
  if (!exportIncludeText && !exportIncludeImages && !exportIncludeFiles) { exportNotice = t('至少选择一种导出内容'); render(); return; }
  const date = new Date();
  const stamp = `${date.getFullYear()}${`${date.getMonth() + 1}`.padStart(2, '0')}${`${date.getDate()}`.padStart(2, '0')}`;
  const safeTitle = title.replace(/[\\/:*?"<>|]/g, '_').slice(0, 64);
  const path = await saveFileDialog({
    title: t(selectedOnly ? '导出所选聊天记录' : '导出全部聊天记录'),
    defaultPath: `qbird-chat-${safeTitle}-${stamp}.qchat`,
    filters: [{ name: 'Qbird 聊天记录', extensions: ['qchat'] }],
  }).catch(() => null);
  if (!path) return;
  const messageIds = selectedOnly ? [...selected] : null;
  exportBusy = true;
  exportNotice = t('正在导出…');
  render();
  try {
    const info = await invoke<{ name: string; message_count: number; attachment_count: number }>('v1_export_chat_archive', {
      kind, target: peerId, password: exportPassword, path, since: range.since, until: range.until, messageIds,
      includeText: exportIncludeText, includeImages: exportIncludeImages, includeFiles: exportIncludeFiles,
    });
    exportNotice = `${t('已导出')}「${info.name}」：${info.message_count}${t('条消息')}，${info.attachment_count}${t('个附件')}。`;
  } catch (error) {
    exportNotice = `${t('导出失败：')}${String(error)}`;
  } finally {
    exportBusy = false;
    render();
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
  loading = true;
  loadError = '';
  console.info('[records] boot', window.location.href);
  window.addEventListener('error', (event) => { console.error('[records] error', event); fail(`${t('页面出错：')}${event.message}`); });
  window.addEventListener('unhandledrejection', (event) => fail(`${t('页面出错：')}${String(event.reason)}`));
  if (!parseConversation()) { fail(`${t('聊天记录窗口参数无效。')} ${window.location.search}`); return; }
  render();
  await loadChatDisplay();
  await resolveTitle();
  try { await loadRows(); }
  catch (error) { fail(`${t('无法加载聊天记录：')}${String(error)}`); return; }
  loading = false;
  render();
}
