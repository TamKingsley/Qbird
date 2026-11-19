import './style.css';
import { invoke, isTauri } from '@tauri-apps/api/core';
import { open as openFileDialog, save as saveFileDialog } from '@tauri-apps/plugin-dialog';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import { version as appVersion } from '../package.json';
import { DirectChat } from './direct_chat';
import { GroupChat } from './group_chat';
import { captureRegion } from './screenshot';

type Page = 'messages' | 'contacts' | 'discover' | 'settings' | 'about' | 'profile';
const root = document.querySelector<HTMLDivElement>('#app');
if (!root) throw new Error('Application root is missing');
const app = root;
let page: Page = 'messages';
let menuOpen = false;
let fingerprint = isTauri() ? '正在读取…' : '仅桌面应用可读取';
type Candidate = { service_name: string; addresses: string[] };
let candidates: Candidate[] = [];
let discoveryStatus = isTauri() ? '正在查找…' : '仅桌面应用支持局域网发现';
let discoveryLoading = false;
type ProbeRequest = { fingerprint: string; remote_ip: string };
type ProbeStatus = { running: boolean; port: number | null; requests: ProbeRequest[] };
let probeStatus: ProbeStatus = { running: false, port: null, requests: [] };
let probePort = '42853';
type NetworkSettings = { default_port: number };
let networkSettings: NetworkSettings = { default_port: 42853 };
let networkNotice = '';
let scanCidr = '192.168.1.0/24';
let scanBusy = false;
let scanNotice = '';
type ScanHit = { address: string; fingerprint: string };
let scanHits: ScanHit[] = [];
let scanBlacklist: string[] = [];
let scanBlacklistDraft = '';
let scanBlacklistNotice = '';
let manualAddress = '';
let probeNotice = isTauri() ? '探测服务未启动' : '仅桌面应用支持身份探测';
let probeMessage = '';
let remoteFingerprint = '';
let observedAddress = '';
let probeBusy = false;
let selectedFingerprint = '';
type Contact = { fingerprint: string; display_name: string; trust_state: string; remark: string | null; category: string | null };
let contactQuery = '';
let profileNotice = '';
let avatarUrl: string | null = null;
let avatarNotice = '';
let avatarBusy = false;
type ComposerSettings = { send_shortcut: string; rows: number };
type ExperimentSettings = { group_file_quote: boolean };
let experiments: ExperimentSettings = { group_file_quote: false };
let experimentNotice = '';
let composer: ComposerSettings = { send_shortcut: 'enter', rows: 3 };
let composerNotice = '';
const COMMON_EMOJIS = ['😀', '😁', '😂', '🤣', '😊', '😍', '😘', '😜', '🤔', '😅', '😭', '😡', '👍', '👎', '👏', '🙏', '💪', '🎉', '❤️', '💔', '🔥', '✨', '⭐', '🌹', '🍀', '☕', '🍰', '🐱', '🐶', '🌙', '☀️', '💡', '✅', '❌', '⚠️', '📌', '📎', '🚀', '🎯', '💬', '🙌', '🤝', '😴', '😎', '🥳', '🤩', '😢', '😤'];
let stickers: string[] = [];
let stickersLoaded = false;
const stickerUrls = new Map<string, string>();
let stickerManage = false;
let emojiNotice = '';
type Profile = { nickname: string; email: string };
type Conversation = { id: string; last_body: string; last_logical: number; kind: string; unread: number };
type Group = { id: string; name: string; owner: string; member_count: number; hidden: boolean; is_owner: boolean; last_body: string; last_logical: number };
let groups: Group[] = [];
type LocalMessage = { id: string; body: string; logical: number; created_at: number };
let selfOpen = false;
let selfMessages: LocalMessage[] = [];
let selfHasOlder = false;
let selfBusy = false;
let selfDraft = '';
type StorageUsage = { used: number; quota: number };
let storageInfo: StorageUsage | null = null;
let storageNotice = '';
let storageBusy = false;
type BackupInfo = { created_at: number; include_chat: boolean; message_count: number; checksum: string };
type RestoreReport = { mode: string; inserted: number; skipped: number; conflicts: number; snapshot_path: string | null };
let backupPassword = '';
let backupPath = '';
let backupInfo: BackupInfo | null = null;
let backupNotice = '';
let backupBusy = false;
type SearchHit = {
  conversation_kind: 'self' | 'direct' | 'group'; conversation_id: string; conversation_name: string;
  message_id: string; sender: string; sender_name: string; body: string;
  created_at: number; logical: number; kind: string;
};
let searchHits: SearchHit[] = [];
let searchQuery = '';
let searchBusy = false;
let searchError = '';
let searchSeq = 0;
let searchTimer: number | null = null;
type LockStatus = { has_password: boolean; locked: boolean; idle_minutes: number };
let lockState: LockStatus = { has_password: false, locked: false, idle_minutes: 0 };
let lockNotice = '';
let lockBusy = false;
let lockInitialized = false;
let lastActivity = Date.now();
const direct = new DirectChat(app, () => { groupChat.clear(); selfOpen = false; page = 'messages'; menuOpen = false; render(); });
const groupChat = new GroupChat(app, () => { direct.clear(); selfOpen = false; page = 'messages'; menuOpen = false; render(); });

const icons = {
  messages: '<path d="M4 5.75A2.75 2.75 0 0 1 6.75 3h10.5A2.75 2.75 0 0 1 20 5.75v7.5A2.75 2.75 0 0 1 17.25 16H10l-4.5 3v-3.25A2.75 2.75 0 0 1 4 13.25z"/>',
  contacts: '<circle cx="9" cy="8" r="3"/><path d="M3.5 18c0-3 2.2-5 5.5-5s5.5 2 5.5 5M16 5.5a3 3 0 0 1 0 5.8M16.5 13.5c2.2.3 4 1.9 4 4.5"/>',
  discover: '<circle cx="12" cy="12" r="8.5"/><path d="m15.5 8.5-2.1 4.9-4.9 2.1 2.1-4.9z"/>',
  lock: '<rect x="5.5" y="10" width="13" height="10" rx="2"/><path d="M8 10V7a4 4 0 0 1 8 0v3M12 14v2.5"/>',
  menu: '<path d="M4 7h16M4 12h16M4 17h16"/>',
  settings: '<circle cx="12" cy="12" r="3"/><path d="M12 3.5v2M12 18.5v2M3.5 12h2M18.5 12h2M6 6l1.4 1.4M16.6 16.6 18 18M18 6l-1.4 1.4M7.4 16.6 6 18"/>',
  spark: '<path d="M12 3.5c.7 4.3 2.2 5.8 6.5 6.5-4.3.7-5.8 2.2-6.5 6.5-.7-4.3-2.2-5.8-6.5-6.5 4.3-.7 5.8-2.2 6.5-6.5ZM18.5 15.5c.3 1.8.9 2.4 2.5 2.5-1.6.2-2.2.8-2.5 2.5-.3-1.7-.9-2.3-2.5-2.5 1.6-.1 2.2-.7 2.5-2.5Z"/>',
} as const;

function icon(name: keyof typeof icons): string {
  return `<svg class="ui-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" focusable="false">${icons[name]}</svg>`;
}

async function openSelf(): Promise<void> {
  if (!isTauri() || selfBusy) return;
  try {
    await invoke('v1_open_self');
    direct.clear();
    selfOpen = true; page = 'messages'; menuOpen = false; selfMessages = []; selfHasOlder = false;
    render();
    await loadSelfMessages(false);
  } catch { const status = app.querySelector<HTMLElement>('#contacts-status'); if (status) status.textContent = '无法打开自聊'; }
}

async function hideSelf(): Promise<void> {
  if (!isTauri() || selfBusy) return;
  try {
    await invoke('v1_hide_self');
    selfOpen = false; render();
  } catch { const error = app.querySelector<HTMLElement>('#self-error'); if (error) error.textContent = '无法隐藏会话'; }
}

async function sendSelf(): Promise<void> {
  const input = app.querySelector<HTMLTextAreaElement>('#self-input');
  const error = app.querySelector<HTMLElement>('#self-error');
  const button = app.querySelector<HTMLButtonElement>('button[data-action="self-send"]');
  if (!isTauri() || !input || !error || selfBusy) return;
  const body = input.value;
  if (!body.trim()) { error.textContent = '请输入消息'; return; }
  selfBusy = true; if (button) button.disabled = true; error.textContent = '';
  try {
    const message = await invoke<LocalMessage>('v1_send_self', { body });
    selfMessages.push(message);
    if (selfDraft === body) selfDraft = '';
    if (input.isConnected && input.value === body) input.value = '';
    paintSelfMessages('bottom');
    void refreshConversations();
  } catch (failure) { if (error.isConnected) error.textContent = displayError(failure); }
  finally { selfBusy = false; if (button?.isConnected) button.disabled = false; }
}

function appendUnread(button: HTMLButtonElement, count: number): void {
  if (count <= 0) return;
  const badge = document.createElement('span');
  badge.className = 'unread-badge';
  badge.textContent = count > 99 ? '99+' : String(count);
  badge.setAttribute('aria-label', `${count} 条未读消息`);
  button.append(badge);
}

async function refreshConversations(): Promise<void> {
  const list = app.querySelector<HTMLElement>('#conversation-list');
  if (!list) return;
  if (!isTauri()) { list.textContent = '请在桌面应用中查看会话'; return; }
  try {
    const [conversations, contacts, groupList] = await Promise.all([
      invoke<Conversation[]>('v1_list_conversations'),
      invoke<Contact[]>('v1_list_contacts'),
      invoke<Group[]>('v1_list_groups'),
    ]);
    groups = groupList;
    if (!list.isConnected) return;
    list.replaceChildren();
    if (!conversations.length) { list.textContent = '暂无会话'; return; }
    // Whatever is on screen counts as read; the backend counter is the source of truth.
    if (direct.peer) {
      const active = conversations.find((item) => item.kind === 'direct' && item.id === direct.peer);
      if (active && active.unread > 0) { active.unread = 0; void invoke('v1_open_direct', { peer: direct.peer }).catch(() => {}); }
    }
    if (groupChat.groupId) {
      const active = conversations.find((item) => item.kind === 'group' && item.id === groupChat.groupId);
      if (active && active.unread > 0) { active.unread = 0; void invoke('v1_open_group', { groupId: groupChat.groupId }).catch(() => {}); }
    }
    for (const conversation of conversations) {
      if (conversation.kind === 'group') {
        const group = groups.find(item => item.id === conversation.id);
        if (!group) continue;
        const button = document.createElement('button');
        button.className = `conversation-row${groupChat.groupId === conversation.id ? ' selected' : ''}`;
        button.dataset.action = 'open-group'; button.dataset.group = group.id; button.dataset.name = group.name;
        const name = document.createElement('strong'); name.textContent = group.name;
        const preview = document.createElement('span'); preview.textContent = conversation.last_body || `${group.member_count} 位成员`;
        button.append(name, preview); appendUnread(button, conversation.unread); list.append(button);
        continue;
      }
      const contact = contacts.find(item => item.fingerprint === conversation.id && item.trust_state === 'verified');
      if (conversation.id !== 'self' && !contact) continue;
      const button = document.createElement('button');
      button.className = `conversation-row${conversation.id === 'self' ? (selfOpen ? ' selected' : '') : (direct.peer === conversation.id ? ' selected' : '')}`;
      button.dataset.action = conversation.id === 'self' ? 'open-self' : 'open-direct';
      appendUnread(button, conversation.unread);
      if (contact) { button.dataset.peer = contact.fingerprint; button.dataset.name = contact.display_name; }
      const name = document.createElement('strong'); name.textContent = contact?.display_name ?? '自己';
      const preview = document.createElement('span'); preview.textContent = conversation.last_body;
      button.append(name, preview); list.append(button);
    }
  } catch { if (list.isConnected) list.textContent = '无法读取会话'; }
}

function paintSelfMessages(scroll: 'bottom' | 'preserve'): void {
  const history = app.querySelector<HTMLElement>('#self-history');
  const list = app.querySelector<HTMLElement>('#self-messages');
  const older = app.querySelector<HTMLButtonElement>('button[data-action="self-older"]');
  if (!history || !list || !older) return;
  const previousHeight = history.scrollHeight, previousTop = history.scrollTop;
  list.replaceChildren();
  for (const message of selfMessages) {
    const row = document.createElement('div'); row.className = 'message-row'; row.dataset.id = message.id;
    const body = document.createElement('div'); body.className = 'message-bubble'; body.textContent = message.body;
    row.append(body); list.append(row);
  }
  older.hidden = !selfHasOlder;
  history.scrollTop = scroll === 'bottom' ? history.scrollHeight : previousTop + history.scrollHeight - previousHeight;
}

async function loadSelfMessages(older: boolean): Promise<void> {
  if (!isTauri() || selfBusy) return;
  selfBusy = true;
  try {
    const batch = await invoke<LocalMessage[]>('v1_list_self_messages', { limit: 200, offset: older ? selfMessages.length : 0 });
    selfMessages = older ? [...batch.reverse(), ...selfMessages] : batch.reverse();
    selfHasOlder = batch.length === 200;
    paintSelfMessages(older ? 'preserve' : 'bottom');
  } catch { const error = app.querySelector<HTMLElement>('#self-error'); if (error) error.textContent = '无法读取自聊记录'; }
  finally { selfBusy = false; }
}

function selfChatView(): string {
  return `<section class="self-chat" aria-label="自聊">
    <header class="chat-header"><div><strong>自己</strong><small>仅保存在本机</small></div><div class="group-actions"><button class="outline" data-action="export-text" data-kind="self">导出文本</button><button class="outline" data-action="hide-self">不显示此会话</button></div></header>
    <div class="chat-history" id="self-history" aria-label="自聊记录"><button class="load-older" data-action="self-older" hidden>加载更早消息</button><div id="self-messages"></div></div>
    <div class="chat-compose"><label for="self-input">消息</label><textarea id="self-input" rows="3" maxlength="32768" placeholder="写点什么…"></textarea><div class="compose-footer"><span>Enter 发送 · Shift+Enter 换行</span><button data-action="self-send">发送</button></div><p id="self-error" role="status"></p></div>
  </section>`;
}

type PlaceholderPage = 'messages' | 'contacts' | 'discover' | 'settings' | 'about' | 'profile';
const pages: Record<PlaceholderPage, { title: string; eyebrow: string; heading: string; copy: string }> = {
  messages: { title: '消息', eyebrow: 'MESSAGES', heading: '还没有会话', copy: '从联系人打开自聊或已确认的单聊。' },
  contacts: { title: '联系人', eyebrow: 'CONTACTS', heading: '联系人', copy: '可搜索联系人、编辑本机备注与分组。' },
  discover: { title: '发现', eyebrow: 'DISCOVERY', heading: '局域网发现', copy: '仅浏览同网段 IPv4 mDNS 服务；发现结果未经身份验证，不代表可信联系人。' },
  settings: { title: '设置', eyebrow: 'PREFERENCES', heading: '设置', copy: '存储配额、备份恢复与应用锁已开放；插件与更多设置将在后续阶段提供。' },
  about: { title: '设置', eyebrow: 'ABOUT', heading: 'Qbird 青鸟', copy: '本地优先的局域网聊天工具。' },
  profile: { title: '我的资料', eyebrow: 'PROFILE', heading: '个人资料', copy: '昵称与邮箱仅保存在本机。' }
};

function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) return '未知';
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
  return `${(bytes / 1024 / 1024 / 1024).toFixed(2)} GB`;
}

function profileView(): string {
  return `<section class="card settings-card">
    <span class="eyebrow">PROFILE / 我的资料</span><h2>个人资料</h2>
    <p>昵称和邮箱只保存在本机，不改变身份指纹；联系人看到的是对方各自保存的名称。</p>
    <div class="avatar-row">
      <div class="avatar-preview">${avatarMarkup()}</div>
      <div class="avatar-actions">
        <button class="outline" data-action="choose-avatar" ${avatarBusy ? 'disabled' : ''}>${avatarUrl ? '更换头像…' : '选择头像…'}</button>
        <button class="outline" data-action="clear-avatar" ${avatarUrl && !avatarBusy ? '' : 'disabled'}>移除头像</button>
        <small>自动居中裁剪为正方形，最长边 512 像素；头像只保存在本机，尚未随消息发送。</small>
      </div>
    </div>
    <label for="profile-nickname">昵称</label>
    <input id="profile-nickname" type="text" maxlength="64" autocomplete="off">
    <label for="profile-email">邮箱（可选）</label>
    <input id="profile-email" type="text" maxlength="128" autocomplete="off">
    <div class="probe-row"><button class="outline" data-action="save-profile">保存资料</button></div>
    <p id="profile-notice" role="status">${profileNotice}</p>
    <p>本机身份指纹（SHA-256）：<br><code class="fingerprint">${fingerprint}</code></p>
    <p id="avatar-notice" role="status">${avatarNotice}</p>
  </section>`;
}

async function refreshProfile(): Promise<void> {
  if (!isTauri()) return;
  try {
    const profile = await invoke<Profile>('v1_profile');
    const nickname = app.querySelector<HTMLInputElement>('#profile-nickname');
    const email = app.querySelector<HTMLInputElement>('#profile-email');
    if (nickname) nickname.value = profile.nickname;
    if (email) email.value = profile.email;
  } catch { profileNotice = '无法读取个人资料'; }
}

async function saveProfile(): Promise<void> {
  const nickname = app.querySelector<HTMLInputElement>('#profile-nickname')?.value ?? '';
  const email = app.querySelector<HTMLInputElement>('#profile-email')?.value ?? '';
  const notice = app.querySelector<HTMLElement>('#profile-notice');
  if (!isTauri()) return;
  try {
    await invoke<Profile>('v1_set_profile', { nickname, email });
    profileNotice = '资料已保存在本机。';
  } catch (error) { profileNotice = typeof error === 'string' ? error : '无法保存资料'; }
  if (notice) notice.textContent = profileNotice;
}

function avatarMarkup(): string {
  return avatarUrl ? `<img src="${avatarUrl}" alt="我的头像">` : '青';
}

/// Copies into a fresh ArrayBuffer so Blob accepts it under strict DOM typings.
function toArrayBuffer(bytes: Uint8Array): ArrayBuffer {
  const buffer = new ArrayBuffer(bytes.byteLength);
  new Uint8Array(buffer).set(bytes);
  return buffer;
}
async function refreshAvatar(): Promise<void> {
  if (!isTauri()) return;
  try {
    const bytes = await invoke<number[] | null>('v1_avatar');
    setAvatarImage(bytes && bytes.length ? new Uint8Array(bytes) : null);
  } catch { /* 保留占位字符，不阻塞界面 */ }
}

function setAvatarImage(bytes: Uint8Array | null): void {
  if (avatarUrl) { URL.revokeObjectURL(avatarUrl); avatarUrl = null; }
  avatarUrl = bytes && bytes.length ? URL.createObjectURL(new Blob([toArrayBuffer(bytes)], { type: 'image/png' })) : null;
}

/// Centre-crops any picked image into a square PNG so the saved avatar is bounded.
async function cropSquare(source: Uint8Array, edge: number): Promise<Uint8Array> {
  const bitmap = await createImageBitmap(new Blob([toArrayBuffer(source)]));
  const side = Math.min(bitmap.width, bitmap.height);
  const canvas = document.createElement('canvas');
  canvas.width = edge; canvas.height = edge;
  const context = canvas.getContext('2d');
  if (!context) throw new Error('无法创建画布');
  context.drawImage(bitmap, (bitmap.width - side) / 2, (bitmap.height - side) / 2, side, side, 0, 0, edge, edge);
  const blob = await new Promise<Blob | null>((resolve) => canvas.toBlob(resolve, 'image/png'));
  if (!blob) throw new Error('无法生成头像');
  return new Uint8Array(await blob.arrayBuffer());
}

async function chooseAvatar(): Promise<void> {
  if (!isTauri() || avatarBusy) return;
  try {
    const picked = await openFileDialog({ multiple: false, filters: [{ name: '图片', extensions: ['png', 'jpg', 'jpeg'] }] });
    if (!picked || typeof picked !== 'string') return;
    avatarBusy = true; avatarNotice = '正在裁剪头像…'; if (page === 'profile') render();
    const source = new Uint8Array(await invoke<number[]>('v1_read_image_file', { path: picked }));
    const cropped = await cropSquare(source, 512);
    await invoke('v1_set_avatar', { bytes: Array.from(cropped) });
    setAvatarImage(cropped);
    avatarNotice = '头像已保存，仅本机可见。';
  } catch (error) { avatarNotice = displayError(error); }
  finally { avatarBusy = false; if (page === 'profile') render(); }
}

async function clearAvatar(): Promise<void> {
  if (!isTauri() || avatarBusy) return;
  avatarBusy = true;
  try {
    await invoke('v1_clear_avatar');
    setAvatarImage(null);
    avatarNotice = '头像已移除。';
  } catch (error) { avatarNotice = displayError(error); }
  finally { avatarBusy = false; if (page === 'profile') render(); }
}

function emojiPanelHtml(): string {
  const grid = COMMON_EMOJIS.map((emoji) => `<button type="button" data-action="pick-emoji" data-emoji="${emoji}">${emoji}</button>`).join('');
  return `<div class="emoji-grid">${grid}</div>
    <div class="sticker-head"><strong>我的表情</strong><button type="button" class="outline" data-action="add-sticker">添加…</button><button type="button" class="outline" data-action="toggle-sticker-manage">${stickerManage ? '完成' : '管理'}</button></div>
    <div class="sticker-list"></div>
    <p class="emoji-notice" role="status">${emojiNotice}</p>`;
}

/// Re-created after every render, so the trigger and panel always exist together.
function applyEmojiUI(): void {
  for (const compose of app.querySelectorAll<HTMLElement>('.chat-compose')) {
    if (compose.querySelector('[data-action="toggle-emoji"]')) continue;
    const footer = compose.querySelector<HTMLElement>('.compose-footer');
    if (!footer) continue;
    const toggle = document.createElement('button');
    toggle.type = 'button';
    toggle.className = 'outline emoji-toggle';
    toggle.dataset.action = 'toggle-emoji';
    toggle.textContent = '表情';
    footer.insertBefore(toggle, footer.firstChild);
    const panel = document.createElement('div');
    panel.className = 'emoji-panel';
    panel.hidden = true;
    panel.innerHTML = emojiPanelHtml();
    footer.after(panel);
  }
}

/// Loads sticker thumbnails only when a panel is opened, never on every render.
async function loadStickers(panel: HTMLElement): Promise<void> {
  if (!isTauri()) return;
  try {
    if (!stickersLoaded) { stickers = await invoke<string[]>('v1_list_stickers'); stickersLoaded = true; }
    for (const name of stickers) {
      if (stickerUrls.has(name)) continue;
      const bytes = new Uint8Array(await invoke<number[]>('v1_sticker_bytes', { name }));
      stickerUrls.set(name, URL.createObjectURL(new Blob([toArrayBuffer(bytes)])));
    }
  } catch { emojiNotice = '无法读取表情收藏'; }
  paintStickers(panel);
}

function paintStickers(panel: HTMLElement): void {
  const list = panel.querySelector<HTMLElement>('.sticker-list');
  const notice = panel.querySelector<HTMLElement>('.emoji-notice');
  if (notice) notice.textContent = emojiNotice;
  if (!list) return;
  if (!stickers.length) { list.textContent = '还没有收藏的表情，点击“添加…”选择 PNG、GIF、JPEG 或 WebP（≤512 KiB）。'; return; }
  list.innerHTML = stickers.map((name) => `<div class="sticker"><button type="button" data-action="use-sticker" data-name="${name}" title="发送此表情"><img src="${stickerUrls.get(name) ?? ''}" alt=""></button>${stickerManage ? `<button type="button" class="sticker-remove" data-action="remove-sticker" data-name="${name}" aria-label="移除表情">×</button>` : ''}</div>`).join('');
}

function insertAtCursor(input: HTMLTextAreaElement, text: string): void {
  const start = input.selectionStart ?? input.value.length;
  const end = input.selectionEnd ?? start;
  input.setRangeText(text, start, end, 'end');
  input.focus();
  input.dispatchEvent(new Event('input', { bubbles: true }));
}

async function sendSticker(name: string): Promise<void> {
  if (!isTauri()) return;
  if (!direct.peer) { emojiNotice = '请先打开一个单聊，再发送表情。'; paintEmojiNotices(); return; }
  try {
    const path = await invoke<string>('v1_sticker_path', { name });
    await direct.queuePaths([path]);
    emojiNotice = '表情已加入待发送队列。';
  } catch (error) { emojiNotice = displayError(error); }
  paintEmojiNotices();
}

async function addSticker(): Promise<void> {
  if (!isTauri()) return;
  const picked = await openFileDialog({ multiple: false, filters: [{ name: '表情图片', extensions: ['png', 'jpg', 'jpeg', 'gif', 'webp'] }] });
  if (!picked || typeof picked !== 'string') return;
  try {
    const name = await invoke<string>('v1_add_sticker', { path: picked });
    stickersLoaded = true;
    if (!stickers.includes(name)) stickers = [...stickers, name].sort();
    emojiNotice = '已加入我的表情。';
  } catch (error) { emojiNotice = displayError(error); }
  const panel = app.querySelector<HTMLElement>('.emoji-panel');
  if (panel) void loadStickers(panel);
}

async function removeSticker(name: string): Promise<void> {
  if (!isTauri()) return;
  try {
    await invoke('v1_remove_sticker', { name });
    stickers = stickers.filter((item) => item !== name);
    const url = stickerUrls.get(name);
    if (url) { URL.revokeObjectURL(url); stickerUrls.delete(name); }
    emojiNotice = '表情已移除。';
  } catch (error) { emojiNotice = displayError(error); }
  const panel = app.querySelector<HTMLElement>('.emoji-panel');
  if (panel) paintStickers(panel);
}

function paintEmojiNotices(): void {
  for (const notice of app.querySelectorAll<HTMLElement>('.emoji-notice')) notice.textContent = emojiNotice;
}

function sendHint(): string {
  return composer.send_shortcut === 'ctrl+enter' ? 'Ctrl+Enter 发送 · Enter 换行' : 'Enter 发送 · Shift+Enter 换行';
}

function isSendKey(event: KeyboardEvent): boolean {
  if (event.key !== 'Enter' || event.isComposing) return false;
  if (composer.send_shortcut === 'ctrl+enter') return event.ctrlKey && !event.shiftKey && !event.altKey && !event.metaKey;
  return !event.shiftKey && !event.ctrlKey && !event.altKey && !event.metaKey;
}

/// Applies composer preferences to the current DOM without a full re-render.
function applyComposerSettings(): void {
  for (const id of ['direct-input', 'group-input', 'self-input']) {
    const input = app.querySelector<HTMLTextAreaElement>('#' + id);
    if (input) input.rows = composer.rows;
  }
  for (const hint of app.querySelectorAll<HTMLElement>('.compose-footer > span')) hint.textContent = sendHint();
}

async function refreshScanState(): Promise<void> {
  if (!isTauri()) return;
  try {
    scanBlacklist = await invoke<string[]>('v1_scan_blacklist');
    scanBlacklistDraft = scanBlacklist.join(', ');
  } catch { scanBlacklistNotice = '无法读取扫描黑名单'; }
}

async function startScan(): Promise<void> {
  if (!isTauri() || scanBusy) return;
  const input = app.querySelector<HTMLInputElement>('#scan-cidr');
  const cidr = (input?.value ?? scanCidr).trim();
  scanCidr = cidr;
  const port = Number(probePort);
  if (!Number.isInteger(port) || port < 1 || port > 65535) { scanNotice = '请先在发现页设置 1–65535 的监听端口'; render(); return; }
  scanBusy = true; scanNotice = '正在扫描…'; scanHits = [];
  render();
  try {
    scanHits = await invoke<ScanHit[]>('v1_scan_range', { cidr, port });
    scanNotice = scanHits.length ? `发现 ${scanHits.length} 个候选，请用独立渠道核对完整指纹。` : '本次未发现候选；端口未开放或对方未启动服务。';
  } catch (error) { scanNotice = displayError(error); }
  finally { scanBusy = false; if (page === 'discover') render(); }
}

async function saveScanBlacklist(): Promise<void> {
  if (!isTauri()) return;
  const raw = app.querySelector<HTMLInputElement>('#scan-block')?.value ?? scanBlacklistDraft;
  const entries = raw.split(',').map((item) => item.trim()).filter(Boolean);
  try {
    scanBlacklist = await invoke<string[]>('v1_set_scan_blacklist', { entries });
    scanBlacklistDraft = scanBlacklist.join(', ');
    scanBlacklistNotice = '黑名单已保存。';
  } catch (error) { scanBlacklistNotice = displayError(error); }
  if (page === 'discover') render();
}

async function refreshNetwork(): Promise<void> {
  if (!isTauri()) return;
  try {
    networkSettings = await invoke<NetworkSettings>('v1_network_settings');
    probePort = String(networkSettings.default_port);
  } catch { networkNotice = '无法读取网络设置'; }
}

async function saveListenPort(): Promise<void> {
  if (!isTauri()) return;
  const port = Number(app.querySelector<HTMLInputElement>('#listen-port')?.value ?? '0');
  if (!Number.isInteger(port) || port < 1024 || port > 65535) { networkNotice = '监听端口需为 1024–65535'; if (page === 'settings') render(); return; }
  try {
    networkSettings = await invoke<NetworkSettings>('v1_set_network_settings', { port });
    probePort = String(networkSettings.default_port);
    networkNotice = '默认端口已保存；下次启动聊天服务时生效。';
  } catch (error) { networkNotice = displayError(error); }
  if (page === 'settings') render();
}

async function refreshComposer(): Promise<void> {
  if (!isTauri()) return;
  try { composer = await invoke<ComposerSettings>('v1_composer_settings'); }
  catch { composerNotice = '无法读取输入设置'; }
}

async function saveComposer(): Promise<void> {
  if (!isTauri()) return;
  const sendShortcut = app.querySelector<HTMLSelectElement>('#send-shortcut')?.value ?? 'enter';
  const rows = Number(app.querySelector<HTMLInputElement>('#composer-rows')?.value ?? '3');
  if (!Number.isInteger(rows) || rows < 2 || rows > 12) { composerNotice = '输入区高度需为 2–12 行'; if (page === 'settings') render(); return; }
  try {
    composer = await invoke<ComposerSettings>('v1_set_composer_settings', { sendShortcut, rows });
    composerNotice = '输入设置已保存。';
  } catch (error) { composerNotice = displayError(error); }
  if (page === 'settings') render();
}

type SettingsTab = 'general' | 'privacy' | 'workspace' | 'experimental';
let settingsTab: SettingsTab = 'general';
const SETTINGS_TABS: { id: SettingsTab; label: string }[] = [
  { id: 'general', label: '常规与网络' },
  { id: 'privacy', label: '隐私与安全' },
  { id: 'workspace', label: '工作区' },
  { id: 'experimental', label: '实验功能' },
];

function settingsView(): string {
  const tab = SETTINGS_TABS.find((item) => item.id === settingsTab) ?? SETTINGS_TABS[0];
  return `<section class="card settings-card">
    <span class="eyebrow">SETTINGS / 设置</span><h2>${tab.label}</h2>
    ${settingsTab === 'privacy' ? privacySettingsHtml() : settingsTab === 'workspace' ? workspaceSettingsHtml() : settingsTab === 'experimental' ? experimentalSettingsHtml() : generalSettingsHtml()}
  </section>`;
}

function generalSettingsHtml(): string {
  const used = storageInfo ? formatBytes(storageInfo.used) : '读取中…';
  const quota = storageInfo ? formatBytes(storageInfo.quota) : '读取中…';
  const quotaMib = storageInfo ? Math.round(storageInfo.quota / (1024 * 1024)) : '';
  return `<p>消息列表、联系人、发现与聊天功能已开放；插件与部分设置仍在后续阶段。</p>
    <div class="probe-box"><h3>存储配额</h3>
      <p>已接收附件占用 <strong id="storage-used">${used}</strong>，上限 <strong id="storage-quota">${quota}</strong>。</p>
      <label for="storage-quota-mib">配额上限（MiB，256–65536）</label>
      <div class="probe-row"><input id="storage-quota-mib" type="number" min="256" max="65536" value="${quotaMib}" ${storageBusy ? 'disabled' : ''}><button class="outline" data-action="save-storage-quota" ${storageBusy ? 'disabled' : ''}>保存配额</button></div>
      <p id="storage-notice" role="status">${storageNotice}</p>
      <small>配额只限制本机接收的附件；未完成的分片也计入占用。超过上限的新文件会被拒绝，已保存的记录不受影响。</small>
    </div>
    <div class="probe-box"><h3>监听端口</h3>
      <p>默认端口 <strong>${networkSettings.default_port}</strong>，在发现页启动聊天服务时使用；正在运行的服务不受保存影响。</p>
      <label for="listen-port">默认端口（1024–65535）</label>
      <div class="probe-row"><input id="listen-port" type="number" min="1024" max="65535" value="${networkSettings.default_port}"><button class="outline" data-action="save-listen-port">保存端口</button></div>
      <p id="network-notice" role="status">${networkNotice}</p>
      <small>端口被占用时启动会提示；修改只影响本机监听，不改变身份或信任状态。</small>
    </div>
    <p>本机身份指纹（SHA-256）：<br><code class="fingerprint">${fingerprint}</code></p>
    <button data-page="about" class="outline">关于 Qbird</button>`;
}

function privacySettingsHtml(): string {
  return `<div class="probe-box"><h3>应用锁</h3>
      <p>当前状态：${lockState.has_password ? '已设置密码' : '未设置密码（锁定只隐藏界面，任何人都能直接解锁）'}；空闲自动锁定：${lockState.idle_minutes ? `${lockState.idle_minutes} 分钟（按系统无键鼠输入计算）` : '关闭'}。</p>
      <label for="lock-current">当前应用锁密码${lockState.has_password ? '' : '（尚未设置，可留空）'}</label>
      <input id="lock-current" type="password" maxlength="128" autocomplete="current-password">
      <label for="lock-new">新密码（6–128 字符；留空并保存可移除密码）</label>
      <input id="lock-new" type="password" maxlength="128" autocomplete="new-password">
      <div class="probe-row"><button class="outline" data-action="save-lock-password" ${lockBusy ? 'disabled' : ''}>保存应用锁密码</button></div>
      <label for="lock-idle">空闲自动锁定（分钟，0 表示关闭，最大 240）</label>
      <div class="probe-row"><input id="lock-idle" type="number" min="0" max="240" value="${lockState.idle_minutes}" ${lockBusy ? 'disabled' : ''}><button class="outline" data-action="save-lock-idle" ${lockBusy ? 'disabled' : ''}>保存空闲设置</button></div>
      <p id="lock-notice" role="status">${lockNotice}</p>
      <small>应用锁只隐藏界面，不加密本地 SQLite 数据库；备份密码与应用锁密码相互独立。忘记应用锁密码时目前只能在本机设置中移除后重设；Windows 身份验证重置尚未实现。</small>
    </div>
    <div class="probe-box"><h3>聊天备份与恢复</h3>
      <p>备份使用密码加密并校验完整性；${backupInfo ? `已校验备份：${backupInfo.message_count} 条消息 · ${new Date(backupInfo.created_at * 1000).toLocaleString()}${backupInfo.include_chat ? ' · 含聊天记录' : ' · 不含聊天记录'}` : '尚未校验备份文件'}</p>
      <label for="backup-password">备份密码（8–256 字符）</label>
      <input id="backup-password" type="password" maxlength="256" autocomplete="new-password" value="${backupPassword.replace(/&/g, '&amp;').replace(/"/g, '&quot;').replace(/</g, '&lt;')}">
      <p><label><input id="backup-include-chat" type="checkbox" checked> 导出时包含聊天记录（取消后仅含联系人与群组）</label></p>
      <div class="probe-row"><button class="outline" data-action="backup-export" ${backupBusy ? 'disabled' : ''}>导出备份…</button><button class="outline" data-action="backup-choose" ${backupBusy ? 'disabled' : ''}>选择备份文件…</button><button class="outline" data-action="backup-inspect" ${backupBusy || !backupPath ? 'disabled' : ''}>校验备份</button></div>
      <p id="backup-path">${backupPath ? `当前文件：${backupPath}` : '合并恢复会跳过相同 ID 且内容一致的消息；覆盖恢复会先在本机保存一份恢复前快照。'}</p>
      <div class="probe-row"><button class="outline" data-action="backup-restore-merge" ${backupBusy || !backupPath ? 'disabled' : ''}>合并恢复</button><button class="outline danger" data-action="backup-restore-overwrite" ${backupBusy || !backupPath ? 'disabled' : ''}>覆盖恢复</button></div>
      <p id="backup-notice" role="status">${backupNotice}</p>
      <small>备份不包含身份私钥与附件文件；在新设备恢复会使用新身份，附件需另行拷贝。恢复不会改动本机存储配额。密码丢失后备份无法解密。</small>
    </div>`;
}

function workspaceSettingsHtml(): string {
  return `<div class="probe-box"><h3>输入与快捷键</h3>
      <label for="send-shortcut">发送方式</label>
      <select id="send-shortcut">
        <option value="enter" ${composer.send_shortcut === 'enter' ? 'selected' : ''}>Enter 发送，Shift+Enter 换行</option>
        <option value="ctrl+enter" ${composer.send_shortcut === 'ctrl+enter' ? 'selected' : ''}>Ctrl+Enter 发送，Enter 换行</option>
      </select>
      <label for="composer-rows">输入区高度（行，2–12）</label>
      <div class="probe-row"><input id="composer-rows" type="number" min="2" max="12" value="${composer.rows}"><button class="outline" data-action="save-composer">保存输入设置</button></div>
      <p id="composer-notice" role="status">${composerNotice}</p>
      <small>发送方式对单聊、群聊和自聊一致生效；输入区高度只影响本机界面，不随消息发送。</small>
    </div>
    <div class="probe-box"><h3>表情</h3>
      <p>常用表情和本地表情收藏在聊天输入区的“表情”按钮中管理；收藏的表情按图片文件发送给已确认联系人。</p>
      <small>本地收藏最多 64 个，单个不超过 512 KiB，支持 PNG、JPEG、GIF 与 WebP。</small>
    </div>
    <div class="probe-box"><h3>右侧工作区</h3>
      <p>文件预览面板可拖拽调宽，支持图片、TXT/日志/CSV/JSON、Markdown 与禁用脚本的 HTML。</p>
      <small>预览只读取已完成的本地附件，不会上传内容。</small>
    </div>`;
}

function experimentalSettingsHtml(): string {
  return `<div class="probe-box"><h3>实验功能</h3>
      <p><label><input id="experiment-group-file-quote" type="checkbox" ${experiments.group_file_quote ? 'checked' : ''}> 允许引用群文件消息（实验）</label></p>
      <p>开启后，群聊里可以引用文件消息并在气泡中显示文件名；关闭时文件消息不能引用。默认关闭，不影响普通成员 @。</p>
      <div class="probe-row"><button class="outline" data-action="save-experiment">保存实验设置</button></div>
      <p id="experiment-notice" role="status">${experimentNotice}</p>
      <small>插件接口与 Agent 能力仍未实现，不会显示为可操作开关。</small>
    </div>`;
}

async function takeScreenshot(): Promise<void> {
  if (!isTauri()) return;
  try {
    const path = await captureRegion();
    if (!path) return;
    if (direct.peer) await direct.queuePaths([path]);
    else if (groupChat.groupId) await groupChat.queuePaths([path]);
    else groupChat.notify('请先打开一个单聊或群聊再截图。');
  } catch (error) { groupChat.notify(displayError(error)); }
}

async function refreshExperiments(): Promise<void> {
  if (!isTauri()) return;
  try { experiments = await invoke<ExperimentSettings>('v1_experiment_settings'); }
  catch { experimentNotice = '无法读取实验设置'; }
}

async function saveExperiments(): Promise<void> {
  if (!isTauri()) return;
  const enabled = app.querySelector<HTMLInputElement>('#experiment-group-file-quote')?.checked ?? false;
  try {
    experiments = await invoke<ExperimentSettings>('v1_set_experiment_settings', { groupFileQuote: enabled });
    experimentNotice = enabled ? '群文件引用已开启。' : '群文件引用已关闭。';
  } catch (error) { experimentNotice = displayError(error); }
  if (page === 'settings') render();
}


async function refreshStorage(): Promise<void> {
  if (!isTauri()) return;
  try {
    const usage = await invoke<StorageUsage>('v1_storage_usage');
    storageInfo = usage;
    if (page === 'settings') {
      const used = app.querySelector<HTMLElement>('#storage-used');
      const quota = app.querySelector<HTMLElement>('#storage-quota');
      const input = app.querySelector<HTMLInputElement>('#storage-quota-mib');
      if (used) used.textContent = formatBytes(usage.used);
      if (quota) quota.textContent = formatBytes(usage.quota);
      if (input && document.activeElement !== input) input.value = String(Math.round(usage.quota / (1024 * 1024)));
    }
  } catch { if (page === 'settings') storageNotice = '无法读取存储占用'; }
}

async function saveQuota(): Promise<void> {
  const input = app.querySelector<HTMLInputElement>('#storage-quota-mib');
  const notice = app.querySelector<HTMLElement>('#storage-notice');
  if (!isTauri() || !input || storageBusy) return;
  const mib = Number(input.value);
  if (!Number.isInteger(mib) || mib < 256 || mib > 65536) { if (notice) notice.textContent = '请输入 256–65536 之间的整数（MiB）'; return; }
  storageBusy = true; if (notice) notice.textContent = '正在保存…';
  try {
    storageInfo = await invoke<StorageUsage>('v1_set_storage_quota', { bytes: mib * 1024 * 1024 });
    storageNotice = '配额已保存；超过上限的新文件将被拒绝接收。';
  } catch (error) { storageNotice = typeof error === 'string' ? error : '无法保存配额'; }
  finally {
    storageBusy = false;
    if (page === 'settings') { render(); }
  }
}

async function exportBackup(): Promise<void> {
  if (!isTauri() || backupBusy) return;
  if (backupPassword.length < 8) { backupNotice = '备份密码至少 8 个字符'; if (page === 'settings') render(); return; }
  const includeChat = app.querySelector<HTMLInputElement>('#backup-include-chat')?.checked ?? true;
  const date = new Date();
  const stamp = `${date.getFullYear()}${String(date.getMonth() + 1).padStart(2, '0')}${String(date.getDate()).padStart(2, '0')}`;
  const target = await saveFileDialog({
    title: '导出 Qbird 备份',
    defaultPath: `qbird-backup-${stamp}.qbird`,
    filters: [{ name: 'Qbird 备份', extensions: ['qbird'] }],
  }).catch(() => null);
  if (!target) return;
  backupBusy = true; backupNotice = '正在生成备份…';
  try {
    const info = await invoke<BackupInfo>('v1_create_backup', { password: backupPassword, path: target, includeChat });
    backupInfo = info; backupPath = target;
    backupNotice = `备份已生成：${info.message_count} 条消息。请妥善保管密码。`;
  } catch (error) { backupNotice = typeof error === 'string' ? error : '导出备份失败'; }
  finally { backupBusy = false; if (page === 'settings') render(); }
}

async function chooseBackup(): Promise<void> {
  if (!isTauri() || backupBusy) return;
  const selected = await openFileDialog({
    title: '选择 Qbird 备份',
    multiple: false,
    filters: [{ name: 'Qbird 备份', extensions: ['qbird'] }],
  }).catch(() => null);
  if (!selected || Array.isArray(selected)) return;
  backupPath = selected; backupInfo = null;
  backupNotice = '已选择备份文件；输入密码后可校验或恢复。';
  if (page === 'settings') render();
}

async function inspectBackup(): Promise<void> {
  if (!isTauri() || backupBusy || !backupPath) return;
  backupBusy = true; backupNotice = '正在校验…';
  try {
    backupInfo = await invoke<BackupInfo>('v1_inspect_backup', { password: backupPassword, path: backupPath });
    backupNotice = '密码正确，备份完整性校验通过。';
  } catch (error) { backupInfo = null; backupNotice = typeof error === 'string' ? error : '校验失败'; }
  finally { backupBusy = false; if (page === 'settings') render(); }
}

async function restoreBackup(overwrite: boolean): Promise<void> {
  if (!isTauri() || backupBusy || !backupPath) return;
  if (overwrite && !window.confirm('覆盖恢复会先保存一份恢复前快照，然后用备份替换联系人与聊天数据。确认继续？')) return;
  backupBusy = true; backupNotice = overwrite ? '正在覆盖恢复…' : '正在合并恢复…';
  try {
    const report = await invoke<RestoreReport>('v1_restore_backup', { password: backupPassword, path: backupPath, overwrite });
    backupNotice = report.conflicts > 0
      ? `发现 ${report.conflicts} 条相同 ID 但内容不同的消息，已中止恢复，未修改任何数据。`
      : `恢复完成：${report.mode === 'overwrite' ? '覆盖' : '合并'}，新增 ${report.inserted} 条消息${report.skipped ? `，跳过 ${report.skipped} 条已有消息` : ''}。${report.snapshot_path ? '恢复前快照已保存在本机数据目录。' : ''}`;
    await refreshConversations();
  } catch (error) { backupNotice = typeof error === 'string' ? error : '恢复失败，现有数据未被修改'; }
  finally { backupBusy = false; if (page === 'settings') render(); }
}

function discoveryView(): string {
  return `<section class="discovery-panel card">
    <span class="eyebrow">DISCOVERY / 发现</span><h2>局域网身份探测</h2>
    <p>探测仅证明对方持有密钥，不会自动建立信任。双方须分别通过独立可靠渠道核对完整指纹。</p>
    <div class="probe-box"><label for="probe-port">本机监听端口</label><div class="probe-row"><input id="probe-port" type="number" min="1" max="65535" aria-label="本机监听端口"><button class="outline" data-action="probe-start">启动聊天和探测</button><button class="outline" data-action="probe-stop">停止</button></div>
    <p id="probe-status" aria-live="polite"></p><small>启动时使用设置页保存的默认端口，此处可临时修改；启动后提供探测、聊天及待发消息重试。停止后待发消息保留在本机。</small></div>
    <div class="probe-box"><label for="probe-address">手动地址</label><div class="probe-row"><input id="probe-address" type="text" placeholder="192.168.1.8:42853" aria-label="手动地址"><button class="outline" data-action="probe-manual">探测指纹</button></div><p id="probe-message" aria-live="polite"></p>
    <p>本机指纹：<br><code id="local-fingerprint" class="fingerprint"></code></p><p>对方指纹（未确认）：<br><span id="remote-target"></span><br><code id="remote-fingerprint" class="fingerprint"></code></p><button class="outline" data-action="select-outbound">选择此指纹核对</button></div>
    <p id="discovery-status" aria-live="polite"></p><ul id="discovery-results" class="discovery-results"></ul>
    <h3>收到的探测请求（未确认）</h3><ul id="probe-requests" class="discovery-results"></ul>
    <div class="probe-box"><h3>扫描其他网段</h3>
      <p>mDNS 只覆盖本网段；可对 /24–/32 的 IPv4 网段限速探测身份。扫描结果只是候选，不代表可信。</p>
      <label for="scan-cidr">网段（CIDR）</label>
      <input id="scan-cidr" type="text" maxlength="24" spellcheck="false" placeholder="192.168.1.0/24" value="${scanCidr.replace(/"/g, '&quot;')}">
      <div class="probe-row"><button class="outline" data-action="scan-start" ${scanBusy ? 'disabled' : ''}>${scanBusy ? '扫描中…' : '开始扫描'}</button></div>
      <p id="scan-status" role="status">${scanNotice}</p>
      <ul class="discovery-results">${scanHits.map((hit) => `<li><span>${hit.address} · <code>${hit.fingerprint.slice(0, 16)}…</code></span><button class="outline" data-action="probe-scanned" data-address="${hit.address}">核对指纹</button></li>`).join('')}</ul>
      <label for="scan-block">黑名单（IP 或 CIDR，逗号分隔）</label>
      <input id="scan-block" type="text" maxlength="512" spellcheck="false" placeholder="192.168.1.20, 10.0.0.0/24" value="${scanBlacklistDraft.replace(/"/g, '&quot;')}">
      <div class="probe-row"><button class="outline" data-action="save-scan-blacklist">保存黑名单</button></div>
      <p id="scan-blacklist-notice" role="status">${scanBlacklistNotice}</p>
      <small>扫描并发 4、单地址超时 400 毫秒，仅用于发现候选；黑名单中的地址不会被扫描。</small>
    </div>
    <div class="probe-box"><h3>确认联系人</h3><p>所选指纹：<br><code id="selected-fingerprint" class="fingerprint"></code></p>
    <p>请通过当面或其他可靠渠道取得对方的完整指纹并输入；不要仅根据此页面显示的结果确认。对方也需要独立确认你的指纹。</p>
    <label for="checked-fingerprint">对方通过可靠渠道提供的完整指纹</label><input id="checked-fingerprint" type="text" autocomplete="off" spellcheck="false">
    <label for="contact-name">联系人名称</label><input id="contact-name" type="text" maxlength="128" autocomplete="off">
    <p><label><input id="checked-independent" type="checkbox"> 我已通过独立可靠渠道核对完整指纹</label></p>
    <button class="outline" data-action="confirm-contact">确认联系人</button><p id="confirm-message" aria-live="polite"></p></div>
  </section>`;
}

function selectFingerprint(value: string): void {
  selectedFingerprint = value;
  for (const id of ['checked-fingerprint', 'contact-name']) {
    const input = app.querySelector<HTMLInputElement>(`#${id}`); if (input) input.value = '';
  }
  const checkbox = app.querySelector<HTMLInputElement>('#checked-independent'); if (checkbox) checkbox.checked = false;
  const message = app.querySelector<HTMLElement>('#confirm-message'); if (message) message.textContent = '';
  paintDiscovery();
}

async function confirmContact(): Promise<void> {
  const message = app.querySelector<HTMLElement>('#confirm-message');
  const checked = app.querySelector<HTMLInputElement>('#checked-fingerprint')?.value.trim() || '';
  const name = app.querySelector<HTMLInputElement>('#contact-name')?.value.trim() || '';
  const independent = app.querySelector<HTMLInputElement>('#checked-independent')?.checked;
  if (!message || !isTauri()) return;
  if (!selectedFingerprint || checked !== selectedFingerprint || !independent || !name) {
    message.textContent = '请选择探测结果，填写名称和独立渠道取得的完整匹配指纹，并勾选确认。'; return;
  }
  const button = app.querySelector<HTMLButtonElement>('button[data-action="confirm-contact"]');
  if (button?.disabled) return;
  if (button) button.disabled = true;
  try {
    await invoke('v1_confirm_contact', { fingerprint: selectedFingerprint, checkedFingerprint: checked, displayName: name });
    message.textContent = '联系人已在本机确认；对方仍须单独确认你的指纹。';
  } catch (error) { message.textContent = displayError(error); }
  finally { if (button) button.disabled = false; }
}

async function saveContactProfile(peer: string): Promise<void> {
  const remark = app.querySelector<HTMLInputElement>(`input[data-remark-for="${peer}"]`)?.value ?? '';
  const category = app.querySelector<HTMLInputElement>(`input[data-category-for="${peer}"]`)?.value ?? '';
  const status = app.querySelector<HTMLElement>('#contacts-status');
  if (!isTauri()) return;
  try {
    await invoke('v1_set_contact_profile', { peer, remark, category });
    if (status) status.textContent = '备注与分组已保存（仅本机可见，不影响信任）。';
    await refreshContacts();
  } catch (error) { if (status) status.textContent = typeof error === 'string' ? error : '无法保存备注或分组'; }
}

async function refreshContacts(): Promise<void> {
  const status = app.querySelector<HTMLElement>('#contacts-status');
  const list = app.querySelector<HTMLUListElement>('#contacts-list');
  if (!status || !list) return;
  if (!isTauri()) { status.textContent = '仅桌面应用支持联系人'; return; }
  try {
    const [contacts, groupList] = await Promise.all([
      contactQuery ? invoke<Contact[]>('v1_search_contacts', { query: contactQuery, limit: 100 }) : invoke<Contact[]>('v1_list_contacts'),
      invoke<Group[]>('v1_list_groups'),
    ]);
    groups = groupList;
    if (!list.isConnected) return;
    list.replaceChildren();
    status.textContent = contactQuery
      ? (contacts.length ? `匹配 ${contacts.length} 位联系人` : '没有匹配的联系人')
      : (contacts.length ? `其他联系人：${contacts.length}` : '暂无其他联系人，可前往发现页面核对指纹。');
    paintGroups(contacts);
    for (const contact of contacts) {
      const item = document.createElement('li');
      const title = document.createElement('span');
      title.className = 'contact-title';
      title.textContent = `${contact.remark ? `${contact.remark}（${contact.display_name}）` : contact.display_name} · ${contact.trust_state === 'verified' ? '已确认' : '待确认'}${contact.category ? ` · ${contact.category}` : ''} · ${contact.fingerprint}`;
      item.append(title);
      const actions = document.createElement('div'); actions.className = 'contact-actions';
      if (contact.trust_state === 'verified') {
        const button = document.createElement('button'); button.className = 'outline'; button.textContent = '打开单聊';
        button.dataset.action = 'open-direct'; button.dataset.peer = contact.fingerprint; button.dataset.name = contact.remark || contact.display_name;
        actions.append(button);
      }
      const remark = document.createElement('input');
      remark.type = 'text'; remark.maxLength = 64; remark.placeholder = '备注'; remark.value = contact.remark ?? '';
      remark.dataset.remarkFor = contact.fingerprint; remark.setAttribute('aria-label', '联系人备注');
      const category = document.createElement('input');
      category.type = 'text'; category.maxLength = 64; category.placeholder = '分组'; category.value = contact.category ?? '';
      category.dataset.categoryFor = contact.fingerprint; category.setAttribute('aria-label', '联系人分组');
      const save = document.createElement('button'); save.className = 'outline'; save.textContent = '保存备注/分组';
      save.dataset.action = 'save-contact-profile'; save.dataset.peer = contact.fingerprint;
      actions.append(remark, category, save);
      item.append(actions);
      list.append(item);
    }
  } catch { if (status.isConnected) status.textContent = '无法读取联系人'; }
}

function paintGroups(contacts: Contact[]): void {
  const list = app.querySelector<HTMLUListElement>('#groups-list');
  const pick = app.querySelector<HTMLElement>('#group-members-pick');
  if (list) {
    list.replaceChildren();
    if (!groups.length) {
      const empty = document.createElement('li');
      empty.textContent = '还没有群聊。选择一个名称和至少一位已确认联系人即可创建。';
      list.append(empty);
    }
    for (const group of groups) {
      const item = document.createElement('li');
      item.textContent = `${group.name} · ${group.member_count} 人${group.is_owner ? ' · 我创建的' : ''}`;
      const open = document.createElement('button');
      open.className = 'outline'; open.textContent = '打开群聊';
      open.dataset.action = 'open-group'; open.dataset.group = group.id; open.dataset.name = group.name;
      item.append(open); list.append(item);
    }
  }
  if (pick) {
    pick.replaceChildren();
    const verified = contacts.filter(contact => contact.trust_state === 'verified');
    if (!verified.length) { pick.textContent = '需要先在发现页面确认至少一位联系人。'; return; }
    for (const contact of verified) {
      const label = document.createElement('label');
      const box = document.createElement('input');
      box.type = 'checkbox'; box.dataset.groupMember = contact.fingerprint;
      label.append(box, document.createTextNode(` ${contact.display_name}（${contact.fingerprint.slice(0, 8)}）`));
      pick.append(label);
    }
  }
}

async function createGroup(): Promise<void> {
  const input = app.querySelector<HTMLInputElement>('#group-name');
  const status = app.querySelector<HTMLElement>('#group-create-status');
  if (!isTauri() || !input || !status) return;
  const name = input.value.trim();
  const members = [...app.querySelectorAll<HTMLInputElement>('input[data-group-member]:checked')]
    .map(node => node.dataset.groupMember || '').filter(Boolean);
  if (!name) { status.textContent = '请输入群聊名称'; return; }
  if (!members.length) { status.textContent = '请至少选择一位已确认联系人'; return; }
  try {
    const created = await invoke<Group>('v1_create_group', { name, members });
    input.value = '';
    for (const node of app.querySelectorAll<HTMLInputElement>('input[data-group-member]')) node.checked = false;
    status.textContent = '群聊已创建；邀请会在对方在线时送达。';
    selfOpen = false; page = 'messages';
    await groupChat.open(created.id, created.name);
  } catch (error) { status.textContent = displayError(error); }
}

function render(): void {
  if (lockState.locked) { app.innerHTML = lockView(); return; }
  const current = pages[page];
  app.innerHTML = `
    <div class="shell">
      <aside class="rail" aria-label="主导航">
        <button class="profile" data-page="profile" aria-label="我的资料" title="我的资料">${avatarMarkup()}</button>
        <nav class="rail-nav" aria-label="主要功能">
          <button data-page="messages" class="${page === 'messages' ? 'active' : ''}" aria-label="消息" title="消息">${icon('messages')}</button>
          <button data-page="contacts" class="${page === 'contacts' ? 'active' : ''}" aria-label="联系人" title="联系人">${icon('contacts')}</button>
          <button data-page="discover" class="${page === 'discover' ? 'active' : ''}" aria-label="发现" title="发现">${icon('discover')}</button>
        </nav>
        <span class="rail-brand">Qbird</span>
        <button class="rail-bottom" data-action="lock-now" aria-label="临时离开锁定" title="临时离开锁定">${icon('lock')}</button>
        <button class="rail-bottom" data-action="menu" aria-label="更多" aria-expanded="${menuOpen}" title="更多">${icon('menu')}</button>
      </aside>
      ${menuOpen ? '<div class="menu" role="menu"><button data-page="settings" role="menuitem">设置</button><button data-page="about" role="menuitem">关于</button><span>备份与还原 · 待实现</span></div>' : ''}
      <aside class="sidebar">
        <div class="team"><span class="team-mark">${icon('spark')}</span><div><strong>Qbird 青鸟</strong><small>LOCAL NETWORK MESSENGER</small></div></div>
        <div class="side-heading"><div><span class="eyebrow">${current.eyebrow}</span><h1>${current.title}</h1></div></div>
        ${page === 'settings' || page === 'about' ? `<div class="settings-links">${SETTINGS_TABS.map((tab) => `<button data-settings-tab="${tab.id}" class="${page === 'settings' && settingsTab === tab.id ? 'selected' : ''}">${tab.label}</button>`).join('')}<button data-page="about" class="${page === 'about' ? 'selected' : ''}">关于</button></div>` : page === 'messages' ? `<div class="side-search"><input id="global-search" type="search" maxlength="200" placeholder="搜索全部消息" aria-label="搜索全部消息" value="${searchQuery.replace(/&/g, '&amp;').replace(/"/g, '&quot;').replace(/</g, '&lt;')}"></div><div id="search-results" class="search-hits" hidden></div><div id="conversation-list" class="conversation-list">正在读取…</div>` : '<div class="side-empty">暂无内容</div>'}
        <div class="side-foot"><span class="dot"></span>开发中 · 本地桌面版</div>
      </aside>
      <main class="workspace">
        ${page === 'messages' && groupChat.groupId ? groupChat.view() : page === 'messages' && direct.peer ? direct.view() : page === 'messages' && selfOpen ? selfChatView() : page === 'discover' ? discoveryView() : page === 'settings' ? settingsView() : page === 'profile' ? profileView() : page === 'contacts' ? `<section class="discovery-panel card"><h2>联系人</h2><button class="self-contact" data-action="open-self" ${isTauri() ? '' : 'disabled'}><strong>自己</strong><span>打开本地自聊</span></button><p>已确认的联系人保存在本机；双方须各自核对指纹后才能聊天。</p><label for="contact-search">搜索联系人</label><input id="contact-search" type="search" maxlength="64" placeholder="名称、备注、分组或指纹" value="${contactQuery.replace(/&/g, '&amp;').replace(/"/g, '&quot;').replace(/</g, '&lt;')}"><p id="contacts-status" aria-live="polite"></p><ul id="contacts-list" class="discovery-results"></ul><div class="probe-box"><h3>我的群聊</h3><ul id="groups-list" class="discovery-results"></ul><label for="group-name">新建群聊名称</label><input id="group-name" type="text" maxlength="128" placeholder="例如：项目组"><div id="group-members-pick" class="group-pick"></div><button class="outline" data-action="create-group">创建群聊</button><p id="group-create-status" aria-live="polite"></p><small>只能邀请已确认的联系人；新成员只能看到加入之后的消息。</small></div></section>` : page === 'about' ? `<section class="card about"><span class="eyebrow">ABOUT / 关于</span><h2>Qbird 青鸟</h2><p>面向局域网的本地优先聊天与文件交流工具。</p><dl><div><dt>当前阶段</dt><dd>Windows 开发版 ${appVersion}</dd></div><div><dt>许可证</dt><dd>Apache License 2.0</dd></div><div><dt>平台规划</dt><dd>Windows 首发 · 后续 Linux / macOS</dd></div></dl><p class="notice">开发版未经安全审计，本地数据库未加密。</p></section>` : `<section class="empty"><div class="empty-mark">${icon('spark')}</div><h2>${current.heading}</h2><p>${current.copy}</p></section>`}
      </main>
    </div>`;
  if (page === 'discover') paintDiscovery();
  if (page === 'settings') void refreshStorage();
  if (page === 'profile') void refreshProfile();
  if (page === 'messages') {
    void refreshConversations();
    paintSearchList();
    if (direct.peer) direct.render();
    if (selfOpen) {
      const input = app.querySelector<HTMLTextAreaElement>('#self-input'); if (input) input.value = selfDraft;
      paintSelfMessages('bottom');
    }
  }
  applyComposerSettings();
  applyEmojiUI();
}

function paintDiscovery(): void {
  const status = app.querySelector<HTMLElement>('#discovery-status');
  const list = app.querySelector<HTMLUListElement>('#discovery-results');
  if (!status || !list) return;
  status.textContent = discoveryStatus;
  list.replaceChildren();
  for (const candidate of candidates) {
    const item = document.createElement('li');
    // mDNS data is untrusted; never insert it as HTML.
    const label = document.createElement('span');
    label.textContent = `${candidate.service_name}（未验证）`;
    item.append(label);
    for (const address of candidate.addresses) {
      const button = document.createElement('button');
      button.className = 'outline';
      button.textContent = `探测 ${address}`;
      button.dataset.action = 'probe-candidate';
      button.dataset.address = address;
      button.disabled = !isTauri() || probeBusy;
      item.append(button);
    }
    list.append(item);
  }
  const port = app.querySelector<HTMLInputElement>('#probe-port');
  const address = app.querySelector<HTMLInputElement>('#probe-address');
  if (port && document.activeElement !== port) port.value = probePort;
  if (address && document.activeElement !== address) address.value = manualAddress;
  const setText = (selector: string, value: string) => { const node = app.querySelector<HTMLElement>(selector); if (node) node.textContent = value; };
  setText('#probe-status', probeNotice);
  setText('#probe-message', probeMessage);
  setText('#local-fingerprint', fingerprint);
  setText('#remote-fingerprint', remoteFingerprint || '暂无');
  setText('#remote-target', observedAddress);
  setText('#selected-fingerprint', selectedFingerprint || '请先选择探测结果');
  const requests = app.querySelector<HTMLUListElement>('#probe-requests');
  if (requests) {
    requests.replaceChildren();
    for (const request of probeStatus.requests) {
      const item = document.createElement('li');
      item.textContent = `${request.remote_ip} · ${request.fingerprint}（未确认）`;
      const button = document.createElement('button');
      button.className = 'outline'; button.textContent = '选择此指纹核对';
      button.dataset.action = 'select-inbound'; button.dataset.fingerprint = request.fingerprint;
      button.disabled = !isTauri() || probeBusy;
      item.append(button);
      requests.append(item);
    }
  }
  const start = app.querySelector<HTMLButtonElement>('button[data-action="probe-start"]');
  const stop = app.querySelector<HTMLButtonElement>('button[data-action="probe-stop"]');
  const manual = app.querySelector<HTMLButtonElement>('button[data-action="probe-manual"]');
  const selectOutbound = app.querySelector<HTMLButtonElement>('button[data-action="select-outbound"]');
  const confirm = app.querySelector<HTMLButtonElement>('button[data-action="confirm-contact"]');
  if (start) start.disabled = !isTauri() || probeBusy || probeStatus.port !== null;
  if (stop) stop.disabled = !isTauri() || probeBusy || probeStatus.port === null;
  if (manual) manual.disabled = !isTauri() || probeBusy;
  if (selectOutbound) selectOutbound.disabled = !isTauri() || probeBusy || !remoteFingerprint || remoteFingerprint === fingerprint;
  if (confirm && !isTauri()) confirm.disabled = true;
  if (port) port.disabled = !isTauri() || probeBusy || probeStatus.port !== null;
  if (address) address.disabled = !isTauri() || probeBusy;
}

async function refreshDiscovery(): Promise<void> {
  if (!isTauri() || page !== 'discover' || discoveryLoading) return;
  discoveryLoading = true;
  try {
    candidates = await invoke<Candidate[]>('v1_discovery_snapshot');
    discoveryStatus = candidates.length ? `发现 ${candidates.length} 个候选服务` : '暂未发现候选服务';
  } catch {
    candidates = [];
    discoveryStatus = '局域网发现不可用，请检查网络和防火墙';
  } finally {
    discoveryLoading = false;
    if (page === 'discover') paintDiscovery();
  }
}

async function refreshProbeStatus(): Promise<void> {
  if (!isTauri() || page !== 'discover') return;
  try {
    probeStatus = await invoke<ProbeStatus>('v1_probe_status');
    probeNotice = probeStatus.running ? `聊天与探测监听中：TCP ${probeStatus.port}` : probeStatus.port !== null ? '监听异常停止，请先停止再重试' : '聊天与探测服务未启动';
  } catch { probeNotice = '无法读取探测服务状态'; }
  if (page === 'discover') paintDiscovery();
}

function lockView(): string {
  return `<div class="lock-screen"><section class="card lock-card" aria-label="应用已锁定">
    <span class="eyebrow">LOCKED / 已锁定</span><h2>Qbird 青鸟</h2>
    ${lockState.has_password
      ? '<label for="lock-password">应用锁密码</label><input id="lock-password" type="password" maxlength="128" autocomplete="current-password">'
      : '<p>此应用锁未设置密码，点击下方按钮返回。</p>'}
    <div class="probe-row"><button data-action="unlock" ${lockBusy ? 'disabled' : ''}>解锁</button></div>
    <p id="lock-status" role="status">${lockNotice}</p>
    <small>应用锁只隐藏界面，不加密本地数据库。</small>
  </section></div>`;
}

async function initLock(): Promise<void> {
  if (!isTauri()) { lockInitialized = true; return; }
  try { lockState = await invoke<LockStatus>('v1_lock_status'); }
  catch { lockNotice = '无法读取应用锁状态'; }
  lockInitialized = true;
  if (lockState.locked) render();
}

async function lockNow(): Promise<void> {
  if (!isTauri()) return;
  try { lockState = await invoke<LockStatus>('v1_lock_now'); lockNotice = ''; render(); }
  catch { /* Keep the current screen when the backend is unavailable. */ }
}

/// Automatic locking prefers OS idle time, so Qbird still locks while unfocused or
/// minimized; the local activity clock is only a fallback when the system cannot report.
async function checkIdleLock(): Promise<void> {
  if (!lockInitialized || lockState.locked || lockState.idle_minutes <= 0) return;
  let idleSeconds: number | null = null;
  if (isTauri()) {
    try { idleSeconds = await invoke<number | null>('v1_system_idle_seconds'); } catch { idleSeconds = null; }
  }
  const idle = idleSeconds ?? (Date.now() - lastActivity) / 1000;
  if (idle >= lockState.idle_minutes * 60) void lockNow();
}

async function unlockApp(): Promise<void> {
  if (!isTauri() || lockBusy) return;
  const password = app.querySelector<HTMLInputElement>('#lock-password')?.value ?? '';
  lockBusy = true;
  try {
    lockState = await invoke<LockStatus>('v1_unlock', { password: password || null });
    lockNotice = ''; lastActivity = Date.now();
  } catch (error) { lockNotice = typeof error === 'string' ? error : '解锁失败'; }
  finally { lockBusy = false; render(); }
}

async function saveLockPassword(): Promise<void> {
  if (!isTauri() || lockBusy) return;
  const current = app.querySelector<HTMLInputElement>('#lock-current')?.value ?? '';
  const next = app.querySelector<HTMLInputElement>('#lock-new')?.value ?? '';
  lockBusy = true;
  try {
    lockState = await invoke<LockStatus>('v1_set_lock_password', { current: current || null, next });
    lockNotice = next ? '应用锁密码已保存；下次启动或临时锁定时需要输入。' : '应用锁密码已移除。';
  } catch (error) { lockNotice = typeof error === 'string' ? error : '无法保存应用锁密码'; }
  finally { lockBusy = false; if (page === 'settings') render(); }
}

async function saveLockIdle(): Promise<void> {
  if (!isTauri() || lockBusy) return;
  const input = app.querySelector<HTMLInputElement>('#lock-idle');
  const minutes = Number(input?.value ?? '0');
  if (!Number.isInteger(minutes) || minutes < 0 || minutes > 240) { lockNotice = '请输入 0–240 之间的整数分钟'; if (page === 'settings') render(); return; }
  lockBusy = true;
  try {
    lockState = await invoke<LockStatus>('v1_set_lock_idle', { minutes });
    lockNotice = minutes ? `无操作 ${minutes} 分钟后将自动锁定。` : '空闲自动锁定已关闭。';
  } catch (error) { lockNotice = typeof error === 'string' ? error : '无法保存空闲锁定设置'; }
  finally { lockBusy = false; if (page === 'settings') render(); }
}

function scheduleSearch(): void {
  paintSearchList();
  if (searchTimer !== null) window.clearTimeout(searchTimer);
  if (!searchQuery.trim()) { searchHits = []; searchBusy = false; searchError = ''; paintSearchList(); return; }
  searchTimer = window.setTimeout(() => { void runSearch(); }, 300);
}

async function runSearch(): Promise<void> {
  if (!isTauri()) return;
  const query = searchQuery.trim();
  if (!query) return;
  const sequence = ++searchSeq;
  searchBusy = true; searchError = ''; paintSearchList();
  try {
    const hits = await invoke<SearchHit[]>('v1_search_all', { query, category: 'all', limit: 100 });
    if (sequence === searchSeq) searchHits = hits;
  } catch {
    if (sequence === searchSeq) { searchHits = []; searchError = '搜索失败，请稍后重试'; }
  } finally {
    if (sequence === searchSeq) { searchBusy = false; paintSearchList(); }
  }
}

function paintSearchList(): void {
  const results = app.querySelector<HTMLElement>('#search-results');
  const list = app.querySelector<HTMLElement>('#conversation-list');
  if (!results || !list) return;
  const active = searchQuery.trim().length > 0;
  results.hidden = !active;
  list.hidden = active;
  results.replaceChildren();
  if (!active) return;
  if (searchBusy) { results.textContent = '正在搜索…'; return; }
  if (searchError) { results.textContent = searchError; return; }
  if (!searchHits.length) { results.textContent = '没有匹配的消息'; return; }
  for (const hit of searchHits) {
    const button = document.createElement('button');
    button.className = 'search-hit';
    button.dataset.action = 'open-hit';
    button.dataset.kind = hit.conversation_kind;
    button.dataset.id = hit.conversation_id;
    button.dataset.message = hit.message_id;
    button.dataset.name = hit.conversation_name;
    const head = document.createElement('strong');
    const kindLabel = hit.conversation_kind === 'self' ? '自聊' : hit.conversation_kind === 'direct' ? '单聊' : '群聊';
    head.textContent = `${kindLabel} · ${hit.conversation_name}`;
    const body = document.createElement('span');
    body.textContent = `${hit.sender_name || ''}：${hit.body}`;
    const time = document.createElement('small');
    time.textContent = new Date(hit.created_at * 1000).toLocaleString();
    button.append(head, body, time);
    results.append(button);
  }
}

async function openSearchHit(kind: string, id: string, messageId: string, name: string): Promise<void> {
  if (!isTauri()) return;
  searchQuery = ''; searchHits = []; searchBusy = false; searchError = '';
  if (kind === 'self') {
    await openSelf();
    await jumpSelf(messageId);
    return;
  }
  if (kind === 'direct') {
    direct.clear(); groupChat.clear(); selfOpen = false; page = 'messages';
    await direct.open(id, name || id.slice(0, 8));
    await direct.jumpTo(messageId);
    return;
  }
  if (kind === 'group') {
    direct.clear(); selfOpen = false; page = 'messages';
    await groupChat.open(id, name || '群聊');
    await groupChat.jumpTo(messageId);
  }
}

async function jumpSelf(messageId: string): Promise<void> {
  const found = () => selfMessages.some(message => message.id === messageId);
  for (let page = 0; page < 20 && !found() && selfHasOlder; page += 1) {
    const before = selfMessages.length;
    await loadSelfMessages(true);
    if (selfMessages.length === before) break;
  }
  const node = app.querySelector<HTMLElement>(`#self-messages [data-id="${messageId}"]`);
  if (!node) { const status = app.querySelector<HTMLElement>('#self-error'); if (status) status.textContent = '该消息暂不在本机记录中'; return; }
  node.scrollIntoView({ block: 'center' });
  node.classList.add('jump-target');
  window.setTimeout(() => node.classList.remove('jump-target'), 1600);
}

type ExportResult = { conversation_name: string; message_count: number; path: string };

async function exportChat(kind: 'self' | 'direct' | 'group'): Promise<void> {
  if (!isTauri()) return;
  const id = kind === 'self' ? 'self' : kind === 'direct' ? direct.peer : groupChat.groupId;
  const status = kind === 'self' ? '#self-error' : kind === 'direct' ? '#direct-file-status' : '#group-error';
  if (!id) return;
  if (!window.confirm('导出的纯文本未加密，任何能打开该文件的人都能读到内容。确认导出？')) return;
  const date = new Date();
  const stamp = `${date.getFullYear()}${String(date.getMonth() + 1).padStart(2, '0')}${String(date.getDate()).padStart(2, '0')}`;
  const target = await saveFileDialog({
    title: '导出聊天记录',
    defaultPath: `qbird-chat-${stamp}.txt`,
    filters: [{ name: '纯文本', extensions: ['txt'] }],
  }).catch(() => null);
  if (!target) return;
  const report = (value: string) => { const node = app.querySelector<HTMLElement>(status); if (node) node.textContent = value; };
  try {
    const result = await invoke<ExportResult>('v1_export_chat', { kind, id, path: target });
    report(`已导出 ${result.message_count} 条消息到 ${result.path}`);
  } catch (error) { report(typeof error === 'string' ? error : '导出失败'); }
}

function displayError(error: unknown): string {
  return typeof error === 'string' && error.length <= 200 ? error : '操作失败，请检查网络后重试';
}

async function setProbeService(start: boolean): Promise<void> {
  if (!isTauri() || probeBusy) return;
  const port = Number(probePort);
  if (start && (!Number.isInteger(port) || port < 1 || port > 65535)) {
    probeMessage = '请输入 1–65535 的端口'; paintDiscovery(); return;
  }
  probeBusy = true; probeMessage = ''; paintDiscovery();
  try {
    if (start) await invoke('v1_start_chat_service', { port });
    else await invoke('v1_stop_probe_service');
    await refreshProbeStatus();
  } catch (error) { probeMessage = displayError(error); }
  finally { probeBusy = false; if (page === 'discover') paintDiscovery(); }
}

async function probePeer(address: string): Promise<void> {
  if (!isTauri() || probeBusy) return;
  manualAddress = address.trim();
  const targetAddress = manualAddress;
  remoteFingerprint = ''; observedAddress = '';
  probeBusy = true; probeMessage = '正在探测…'; paintDiscovery();
  try {
    const value = await invoke<string>('v1_probe_peer', { address: targetAddress });
    if (!/^[0-9a-f]{64}$/.test(value)) throw new Error('invalid fingerprint');
    remoteFingerprint = value; observedAddress = targetAddress;
    probeMessage = value === fingerprint ? '该地址使用与本机相同的身份密钥' : '请双方通过可靠渠道核对完整指纹；此结果尚未建立信任';
  } catch (error) { probeMessage = displayError(error); }
  finally { probeBusy = false; if (page === 'discover') paintDiscovery(); }
}

app.addEventListener('input', (event) => {
  const target = event.target;
  if (target instanceof HTMLTextAreaElement && target.id === 'direct-input') { direct.draft(target.value); return; }
  if (target instanceof HTMLTextAreaElement && target.id === 'group-input') { groupChat.draft(target.value); return; }
  if (target instanceof HTMLTextAreaElement && target.id === 'self-input') { selfDraft = target.value; return; }
  if (!(target instanceof HTMLInputElement)) return;
  if (target.id === 'direct-address') { direct.addressDraft(target.value); return; }
  if (target.id === 'probe-port') probePort = target.value;
  if (target.id === 'scan-cidr') scanCidr = target.value;
  if (target.id === 'scan-block') scanBlacklistDraft = target.value;
  if (target.id === 'probe-address') { manualAddress = target.value; remoteFingerprint = ''; observedAddress = ''; paintDiscovery(); }
  if (target.id === 'group-search-query') { groupChat.searchDraft('query', target.value); return; }
  if (target.id === 'group-search-member') { groupChat.searchDraft('member', target.value); return; }
  if (target.id === 'group-search-category') { groupChat.searchDraft('category', target.value); return; }
  if (target.id === 'backup-password') { backupPassword = target.value; return; }
  if (target.id === 'contact-search') { contactQuery = target.value; void refreshContacts(); return; }
  if (target.id === 'global-search') { searchQuery = target.value; scheduleSearch(); return; }
});
app.addEventListener('keydown', (event) => {
  const target = event.target;
  if (!(target instanceof HTMLTextAreaElement) || !isSendKey(event)) return;
  if (target.id === 'direct-input') { event.preventDefault(); void direct.send(); return; }
  if (target.id === 'group-input') { event.preventDefault(); void groupChat.send(); return; }
  if (target.id === 'self-input') { event.preventDefault(); void sendSelf(); }
});
app.addEventListener('change', (event) => {
  const target = event.target;
  if (!(target instanceof HTMLSelectElement)) return;
  if (target.id === 'group-search-member') groupChat.searchDraft('member', target.value);
  if (target.id === 'group-search-category') groupChat.searchDraft('category', target.value);
});
app.addEventListener('click', (event) => {
  const target = event.target;
  if (!(target instanceof Element)) return;
  const action = target.closest<HTMLButtonElement>('button[data-action]');
  if (action?.dataset.action === 'open-group' && action.dataset.group) {
    const groupId = action.dataset.group, name = action.dataset.name || '群聊';
    direct.clear(); selfOpen = false; page = 'messages';
    void groupChat.open(groupId, name).catch(error => {
      const status = app.querySelector<HTMLElement>('#group-error'); if (status) status.textContent = displayError(error);
    }); return;
  }
  if (action?.dataset.action === 'open-direct' && action.dataset.peer) {
    void direct.open(action.dataset.peer, action.dataset.name || action.dataset.peer).catch(error => {
      const status = app.querySelector<HTMLElement>('#contacts-status'); if (status) status.textContent = displayError(error);
    }); return;
  }
  if (action?.dataset.action === 'hide-direct') { void direct.hide(); return; }
  if (action?.dataset.action === 'direct-send') { void direct.send(); return; }
  if (action?.dataset.action === 'direct-image') { void direct.pick('image'); return; }
  if (action?.dataset.action === 'direct-file') { void direct.pick('file'); return; }
  if (action?.dataset.action === 'direct-older') { void direct.load(true); return; }
  if (action?.dataset.action === 'direct-preview' && action.dataset.id) { direct.openPreviewById(action.dataset.id); return; }
  if (action?.dataset.action === 'direct-reveal' && action.dataset.id) { void direct.reveal(action.dataset.id); return; }
  if (action?.dataset.action === 'panel-reveal' && direct.panelId) { void direct.reveal(direct.panelId); return; }
  if (action?.dataset.action === 'panel-close') { direct.closePanel(); return; }
  if (action?.dataset.action === 'save-storage-quota') { void saveQuota(); return; }
  if (action?.dataset.action === 'backup-export') { void exportBackup(); return; }
  if (action?.dataset.action === 'backup-choose') { void chooseBackup(); return; }
  if (action?.dataset.action === 'backup-inspect') { void inspectBackup(); return; }
  if (action?.dataset.action === 'backup-restore-merge') { void restoreBackup(false); return; }
  if (action?.dataset.action === 'backup-restore-overwrite') { void restoreBackup(true); return; }
  if (action?.dataset.action === 'save-profile') { void saveProfile(); return; }
  if (action?.dataset.action === 'save-composer') { void saveComposer(); return; }
  if (action?.dataset.action === 'save-listen-port') { void saveListenPort(); return; }
  if (action?.dataset.action === 'scan-start') { void startScan(); return; }
  if (action?.dataset.action === 'save-scan-blacklist') { void saveScanBlacklist(); return; }
  if (action?.dataset.action === 'probe-scanned' && action.dataset.address) { void probePeer(action.dataset.address); return; }
  if (action?.dataset.action === 'toggle-emoji') {
    const panel = action.closest('.chat-compose')?.querySelector<HTMLElement>('.emoji-panel');
    if (panel) { const open = panel.hidden; if (open) void loadStickers(panel); panel.hidden = !open; }
    return;
  }
  if (action?.dataset.action === 'pick-emoji' && action.dataset.emoji) {
    const input = action.closest('.chat-compose')?.querySelector<HTMLTextAreaElement>('textarea');
    if (input) insertAtCursor(input, action.dataset.emoji);
    return;
  }
  if (action?.dataset.action === 'add-sticker') { void addSticker(); return; }
  if (action?.dataset.action === 'use-sticker' && action.dataset.name) { void sendSticker(action.dataset.name); return; }
  if (action?.dataset.action === 'remove-sticker' && action.dataset.name) { void removeSticker(action.dataset.name); return; }
  if (action?.dataset.action === 'toggle-sticker-manage') {
    stickerManage = !stickerManage;
    const panel = app.querySelector<HTMLElement>('.emoji-panel');
    if (panel) { panel.innerHTML = emojiPanelHtml(); void loadStickers(panel); }
    return;
  }
  if (action?.dataset.action === 'choose-avatar') { void chooseAvatar(); return; }
  if (action?.dataset.action === 'clear-avatar') { void clearAvatar(); return; }
  if (action?.dataset.action === 'save-contact-profile' && action.dataset.peer) { void saveContactProfile(action.dataset.peer); return; }
  if (action?.dataset.action === 'save-direct-address') { void direct.saveAddress(); return; }
  if (action?.dataset.action === 'group-send') { void groupChat.send(); return; }
  if (action?.dataset.action === 'group-image') { void groupChat.pick('image'); return; }
  if (action?.dataset.action === 'direct-screenshot' || action?.dataset.action === 'group-screenshot') { void takeScreenshot(); return; }
  if (action?.dataset.action === 'group-file') { void groupChat.pick('file'); return; }
  if (action?.dataset.action === 'group-reveal-file' && action.dataset.id) {
    void invoke('v1_reveal_group_attachment', { groupId: groupChat.groupId, messageId: action.dataset.id })
      .catch((error) => { groupChat.notify(displayError(error)); });
    return;
  }
  if (action?.dataset.action === 'save-experiment') { void saveExperiments(); return; }
  if (action?.dataset.action === 'group-older') { void groupChat.load(true); return; }
  if (action?.dataset.action === 'hide-group') { void groupChat.hide(); return; }
  if (action?.dataset.action === 'leave-group') { void groupChat.leave(); return; }
  if (action?.dataset.action === 'toggle-group-members') { groupChat.toggleMembers(); return; }
  if (action?.dataset.action === 'add-group-member') { void groupChat.addMember(); return; }
  if (action?.dataset.action === 'remove-group-member' && action.dataset.member) { void groupChat.removeMember(action.dataset.member); return; }
  if (action?.dataset.action === 'create-group') { void createGroup(); return; }
  if (action?.dataset.action === 'open-self') { void openSelf(); return; }
  if (action?.dataset.action === 'hide-self') { void hideSelf(); return; }
  if (action?.dataset.action === 'self-send') { void sendSelf(); return; }
  if (action?.dataset.action === 'self-older') { void loadSelfMessages(true); return; }
  if (action?.dataset.action === 'menu') { menuOpen = !menuOpen; render(); return; }
  if (action?.dataset.action === 'lock-now') { void lockNow(); return; }
  if (action?.dataset.action === 'unlock') { void unlockApp(); return; }
  if (action?.dataset.action === 'save-lock-password') { void saveLockPassword(); return; }
  if (action?.dataset.action === 'save-lock-idle') { void saveLockIdle(); return; }
  if (action?.dataset.action === 'export-text' && action.dataset.kind) { void exportChat(action.dataset.kind as 'self' | 'direct' | 'group'); return; }
  if (action?.dataset.action === 'open-hit' && action.dataset.kind && action.dataset.id && action.dataset.message) {
    void openSearchHit(action.dataset.kind, action.dataset.id, action.dataset.message, action.dataset.name || '');
    return;
  }
  if (action?.dataset.action === 'probe-start') { void setProbeService(true); return; }
  if (action?.dataset.action === 'probe-stop') { void setProbeService(false); return; }
  if (action?.dataset.action === 'probe-manual') { void probePeer(manualAddress); return; }
  if (action?.dataset.action === 'probe-candidate' && action.dataset.address) { void probePeer(action.dataset.address); return; }
  if (action?.dataset.action === 'select-outbound' && remoteFingerprint) { selectFingerprint(remoteFingerprint); return; }
  if (action?.dataset.action === 'select-inbound' && action.dataset.fingerprint) { selectFingerprint(action.dataset.fingerprint); return; }
  if (action?.dataset.action === 'confirm-contact') { void confirmContact(); return; }
  if (action?.dataset.action === 'toggle-group-search') { groupChat.toggleSearch(); return; }
  if (action?.dataset.action === 'group-search-close') { groupChat.closeSearch(); return; }
  if (action?.dataset.action === 'group-search-run') { void groupChat.runSearch(); return; }
  if (action?.dataset.action === 'group-reply' && action.dataset.id) { void groupChat.quote(action.dataset.id); return; }
  if (action?.dataset.action === 'group-cancel-reply') { groupChat.cancelQuote(); return; }
  if (action?.dataset.action === 'group-jump' && action.dataset.id) { void groupChat.jumpTo(action.dataset.id); return; }
  if (action?.dataset.action === 'group-mention' && action.dataset.member) { groupChat.selectMention(action.dataset.member); return; }
  if (action?.dataset.action === 'group-transfer-owner' && action.dataset.member) { void groupChat.transferOwner(action.dataset.member); return; }
  if (action?.dataset.action === 'group-dissolve') { void groupChat.dissolve(); return; }
  const settingsNav = target.closest<HTMLButtonElement>('button[data-settings-tab]');
  if (settingsNav?.dataset.settingsTab && SETTINGS_TABS.some((tab) => tab.id === settingsNav.dataset.settingsTab)) {
    settingsTab = settingsNav.dataset.settingsTab as SettingsTab;
    page = 'settings';
    menuOpen = false;
    render();
    void refreshStorage();
    return;
  }
  const nav = target.closest<HTMLButtonElement>('button[data-page]');
  if (nav?.dataset.page && nav.dataset.page in pages) {
    page = nav.dataset.page as Page;
    menuOpen = false;
    render();
    if (page === 'discover') { void refreshDiscovery(); void refreshProbeStatus(); }
    if (page === 'settings') void refreshStorage();
    if (page === 'contacts') void refreshContacts();
  }
});
render();
void initLock();
void refreshAvatar().then(() => { if (!lockState.locked) render(); });
void refreshComposer().then(() => { if (!lockState.locked) render(); });
void refreshNetwork().then(() => { if (!lockState.locked) render(); });
void refreshScanState().then(() => { if (!lockState.locked) render(); });
void refreshExperiments().then(() => { if (!lockState.locked) render(); });
for (const type of ['pointerdown', 'keydown', 'wheel', 'mousemove'] as const) {
  window.addEventListener(type, () => { lastActivity = Date.now(); }, { passive: true });
}
window.setInterval(() => { void checkIdleLock(); }, 15_000);
app.addEventListener('conversations-changed', () => { void refreshConversations(); });
window.setInterval(() => {
  void refreshDiscovery(); void refreshProbeStatus();
  if (page === 'messages') { void refreshConversations(); if (direct.peer) void direct.load(); if (groupChat.groupId) void groupChat.load(); }
}, 4000);
if (isTauri()) {
  // Dragging files onto the window provides real local paths, so no extra permission is needed.
  void getCurrentWebview().onDragDropEvent((event) => {
    if (event.payload.type !== 'drop') return;
    const paths = event.payload.paths.filter(path => path.length <= 4096);
    if (!paths.length) return;
    if (direct.peer) void direct.queuePaths(paths);
    else if (groupChat.groupId) void groupChat.queuePaths(paths);
  }).catch(() => undefined);
  invoke<string>('identity_fingerprint').then((value) => {
    fingerprint = /^[0-9a-f]{64}$/.test(value) ? value : '无法读取';
    if (page === 'settings') render(); else if (page === 'discover') paintDiscovery();
  }).catch(() => {
    fingerprint = '无法读取';
    if (page === 'settings') render(); else if (page === 'discover') paintDiscovery();
  });
}
