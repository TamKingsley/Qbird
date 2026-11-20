import './style.css';
import { invoke, isTauri } from '@tauri-apps/api/core';
import { open as openFileDialog, save as saveFileDialog } from '@tauri-apps/plugin-dialog';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import { listen } from '@tauri-apps/api/event';
import { version as appVersion } from '../package.json';
import { DirectChat } from './direct_chat';
import { GroupChat } from './group_chat';
import { composerTools, toolIcon } from './ui_icons';
import { showClosePrompt } from './close_prompt';
import { cropAvatarInteractively } from './avatar_crop';
import { workspace } from './workspace';
import { linkifiedText } from './linkify';

type Page = 'messages' | 'contacts' | 'discover' | 'settings' | 'about' | 'profile';
const root = document.querySelector<HTMLDivElement>('#app');
if (!root) throw new Error('Application root is missing');
const app = root;
let page: Page = 'messages';
let menuOpen = false;
let fingerprint = isTauri() ? '正在读取…' : '仅桌面应用可读取';
type Candidate = { service_name: string; addresses: string[]; nickname: string | null; username: string | null };
let candidates: Candidate[] = [];
type LanPerson = {
  service_name: string; address: string; fingerprint: string; qbird_id: string;
  nickname: string | null; username: string | null; known: boolean; verified: boolean;
};
let lanPeople: LanPerson[] = [];
let lanQuery = '';
let lanStatus = isTauri() ? '正在识别同网段用户…' : '仅桌面应用支持局域网发现';
let lanLoading = false;
let lanAddBusy = '';
let lanNotice = '';
let lanRefreshedAt = 0;
let discoveryStatus = isTauri() ? '正在查找…' : '仅桌面应用支持局域网发现';
let discoveryLoading = false;
type ProbeRequest = { fingerprint: string; remote_ip: string };
type ProbeStatus = { running: boolean; port: number | null; requests: ProbeRequest[] };
let probeStatus: ProbeStatus = { running: false, port: null, requests: [] };
let probePort = '42853';
type NetworkSettings = { default_port: number };
let networkSettings: NetworkSettings = { default_port: 42853 };
let networkNotice = '';
type CloseAction = 'ask' | 'tray' | 'quit';
const CLOSE_ACTION_LABEL: Record<CloseAction, string> = {
  ask: '每次关闭时询问',
  tray: '最小化到托盘',
  quit: '直接退出 Qbird',
};
let windowSettings: { close_action: CloseAction } = { close_action: 'ask' };
let closeActionNotice = '';
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
type Contact = { fingerprint: string; display_name: string; trust_state: string; username: string | null; remark: string | null; category: string | null };
let contactQuery = '';
let contactTab: 'direct' | 'group' = 'direct';
let contactsCache: Contact[] = [];
/// Best-effort presence keyed by fingerprint; refreshed in the background only.
let presence = new Map<string, boolean>();
let profileNotice = '';
/// Local nickname used for the placeholder initial when no avatar is set.
let myNickname = '';
let avatarUrl: string | null = null;
let avatarNotice = '';
let avatarBusy = false;
type ComposerSettings = { send_shortcut: string; rows: number };
type ExperimentSettings = { group_file_quote: boolean };
let experiments: ExperimentSettings = { group_file_quote: false };
let experimentNotice = '';
let composer: ComposerSettings = { send_shortcut: 'enter', rows: 3 };
let composerNotice = '';
type GroupSearchHit = { group_id: string; name: string; owner: string; member_count: number; reported_by: string; owner_known: boolean };
type GroupInvitation = { id: string; group_id: string; name: string; owner: string; owner_name: string | null; member_count: number; created_at: number };
let groupInvites: GroupInvitation[] = [];
let leftGroups: Group[] = [];
let invitesNotice = '';
let autoJoinGroups = false;
let autoAcceptContacts = true;
let contactSyncNotice = '';
type JoinRequest = { id: string; group_id: string; group_name: string; owner: string; applicant: string; applicant_name: string; created_at: number; status: string };
let groupSearchHits: GroupSearchHit[] = [];
let groupSearchStatus = '';
let groupSearchBusy = false;
let joinRequests: { incoming: JoinRequest[]; outgoing: JoinRequest[] } = { incoming: [], outgoing: [] };
let joinNotice = '';
type FileAcceptSettings = { mode: 'always' | 'remember' | 'auto'; remembered: string[] };
let fileAccept: FileAcceptSettings = { mode: 'always', remembered: [] };
type FileRecvSettings = { direct: string; group: string; direct_custom: boolean; group_custom: boolean };
let fileRecv: FileRecvSettings = { direct: '', group: '', direct_custom: false, group_custom: false };
const ACCEPT_MODE_LABEL: Record<FileAcceptSettings['mode'], string> = {
  always: '每次都询问（默认）',
  remember: '只询问未记住的来源',
  auto: '全部自动接收',
};
let fileAcceptNotice = '';
let team = { name: '', has_logo: false };
let teamLogoUrl: string | null = null;
let teamNotice = '';
let chatArchiveNotice = '';
let chatArchiveBusy = false;
let imageOpenMode: 'lightbox' | 'workspace' = 'lightbox';
let imageOpenNotice = '';
let fileOpenMode: 'workspace' | 'system' = 'workspace';
let fileOpenNotice = '';
let hover = { show_ip: true, show_mac: false };
let hoverNotice = '';
type PluginView = { id: string; name: string; version: string; api: number; enabled: boolean; extensions: string[] };
let plugins: PluginView[] = [];
let pluginNotice = '';
let pluginRemovePending = '';
const COMMON_EMOJIS = ['😀', '😁', '😂', '🤣', '😊', '😍', '😘', '😜', '🤔', '😅', '😭', '😡', '👍', '👎', '👏', '🙏', '💪', '🎉', '❤️', '💔', '🔥', '✨', '⭐', '🌹', '🍀', '☕', '🍰', '🐱', '🐶', '🌙', '☀️', '💡', '✅', '❌', '⚠️', '📌', '📎', '🚀', '🎯', '💬', '🙌', '🤝', '😴', '😎', '🥳', '🤩', '😢', '😤'];
let stickers: string[] = [];
let stickersLoaded = false;
const stickerUrls = new Map<string, string>();
let stickerManage = false;
let emojiNotice = '';
type Profile = { nickname: string; email: string; username: string };
type OnboardingState = { done: boolean; profile: Profile; qbird_id: string };
let onboardingLoaded = !isTauri();
let onboarding: OnboardingState | null = null;
let onboardingStep = 1;
let onboardingBusy = false;
let onboardingNotice = '';
let onboardingDraft = { nickname: '', username: '', email: '', recvDir: '' };
let qbirdId = '';
let contactNotice = '';
/// Browser-only visual check of the first-run flow (`?demo=1`); desktop builds ignore it.
const demoMode = !isTauri() && new URLSearchParams(location.search).get('demo') === '1';
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
type DirectoryUsage = { bytes: number; files: number; dirs: number; truncated: boolean };
type DiskSpace = { free_bytes: number | null; total_bytes: number | null; error: string | null };
type NetworkReport = {
  platform: string; hostname: string; local_ipv4: string[]; service_running: boolean; chat_enabled: boolean;
  service_port: number | null; default_port: number; default_port_available: boolean;
  discovery_started: boolean; lan_candidates: number; contacts: number; notes: string[];
};
type RecvDirReport = { kind: string; path: string; custom: boolean; exists: boolean; usage: DirectoryUsage; disk: DiskSpace };
type SpaceReport = {
  data_dir: string; data_dir_usage: DirectoryUsage; data_dir_disk: DiskSpace; database_bytes: number;
  attachments_bytes: number; attachments_quota: number; direct: RecvDirReport; group: RecvDirReport; notes: string[];
};
type RepairReport = { steps: { step: string; ok: boolean; detail: string }[] };
let networkReport: NetworkReport | null = null;
let spaceReport: SpaceReport | null = null;
let diagnosticsNotice = '';
let diagnosticsLoading = false;
let repairBusy = false;
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

// The workspace stores only serializable sources; chat modules resolve them back into
// preview content, so the same path works for live opens and restored tabs.
workspace.setResolver(async (source) => {
  if (source.kind === 'web') {
    return { title: source.url, loaded: { kind: 'error', message: '网页由 iframe 直接加载' } };
  }
  if (source.kind === 'group-file') {
    return {
      title: source.name,
      loaded: await groupChat.loadWorkspaceFile(source),
      actions: {
        systemOpen: () => { void groupChat.openSystemById(source.messageId); },
        reveal: () => { void groupChat.reveal(source.messageId); },
      },
    };
  }
  return {
    title: source.name,
    loaded: await direct.loadWorkspaceFile(source),
    actions: {
      systemOpen: () => { void direct.openSystemById(source.messageId); },
      reveal: () => { void direct.reveal(source.messageId); },
    },
  };
});

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
    const body = document.createElement('div'); body.className = 'message-bubble'; body.append(linkifiedText(message.body));
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
    <div class="chat-compose"><label for="self-input">消息</label><textarea id="self-input" rows="3" maxlength="32768" placeholder="写点什么…"></textarea>${composerTools('self', 'Enter 发送 · Shift+Enter 换行', false)}<p id="self-error" role="status"></p></div>
  </section>`;
}

type PlaceholderPage = 'messages' | 'contacts' | 'discover' | 'settings' | 'about' | 'profile';
const pages: Record<PlaceholderPage, { title: string; eyebrow: string; heading: string; copy: string }> = {
  messages: { title: '消息', eyebrow: 'MESSAGES', heading: '还没有会话', copy: '从联系人打开自聊或已确认的单聊。' },
  contacts: { title: '联系人', eyebrow: 'CONTACTS', heading: '联系人', copy: '可搜索联系人、编辑本机备注与分组。' },
  discover: { title: '局域网用户', eyebrow: 'DISCOVERY', heading: '局域网用户', copy: '同一网段的 Qbird 用户会自动出现，点「添加」即可加入联系人。' },
  settings: { title: '设置', eyebrow: 'PREFERENCES', heading: '设置', copy: '存储配额、备份恢复与应用锁已开放；插件与更多设置将在后续阶段提供。' },
  about: { title: '关于', eyebrow: 'ABOUT', heading: 'Qbird 青鸟', copy: '本地优先的局域网聊天工具。' },
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
    <p>昵称、用户名和邮箱只保存在本机，不改变身份。用户名用于局域网搜索，昵称可以随时修改。</p>
    <p class="hint">Qbird ID <code>${shortId(fingerprint)}</code> · 由身份密钥生成，安装后不变</p>
    <div class="avatar-row">
      <div class="avatar-preview">${avatarMarkup()}</div>
      <div class="avatar-actions">
        <button class="outline" data-action="choose-avatar" ${avatarBusy ? 'disabled' : ''}>${avatarUrl ? '更换头像…' : '选择头像…'}</button>
        <button class="outline" data-action="clear-avatar" ${avatarUrl && !avatarBusy ? '' : 'disabled'}>移除头像</button>
        <small>选择图片后可预览、拖动与缩放，确认后保存为正方形（最大 512 像素）；头像只保存在本机，尚未随消息发送。</small>
      </div>
    </div>
    <label for="profile-nickname">昵称</label>
    <input id="profile-nickname" type="text" maxlength="64" autocomplete="off">
    <label for="profile-username">用户名</label>
    <input id="profile-username" type="text" maxlength="20" autocomplete="off" spellcheck="false" placeholder="2–20 位英文字母、数字或下划线">
    <p class="hint">只能使用${USERNAME_HINT}；用户名用于局域网搜索，不能以数字开头。</p>
    <label for="profile-email">邮箱（可选）</label>
    <input id="profile-email" type="text" maxlength="128" autocomplete="off">
    <div class="probe-row"><button class="outline" data-action="save-profile">保存资料</button></div>
    <p id="profile-notice" role="status">${profileNotice}</p>
    <div class="probe-box"><h3>联系人添加</h3>
      <p>同一局域网的同事添加你时，本机默认也把对方加入联系人，双方一次添加即可互相聊天。</p>
      <p><label><input id="auto-accept-contacts" type="checkbox" ${autoAcceptContacts ? 'checked' : ''}> 自动接受对方添加（关闭后只在本机提示，不自动加入）</label></p>
      <p id="contact-sync-notice" class="hint" role="status">${contactSyncNotice}</p>
    </div>
    <div class="probe-box"><h3>联系人悬浮信息</h3>
      <p>悬停联系人时显示：${hover.show_ip ? 'IP 地址' : '不显示 IP'}${hover.show_mac ? '、MAC 地址' : '、不显示 MAC'}。</p>
      <p><label><input id="hover-show-ip" type="checkbox" ${hover.show_ip ? 'checked' : ''}> 显示 IP 地址（来自本机保存的联系人地址）</label></p>
      <p><label><input id="hover-show-mac" type="checkbox" ${hover.show_mac ? 'checked' : ''}> 显示 MAC 地址（读取本机 ARP 表，取不到时显示“未获取”）</label></p>
      <div class="probe-row"><button class="outline" data-action="save-hover-settings">保存悬浮设置</button></div>
      <p id="hover-notice" role="status">${hoverNotice}</p>
      <small>IP 与 MAC 只作参考，不构成身份凭证；跨网段或系统未缓存时 MAC 无法获取，界面不会虚构。</small>
    </div>
    <p>本机身份指纹（SHA-256）：<br><code class="fingerprint">${fingerprint}</code></p>
    <p id="avatar-notice" role="status">${avatarNotice}</p>
  </section>`;
}

async function refreshProfile(): Promise<void> {
  if (!isTauri()) return;
  try {
    const profile = await invoke<Profile>('v1_profile');
    if (profile.nickname !== myNickname) {
      myNickname = profile.nickname;
      paintOwnAvatar();
    }
    try {
      const sync = await invoke<{ auto_accept: boolean }>('v1_contact_sync_settings');
      autoAcceptContacts = sync.auto_accept;
      const toggle = app.querySelector<HTMLInputElement>('#auto-accept-contacts');
      if (toggle) toggle.checked = autoAcceptContacts;
    } catch { /* keep the default */ }
    const nickname = app.querySelector<HTMLInputElement>('#profile-nickname');
    const username = app.querySelector<HTMLInputElement>('#profile-username');
    const email = app.querySelector<HTMLInputElement>('#profile-email');
    if (nickname) nickname.value = profile.nickname;
    if (username) username.value = profile.username;
    if (email) email.value = profile.email;
  } catch { profileNotice = '无法读取个人资料'; }
}

async function saveProfile(): Promise<void> {
  const nickname = app.querySelector<HTMLInputElement>('#profile-nickname')?.value ?? '';
  const username = app.querySelector<HTMLInputElement>('#profile-username')?.value ?? '';
  const email = app.querySelector<HTMLInputElement>('#profile-email')?.value ?? '';
  const notice = app.querySelector<HTMLElement>('#profile-notice');
  if (!isTauri()) return;
  if (!USERNAME_PATTERN.test(username.trim())) {
    profileNotice = `用户名须为${USERNAME_HINT}`;
    if (notice) notice.textContent = profileNotice;
    return;
  }
  try {
    await invoke<Profile>('v1_set_profile', { nickname, email });
    await invoke<Profile>('v1_set_username', { username });
    if (nickname.trim() !== myNickname) {
      myNickname = nickname.trim();
      const preview = app.querySelector<HTMLElement>('.avatar-preview');
      if (preview) preview.innerHTML = avatarMarkup();
      paintOwnAvatar();
    }
    // Rebuild the mDNS announcement so peers see the new nickname right away.
    const broadcast = await invoke('v1_refresh_listener').then(() => true).catch(() => false);
    profileNotice = broadcast
      ? '资料已保存，并已重新广播给同一网段的同事。'
      : '资料已保存在本机；当前未监听，下次启动应用时会使用新资料广播。';
  } catch (error) { profileNotice = typeof error === 'string' ? error : '无法保存资料'; }
  if (notice) notice.textContent = profileNotice;
}

/// Placeholder shown before any avatar is uploaded: the first character of the
/// nickname (Chinese character or Latin initial), never a fixed brand glyph.
function avatarInitial(name: string): string {
  const first = Array.from(name.trim())[0] ?? '';
  if (!first) return '青';
  return /^[a-z]$/i.test(first) ? first.toUpperCase() : first;
}

function avatarMarkup(nickname: string = myNickname): string {
  return avatarUrl ? `<img src="${avatarUrl}" alt="我的头像">` : escapeText(avatarInitial(nickname));
}

/// The rail avatar lives outside the profile card, so update it in place
/// instead of re-rendering the whole shell while an input has focus.
function paintOwnAvatar(): void {
  const button = app.querySelector<HTMLElement>('button.profile[data-page="profile"]');
  if (button) button.innerHTML = avatarMarkup();
}

async function refreshOwnNickname(): Promise<void> {
  if (!isTauri()) return;
  try {
    const profile = await invoke<Profile>('v1_profile');
    if (profile.nickname === myNickname) return;
    myNickname = profile.nickname;
    paintOwnAvatar();
  } catch { /* keep the neutral placeholder */ }
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
    avatarBusy = true; avatarNotice = '正在读取图片…'; if (page === 'profile') render();
    const source = new Uint8Array(await invoke<number[]>('v1_read_image_file', { path: picked }));
    // Nothing is written until the user confirms the crop.
    const cropped = await cropAvatarInteractively(source, { title: '调整头像' });
    if (!cropped) { avatarNotice = '已取消，头像未修改。'; return; }
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

/// The toolbar button is part of each template; this only mounts the shared panel.
function applyEmojiUI(): void {
  for (const compose of app.querySelectorAll<HTMLElement>('.chat-compose')) {
    if (compose.querySelector('.emoji-panel')) continue;
    const panel = document.createElement('div');
    panel.className = 'emoji-panel';
    panel.hidden = true;
    panel.innerHTML = emojiPanelHtml();
    compose.append(panel);
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

async function refreshOnboarding(): Promise<void> {
  if (!isTauri()) {
    onboardingLoaded = true;
    if (demoMode) {
      qbirdId = 'QB-2F91-8C3D';
      onboarding = { done: false, profile: { nickname: 'Tam', email: '', username: 'tam' }, qbird_id: qbirdId };
      onboardingDraft = { nickname: 'Tam', username: 'tam', email: '', recvDir: '' };
      lanPeople = [
        { service_name: 'a', address: '192.168.1.23:42853', fingerprint: 'a'.repeat(64), qbird_id: 'QB-AAAA-1111', nickname: '林小满', username: 'xiaoman', known: true, verified: true },
        { service_name: 'b', address: '192.168.1.41:42853', fingerprint: 'b'.repeat(64), qbird_id: 'QB-BBBB-2222', nickname: '周隐', username: 'zhouyin', known: false, verified: false },
        { service_name: 'c', address: '192.168.1.77:42853', fingerprint: 'c'.repeat(64), qbird_id: 'QB-CCCC-3333', nickname: '陈默', username: 'chenmo', known: false, verified: false },
      ];
      lanStatus = '发现 3 位用户';
      lanRefreshedAt = Date.now();
      page = 'discover';
    }
    return;
  }
  try {
    onboarding = await invoke<OnboardingState>('v1_onboarding_state');
    qbirdId = onboarding.qbird_id || shortId(fingerprint);
    if (!onboarding.done) {
      onboardingDraft.nickname = onboarding.profile.nickname || '';
      onboardingDraft.username = onboarding.profile.username || suggestUsername();
      onboardingDraft.email = onboarding.profile.email || '';
      page = 'discover';
    }
  } catch {
    // A broken profile must not trap the user in the setup screen.
    onboarding = { done: true, profile: { nickname: '', email: '', username: '' }, qbird_id: shortId(fingerprint) };
  }
  onboardingLoaded = true;
  render();
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

async function refreshWindowSettings(): Promise<void> {
  if (!isTauri()) return;
  try { windowSettings = await invoke<{ close_action: CloseAction }>('v1_window_settings'); }
  catch { closeActionNotice = '无法读取关闭行为'; }
}

async function saveCloseAction(): Promise<void> {
  if (!isTauri()) return;
  const value = app.querySelector<HTMLSelectElement>('#close-action')?.value;
  if (value !== 'ask' && value !== 'tray' && value !== 'quit') return;
  try {
    windowSettings = await invoke<{ close_action: CloseAction }>('v1_set_window_settings', { closeAction: value });
    closeActionNotice = '关闭行为已保存。';
  } catch (error) { closeActionNotice = displayError(error); }
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

type SettingsTab = 'general' | 'privacy' | 'workspace' | 'experimental' | 'debug';
let settingsTab: SettingsTab = 'general';
const SETTINGS_TABS: { id: SettingsTab; label: string }[] = [
  { id: 'general', label: '常规与网络' },
  { id: 'privacy', label: '隐私与安全' },
  { id: 'workspace', label: '工作区' },
  { id: 'experimental', label: '实验功能' },
  { id: 'debug', label: '调试模式' },
];

function settingsView(): string {
  const tab = SETTINGS_TABS.find((item) => item.id === settingsTab) ?? SETTINGS_TABS[0];
  return `<section class="card settings-card">
    <span class="eyebrow">SETTINGS / 设置</span><h2>${tab.label}</h2>
    ${settingsTab === 'privacy' ? privacySettingsHtml() : settingsTab === 'workspace' ? workspaceSettingsHtml() : settingsTab === 'experimental' ? experimentalSettingsHtml() : settingsTab === 'debug' ? debugSettingsHtml() : generalSettingsHtml()}
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
      <p>默认端口 <strong>${networkSettings.default_port}</strong>，应用启动时自动用它开始监听；正在运行的服务不受保存影响。</p>
      <label for="listen-port">默认端口（1024–65535）</label>
      <div class="probe-row"><input id="listen-port" type="number" min="1024" max="65535" value="${networkSettings.default_port}"><button class="outline" data-action="save-listen-port">保存端口</button></div>
      <p id="network-notice" role="status">${networkNotice}</p>
      <small>应用每次启动都会自动监听：端口被占用时自动重试，仍失败则改用空闲端口（mDNS 会广播实际端口）；监听意外停止会在 15 秒内自动恢复。手动停止后本次运行不再自动重启，修改端口在下次启动应用时生效。</small>
    </div>
    <p>本机身份指纹（SHA-256）：<br><code class="fingerprint">${fingerprint}</code></p>
    <div class="probe-box"><h3>关闭窗口时</h3>
      <p>当前行为：<strong>${CLOSE_ACTION_LABEL[windowSettings.close_action]}</strong></p>
      <label for="close-action">点击窗口右上角关闭按钮时</label>
      <select id="close-action">
        <option value="ask" ${windowSettings.close_action === 'ask' ? 'selected' : ''}>每次询问（最小化到托盘 / 退出）</option>
        <option value="tray" ${windowSettings.close_action === 'tray' ? 'selected' : ''}>最小化到托盘</option>
        <option value="quit" ${windowSettings.close_action === 'quit' ? 'selected' : ''}>直接退出 Qbird</option>
      </select>
      <div class="probe-row"><button class="outline" data-action="save-close-action">保存关闭行为</button></div>
      <p id="close-action-notice" role="status">${closeActionNotice}</p>
      <small>最小化到托盘时 Qbird 继续在后台运行并接收消息，可从系统托盘图标或托盘菜单重新打开；托盘菜单也提供“退出 Qbird”。</small>
    </div>
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
    <div class="probe-box"><h3>文件接收</h3>
      <p>当前策略：<strong>${ACCEPT_MODE_LABEL[fileAccept.mode]}</strong>；${fileAccept.remembered.length ? `已记住 ${fileAccept.remembered.length} 个来源` : '尚未记住任何来源'}。</p>
      <label for="file-accept-mode">接收文件时的处理方式</label>
      <select id="file-accept-mode">
        <option value="always" ${fileAccept.mode === 'always' ? 'selected' : ''}>每次都询问（默认）</option>
        <option value="remember" ${fileAccept.mode === 'remember' ? 'selected' : ''}>只询问未记住的来源</option>
        <option value="auto" ${fileAccept.mode === 'auto' ? 'selected' : ''}>全部自动接收</option>
      </select>
      <div class="probe-row"><button class="outline" data-action="save-file-accept">保存接收策略</button><button class="outline" data-action="forget-file-sources" ${fileAccept.remembered.length ? '' : 'disabled'}>清除已记住的来源</button></div>
      <label>单聊接收目录</label>
      <p><code class="recv-dir">${escapeText(fileRecv.direct || '读取中…')}</code>${fileRecv.direct_custom ? '（自定义）' : '（默认）'}</p>
      <div class="probe-row"><button class="outline" data-action="choose-recv-direct">更改单聊目录…</button><button class="outline" data-action="reset-recv-direct" ${fileRecv.direct_custom ? '' : 'disabled'}>恢复默认</button></div>
      <label>群聊接收目录</label>
      <p><code class="recv-dir">${escapeText(fileRecv.group || '读取中…')}</code>${fileRecv.group_custom ? '（自定义）' : '（默认）'}</p>
      <div class="probe-row"><button class="outline" data-action="choose-recv-group">更改群聊目录…</button><button class="outline" data-action="reset-recv-group" ${fileRecv.group_custom ? '' : 'disabled'}>恢复默认</button></div>
      <p id="file-accept-notice" role="status">${fileAcceptNotice}</p>
      <small>路径变更只影响之后接收的文件，进行中的传输和已有附件保持原路径。自定义到应用数据目录之外的文件夹不占用应用存储配额，请自行管理磁盘空间；网络盘或移动盘断线会导致传输失败，恢复后继续续传。</small>
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
    </div>
    <div class="probe-box"><h3>聊天记录导入导出</h3>
      <p>加密导出当前打开会话（单聊、群聊或自聊）的纯文字记录；附件文件不包含在内。</p>
      <label for="chat-archive-password">聊天记录密码（8–256 字符）</label>
      <input id="chat-archive-password" type="password" maxlength="256" autocomplete="new-password">
      <div class="probe-row"><button class="outline" data-action="export-chat-archive" ${chatArchiveBusy ? 'disabled' : ''}>导出当前会话…</button><button class="outline" data-action="import-chat-archive" ${chatArchiveBusy ? 'disabled' : ''}>导入聊天记录…</button></div>
      <p id="chat-archive-notice" role="status">${chatArchiveNotice}</p>
      <small>导入只合并文字消息：相同 ID 且内容一致会跳过，内容不同的记录不会被覆盖；单聊需对方已确认，群聊需你仍是成员。密码丢失后文件无法解密。</small>
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
    <div class="probe-box"><h3>团队与品牌</h3>
      <p>团队名称 ${team.name ? `<strong>${escapeText(team.name)}</strong>` : '未设置（显示 Qbird 青鸟）'}；团队 Logo：${team.has_logo ? '已设置' : '未设置'}。</p>
      <label for="team-name">团队名称（1–64 字符，留空恢复产品名）</label>
      <div class="probe-row"><input id="team-name" type="text" maxlength="64" value="${escapeText(team.name)}"><button class="outline" data-action="save-team-name">保存名称</button></div>
      <div class="probe-row"><button class="outline" data-action="choose-team-logo">选择 Logo…</button><button class="outline" data-action="clear-team-logo" ${team.has_logo ? '' : 'disabled'}>移除 Logo</button></div>
      <p id="team-notice" role="status">${teamNotice}</p>
      <small>团队名称与 Logo 只保存在本机侧栏与设置页，不随消息或群组发送。</small>
    </div>
    <div class="probe-box"><h3>右侧工作区</h3>
      <p>图片打开方式：${imageOpenMode === 'workspace' ? '在右侧工作区预览' : '在灯箱中放大'}；附件打开方式：${fileOpenMode === 'system' ? '用默认程序打开' : '在右侧工作区预览'}。</p>
      <label for="image-open-mode">点击聊天图片时</label>
      <select id="image-open-mode">
        <option value="lightbox" ${imageOpenMode === 'lightbox' ? 'selected' : ''}>灯箱查看（可缩放、旋转、拖拽）</option>
        <option value="workspace" ${imageOpenMode === 'workspace' ? 'selected' : ''}>右侧工作区打开</option>
      </select>
      <div class="probe-row"><button class="outline" data-action="save-image-open-mode">保存图片打开方式</button></div>
      <p id="image-open-notice" role="status">${imageOpenNotice}</p>
      <label for="file-open-mode">点击聊天文件时</label>
      <select id="file-open-mode">
        <option value="workspace" ${fileOpenMode === 'workspace' ? 'selected' : ''}>在右侧工作区预览</option>
        <option value="system" ${fileOpenMode === 'system' ? 'selected' : ''}>用默认程序打开</option>
      </select>
      <div class="probe-row"><button class="outline" data-action="save-file-open-mode">保存附件打开方式</button></div>
      <p id="file-open-notice" role="status">${fileOpenNotice}</p>
      <p>右侧工作区为全局标签页，切换会话不关闭；支持图片、PDF、TXT/日志/CSV/JSON、Markdown、禁用脚本的 HTML 与网页链接。</p>
      <small>预览只读取已完成的本地附件，不会上传内容；PDF 依赖 Windows WebView2 内置阅读器，docx/xlsx/pptx 暂无内置预览，可用默认程序打开或安装预览插件。</small>
    </div>
    <div class="probe-box"><h3>表情</h3>
      <p>常用表情和本地表情收藏在聊天输入区的“表情”按钮中管理；收藏的表情按图片文件发送给已确认联系人。</p>
      <small>本地收藏最多 64 个，单个不超过 512 KiB，支持 PNG、JPEG、GIF 与 WebP。</small>
    </div>
`;
}

function experimentalSettingsHtml(): string {
  return `<div class="probe-box"><h3>实验功能</h3>
      <p><label><input id="experiment-group-file-quote" type="checkbox" ${experiments.group_file_quote ? 'checked' : ''}> 允许引用群文件消息（实验）</label></p>
      <p>开启后，群聊里可以引用文件消息并在气泡中显示文件名；关闭时文件消息不能引用。默认关闭，不影响普通成员 @。</p>
      <div class="probe-row"><button class="outline" data-action="save-experiment">保存实验设置</button></div>
      <p id="experiment-notice" role="status">${experimentNotice}</p>
      <small>Agent 联系人等能力仍未实现，不会显示为可操作开关。</small>
    </div>
    <div class="probe-box"><h3>预览插件（只读）</h3>
      <p>插件以本地文件夹安装，页面在沙箱 iframe 中运行：没有同源权限和网络访问，只能拿到被预览文件的字节，看不到聊天数据、身份或本机路径。</p>
      <ul id="plugin-list" class="discovery-results">${plugins.map(plugin => `<li><span class="contact-title">${escapeText(plugin.name)} ${escapeText(plugin.version)} · ${escapeText(plugin.extensions.join(' / ')) || '无扩展名'}</span><div class="contact-actions"><label><input type="checkbox" data-plugin-toggle="${escapeText(plugin.id)}" ${plugin.enabled ? 'checked' : ''}> 启用</label><button class="linkish" data-action="remove-plugin" data-id="${escapeText(plugin.id)}">${pluginRemovePending === plugin.id ? '确认移除' : '移除'}</button></div></li>`).join('') || '<li>尚未安装插件</li>'}</ul>
      <div class="probe-row"><button class="outline" data-action="install-plugin">从文件夹安装…</button></div>
      <p id="plugin-notice" role="status">${pluginNotice}</p>
      <small>第一版只读：docx、pptx、pdf、xlsx 等由插件自带查看器渲染；编辑写回不在本版。插件目录位于本机应用数据目录的 plugins/ 下。</small>
    </div>`;
}

function diagLine(label: string, value: string, tone: 'ok' | 'warn' | 'muted' = 'muted'): string {
  return `<div class="diag-row"><span class="diag-label">${escapeText(label)}</span><span class="diag-value ${tone}">${escapeText(value)}</span></div>`;
}

function diskText(disk: DiskSpace): string {
  if (disk.free_bytes === null || disk.total_bytes === null) return `未获取${disk.error ? `：${disk.error}` : ''}`;
  return `可用 ${formatBytes(disk.free_bytes)} / 共 ${formatBytes(disk.total_bytes)}`;
}

function networkReportHtml(): string {
  if (!networkReport) return '<p class="hint">尚未检测。点击“重新检测”读取本机网络状态。</p>';
  const report = networkReport;
  const service = report.service_running ? `运行中 · 端口 ${report.service_port ?? '未知'}` : '未启动';
  const chat = report.service_running
    ? (report.chat_enabled ? '已启用（可接收消息和文件）' : '未启用（仅身份探测）')
    : '不可用（服务未启动）';
  const portOk = report.default_port_available || (report.service_running && report.service_port === report.default_port);
  const port = `${report.default_port} · ${report.default_port_available ? '空闲' : (portOk ? '由本机监听占用' : '被其他程序占用')}`;
  const discovery = report.discovery_started ? `已启动 · 候选 ${report.lan_candidates} 个` : '未启动';
  return `<div class="diag-grid">
      ${diagLine('平台 / 主机', `${report.platform} · ${report.hostname}`)}
      ${diagLine('本机 IPv4', report.local_ipv4.length ? report.local_ipv4.join('、') : '未获取', report.local_ipv4.length ? 'ok' : 'warn')}
      ${diagLine('监听服务', service, report.service_running ? 'ok' : 'warn')}
      ${diagLine('聊天接收', chat, report.chat_enabled ? 'ok' : 'warn')}
      ${diagLine('默认端口', port, portOk ? 'ok' : 'warn')}
      ${diagLine('局域网发现', discovery, report.lan_candidates > 0 ? 'ok' : 'muted')}
      ${diagLine('已确认联系人', String(report.contacts))}
    </div>
    ${report.notes.length ? `<ul class="diag-notes">${report.notes.map((note) => `<li>${escapeText(note)}</li>`).join('')}</ul>` : '<p class="diag-ok">未发现明显网络异常。</p>'}`;
}

function recvDirLine(label: string, dir: RecvDirReport): string {
  const suffix = dir.custom ? '（自定义）' : '（默认）';
  const usage = dir.exists ? formatBytes(dir.usage.bytes) : '目录不存在';
  return diagLine(label, `${dir.path}${suffix} · ${usage} · ${diskText(dir.disk)}`, dir.exists ? 'muted' : 'warn');
}

function spaceReportHtml(): string {
  if (!spaceReport) return '<p class="hint">尚未检测。点击“重新检测空间”读取本地占用与磁盘余量。</p>';
  const report = spaceReport;
  const percent = report.attachments_quota > 0
    ? Math.min(100, Math.round((report.attachments_bytes / report.attachments_quota) * 100))
    : 0;
  return `<div class="diag-grid">
      ${diagLine('应用数据目录', `${report.data_dir}（${formatBytes(report.data_dir_usage.bytes)}）`)}
      ${diagLine('数据目录磁盘', diskText(report.data_dir_disk))}
      ${diagLine('数据库文件', formatBytes(report.database_bytes))}
      ${diagLine('附件占用', `${formatBytes(report.attachments_bytes)} / ${formatBytes(report.attachments_quota)}（${percent}%）`)}
      <progress max="100" value="${percent}"></progress>
      ${recvDirLine('单聊接收目录', report.direct)}
      ${recvDirLine('群聊接收目录', report.group)}
    </div>
    ${report.notes.length ? `<ul class="diag-notes">${report.notes.map((note) => `<li>${escapeText(note)}</li>`).join('')}</ul>` : '<p class="diag-ok">空间状态正常。</p>'}`;
}

function debugSettingsHtml(): string {
  return `<p>用于排查“看不到同事、收不到消息或文件”的本机自检。检测只读取本机状态，不上传任何内容。</p>
    <div class="probe-box"><h3>网络检测与修复</h3>
      <div class="probe-row">
        <button class="outline" data-action="debug-scan" ${diagnosticsLoading ? 'disabled' : ''}>重新检测</button>
        <button class="outline" data-action="debug-repair" data-mode="restart" ${repairBusy ? 'disabled' : ''}>重启聊天服务</button>
        <button class="outline" data-action="debug-repair" data-mode="rediscover" ${repairBusy ? 'disabled' : ''}>重建局域网发现</button>
        <button class="outline" data-action="debug-repair" data-mode="all" ${repairBusy ? 'disabled' : ''}>一键修复</button>
      </div>
      <p id="debug-network-notice" role="status">${escapeText(diagnosticsNotice)}</p>
      <div id="debug-network-report">${networkReportHtml()}</div>
      <small>“重启聊天服务”会短暂断开监听后按原端口恢复，原端口不可用时改用默认端口；“重建局域网发现”只刷新广播浏览。两者都不会改动身份、联系人和聊天记录。</small>
    </div>
    <div class="probe-box"><h3>空间检测</h3>
      <div class="probe-row"><button class="outline" data-action="debug-space" ${diagnosticsLoading ? 'disabled' : ''}>重新检测空间</button></div>
      <div id="debug-space-report">${spaceReportHtml()}</div>
      <small>统计为本地文件实际占用。自定义接收目录位于应用数据目录之外时不受存储配额限制，但同样受磁盘容量限制；空间不足通常是“对方发送失败”或“接收失败”的原因之一。</small>
    </div>`;
}

async function loadDebugData(): Promise<void> {
  const [network, space] = await Promise.all([
    invoke<NetworkReport>('v1_network_diagnostics'),
    invoke<SpaceReport>('v1_space_diagnostics'),
  ]);
  networkReport = network;
  spaceReport = space;
}

async function refreshDebugReports(notice?: string): Promise<void> {
  if (!isTauri() || diagnosticsLoading) return;
  diagnosticsLoading = true;
  paintDebugReports();
  try {
    await loadDebugData();
    diagnosticsNotice = notice ?? '';
  } catch (error) {
    diagnosticsNotice = displayError(error);
  } finally {
    diagnosticsLoading = false;
    paintDebugReports();
  }
}

function paintDebugReports(): void {
  if (page !== 'settings' || settingsTab !== 'debug') return;
  const network = app.querySelector<HTMLElement>('#debug-network-report');
  if (network) network.innerHTML = networkReportHtml();
  const space = app.querySelector<HTMLElement>('#debug-space-report');
  if (space) space.innerHTML = spaceReportHtml();
  const notice = app.querySelector<HTMLElement>('#debug-network-notice');
  if (notice) notice.textContent = diagnosticsNotice;
}

async function runNetworkRepair(mode: 'restart' | 'rediscover' | 'all'): Promise<void> {
  if (!isTauri() || repairBusy) return;
  repairBusy = true;
  diagnosticsNotice = '正在执行修复…';
  paintDebugReports();
  try {
    const report = await invoke<RepairReport>('v1_network_repair', { action: mode });
    const summary = report.steps.map((step) => `${step.ok ? '✓' : '✗'} ${step.step}：${step.detail}`).join('；');
    diagnosticsNotice = `修复完成：${summary}`;
    try { await loadDebugData(); }
    catch { diagnosticsNotice += '；修复后重新检测失败，请手动重新检测。'; }
  } catch (error) {
    diagnosticsNotice = `修复失败：${displayError(error)}`;
  } finally {
    repairBusy = false;
    paintDebugReports();
  }
}

async function takeScreenshot(): Promise<void> {
  if (!isTauri()) return;
  if (!direct.peer && !groupChat.groupId) { groupChat.notify('请先打开一个单聊或群聊再截图。'); return; }
  try { await invoke('v1_begin_capture'); }
  catch (error) { groupChat.notify(displayError(error)); }
}

/// Images pasted from the clipboard (for example a third-party screenshot tool)
/// are saved as a temp file and staged, exactly like a dropped file.
async function handleImagePaste(event: ClipboardEvent): Promise<void> {
  if (!isTauri()) return;
  const target = event.target as HTMLElement | null;
  if (!target?.closest('.chat-compose')) return;
  const items = Array.from(event.clipboardData?.items ?? []);
  const item = items.find((entry) => entry.kind === 'file' && entry.type.startsWith('image/'));
  if (!item) return;
  const file = item.getAsFile();
  if (!file) return;
  event.preventDefault();
  const extension = file.type === 'image/bmp' ? 'bmp' : file.type === 'image/png' ? 'png' : '';
  if (!extension) { groupChat.notify('仅支持粘贴 PNG/BMP 图片；JPG 请先另存为文件再拖入。'); return; }
  try {
    const bytes = Array.from(new Uint8Array(await file.arrayBuffer()));
    if (!bytes.length || bytes.length > 16 * 1024 * 1024) { groupChat.notify('粘贴的图片过大（上限 16 MiB）'); return; }
    const path = await invoke<string>('v1_save_temp_image', { bytes, extension });
    if (direct.peer) await direct.stagePaths([path]);
    else if (groupChat.groupId) await groupChat.stagePaths([path]);
  } catch (error) { groupChat.notify(displayError(error)); }
}

async function refreshFileAccept(): Promise<void> {
  if (!isTauri()) return;
  try { fileAccept = await invoke<FileAcceptSettings>('v1_file_accept_settings'); }
  catch { fileAcceptNotice = '无法读取接收设置'; }
  try { fileRecv = await invoke<FileRecvSettings>('v1_file_recv_settings'); }
  catch { fileAcceptNotice = '无法读取接收目录'; }
}

async function saveFileAccept(): Promise<void> {
  if (!isTauri()) return;
  const mode = app.querySelector<HTMLSelectElement>('#file-accept-mode')?.value ?? 'always';
  try {
    fileAccept = await invoke<FileAcceptSettings>('v1_set_file_accept_policy', { mode });
    fileAcceptNotice = `已保存：${ACCEPT_MODE_LABEL[fileAccept.mode]}。`;
  } catch (error) { fileAcceptNotice = displayError(error); }
  if (page === 'settings') render();
}

async function forgetFileSources(): Promise<void> {
  if (!isTauri()) return;
  try {
    fileAccept = await invoke<FileAcceptSettings>('v1_forget_file_sources');
    fileAcceptNotice = '已清除记住的来源，之后会重新询问。';
  } catch (error) { fileAcceptNotice = displayError(error); }
  if (page === 'settings') render();
}

async function chooseRecvDir(kind: 'direct' | 'group'): Promise<void> {
  if (!isTauri()) return;
  try {
    const picked = await openFileDialog({ directory: true, multiple: false, title: kind === 'direct' ? '选择单聊文件接收目录' : '选择群聊文件接收目录' });
    if (typeof picked !== 'string') return;
    fileRecv = await invoke<FileRecvSettings>('v1_set_file_recv_dir', { kind, path: picked });
    fileAcceptNotice = `已更新${kind === 'direct' ? '单聊' : '群聊'}接收目录。`;
  } catch (error) { fileAcceptNotice = displayError(error); }
  if (page === 'settings') render();
}

async function resetRecvDir(kind: 'direct' | 'group'): Promise<void> {
  if (!isTauri()) return;
  try {
    fileRecv = await invoke<FileRecvSettings>('v1_set_file_recv_dir', { kind, path: null });
    fileAcceptNotice = `已恢复${kind === 'direct' ? '单聊' : '群聊'}默认接收目录。`;
  } catch (error) { fileAcceptNotice = displayError(error); }
  if (page === 'settings') render();
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

function currentConversation(): { kind: 'direct' | 'group' | 'self'; target: string; name: string } | null {
  if (groupChat.groupId) return { kind: 'group', target: groupChat.groupId, name: '群聊' };
  if (direct.peer) return { kind: 'direct', target: direct.peer, name: '单聊' };
  if (selfOpen) return { kind: 'self', target: 'self', name: '自己' };
  return null;
}

async function exportChatArchive(): Promise<void> {
  if (!isTauri() || chatArchiveBusy) return;
  const password = app.querySelector<HTMLInputElement>('#chat-archive-password')?.value ?? '';
  if (password.length < 8) { chatArchiveNotice = '聊天记录密码至少 8 个字符'; render(); return; }
  const conversation = currentConversation();
  if (!conversation) { chatArchiveNotice = '请先打开一个单聊、群聊或自聊'; render(); return; }
  const date = new Date();
  const stamp = `${date.getFullYear()}${String(date.getMonth() + 1).padStart(2, '0')}${String(date.getDate()).padStart(2, '0')}`;
  const target = await saveFileDialog({
    title: '导出加密聊天记录',
    defaultPath: `qbird-chat-${stamp}.qchat`,
    filters: [{ name: 'Qbird 聊天记录', extensions: ['qchat'] }],
  }).catch(() => null);
  if (!target) return;
  chatArchiveBusy = true; chatArchiveNotice = '正在导出…'; render();
  try {
    const info = await invoke<{ kind: string; name: string; message_count: number }>('v1_export_chat_archive',
      { kind: conversation.kind, target: conversation.target, password, path: target });
    chatArchiveNotice = `已导出 ${info.message_count} 条消息（${info.name}），附件未包含。`;
  } catch (error) { chatArchiveNotice = displayError(error); }
  finally { chatArchiveBusy = false; if (page === 'settings') render(); }
}

async function importChatArchive(): Promise<void> {
  if (!isTauri() || chatArchiveBusy) return;
  const password = app.querySelector<HTMLInputElement>('#chat-archive-password')?.value ?? '';
  if (password.length < 8) { chatArchiveNotice = '请输入导出时使用的密码'; render(); return; }
  const picked = await openFileDialog({ multiple: false, filters: [{ name: 'Qbird 聊天记录', extensions: ['qchat'] }] }).catch(() => null);
  if (!picked || typeof picked !== 'string') return;
  chatArchiveBusy = true; chatArchiveNotice = '正在导入…'; render();
  try {
    const report = await invoke<{ kind: string; name: string; inserted: number; skipped: number; conflicts: number }>('v1_import_chat_archive',
      { password, path: picked });
    chatArchiveNotice = `已导入「${report.name}」：新增 ${report.inserted} 条，跳过重复 ${report.skipped} 条${report.conflicts ? `，冲突 ${report.conflicts} 条（未覆盖）` : ''}。`;
    if (groupChat.groupId) void groupChat.load(false, true);
    if (direct.peer) void direct.load(false, true);
  } catch (error) { chatArchiveNotice = displayError(error); }
  finally { chatArchiveBusy = false; if (page === 'settings') render(); }
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

function shortId(value: string): string {
  const upper = (value || '').toUpperCase();
  return upper.length >= 8 ? `QB-${upper.slice(0, 4)}-${upper.slice(4, 8)}` : 'QB-0000-0000';
}

const USERNAME_PATTERN = /^[A-Za-z_][A-Za-z0-9_]{1,19}$/;
const USERNAME_HINT = '2–20 位英文字母、数字或下划线，不能以数字开头';

/// A random, rule-compliant suggestion. The nickname is deliberately not used
/// here: two colleagues called "Tam" would otherwise end up with the same
/// searchable username on one LAN.
function suggestUsername(): string {
  const alphabet = 'abcdefghijklmnopqrstuvwxyz0123456789';
  const random = new Uint32Array(6);
  crypto.getRandomValues(random);
  const tail = Array.from(random, (value) => alphabet[value % alphabet.length]).join('');
  return `qbird_${tail}`;
}

/* ---------- first run ---------- */

function onboardingSteps(): string {
  const steps = [
    [1, '你的资料', '同事如何认出你'],
    [2, '文件位置', '接收的文件存到哪'],
    [3, '找到同事', '同一网络里的人'],
  ] as const;
  return steps.map(([step, title, sub]) => `
    <li class="${onboardingStep === step ? 'current' : ''} ${onboardingStep > step ? 'done' : ''}">
      <span>0${step}</span><strong>${title}</strong><small>${sub}</small>
    </li>`).join('');
}

function onboardingBrand(): string {
  return `<aside class="ob-brand">
    <div class="ob-brand-top"><span class="ob-logo">Q</span><div><strong>Qbird 青鸟</strong><small>局域网聊天与文件</small></div></div>
    <div class="ob-brand-copy">
      <p class="ob-quote">同一张网，<br>说一句话就到。</p>
      <p class="ob-desc">不需要服务器，不经过云端。设备与设备直接通信，聊天记录只留在你自己的电脑上。</p>
    </div>
    <ol class="ob-steps">${onboardingSteps()}</ol>
    <p class="ob-brand-foot">设置之后都可以在「设置」里修改</p>
  </aside>`;
}

function onboardingView(): string {
  const draft = onboardingDraft;
  const step = onboardingStep;
  const dir = draft.recvDir || fileRecv.direct;
  return `<div class="onboard">
    ${onboardingBrand()}
    <div class="ob-main">
      ${step === 1 ? `
      <div class="ob-page">
        <p class="eyebrow">STEP 1 / 3</p>
        <h1>先认识一下你</h1>
        <p class="lede">这些信息只保存在本机，用来让同一局域网里的同事认出你。</p>
        <div class="ob-avatar-row">
          <div class="ob-avatar">${avatarMarkup(draft.nickname || onboarding?.profile.nickname || myNickname)}</div>
          <div><strong class="ob-avatar-name">${escapeText(draft.nickname || '你的头像')}</strong><p class="hint">头像可在「设置 → 个人资料」中上传，本轮先跳过。</p></div>
          <span class="ob-id">${qbirdId || 'QB-…'}</span>
        </div>
        <div class="field"><label for="ob-nickname">昵称 <span class="req">必填</span></label>
          <input id="ob-nickname" type="text" maxlength="24" autocomplete="off" value="${escapeText(draft.nickname)}" placeholder="例如：Tam">
          <p class="hint">同事在联系人列表里看到的名字，随时可改。</p></div>
        <div class="field"><label for="ob-username">用户名 <span class="req">必填</span></label>
          <div class="input-prefix"><span>@</span><input id="ob-username" type="text" maxlength="20" autocomplete="off" spellcheck="false" value="${escapeText(draft.username)}"></div>
          <p class="hint">局域网内用来搜索你：${USERNAME_HINT}，不能以数字开头。 <span id="ob-username-state" class="${USERNAME_PATTERN.test(draft.username) ? 'ok' : 'warn'}">${USERNAME_PATTERN.test(draft.username) ? '可用' : '待填写'}</span></p></div>
        <div class="field"><label for="ob-email">邮箱 <span class="opt">可选</span></label>
          <input id="ob-email" type="email" maxlength="128" autocomplete="off" value="${escapeText(draft.email)}" placeholder="仅保存在本机"></div>
        <div class="ob-actions"><span class="hint">昵称和用户名之后都可以修改</span>
          <button class="ob-primary" data-action="ob-next" data-step="2">下一步</button></div>
      </div>` : step === 2 ? `
      <div class="ob-page">
        <p class="eyebrow">STEP 2 / 3</p>
        <h1>接收的文件放哪里</h1>
        <p class="lede">同事发来的图片和文件会保存到这里。身份和聊天记录仍然保存在应用的 <code>data</code> 目录。</p>
        <div class="field"><label for="ob-path">保存位置</label>
          <div class="ob-path-row"><input id="ob-path" type="text" readonly value="${escapeText(dir || '读取中…')}"><button class="ob-ghost" data-action="ob-pick-dir">更改…</button><button class="ob-text" data-action="ob-reset-dir">恢复默认</button></div>
          <p class="hint">${draft.recvDir ? '已选择自定义目录 · ' : ''}目录须存在或可创建，并且当前用户可写。</p></div>
        <div class="ob-callout"><strong>建议不要选这些位置</strong><p>C:\\Program Files、Windows、磁盘根目录。卸载或覆盖安装不会删除已经接收的文件，但受保护目录里的文件可能写入失败。</p></div>
        <div class="ob-actions"><button class="ob-text" data-action="ob-back">返回</button><button class="ob-primary" data-action="ob-next" data-step="3">下一步</button></div>
      </div>` : `
      <div class="ob-page">
        <p class="eyebrow">STEP 3 / 3</p>
        <h1>同一网络里的同事</h1>
        <p class="lede">Qbird 会自动发现同一网段使用它的用户。添加后会出现在你的联系人里，可以直接开始聊天。</p>
        <div class="ob-scan"><span class="pulse"></span><span id="ob-lan-status">${escapeText(lanStatus)}</span><span class="demo-tag">未验证的局域网信息</span></div>
        <ul class="people" id="ob-people">${lanPeople.length ? '' : `<li class="people-empty">${lanLoading ? '正在扫描同网段…' : '暂未发现其他用户。可以稍后在「局域网用户」页手动添加地址。'}</li>`}</ul>
        <p class="hint">没看到想找的人？可以稍后在「局域网用户 → 其他网段与手动地址」中填写对方 IP。</p>
        <p class="ob-notice" role="status">${escapeText(onboardingNotice)}</p>
        <div class="ob-actions"><button class="ob-text" data-action="ob-finish">稍后再说</button>
          <button class="ob-primary" data-action="ob-finish" ${onboardingBusy ? 'disabled' : ''}>${onboardingBusy ? '正在完成…' : '完成，进入 Qbird'}</button></div>
      </div>`}
    </div>
  </div>`;
}

function paintOnboardingAvatar(): void {
  const box = app.querySelector<HTMLElement>('.ob-avatar');
  if (box) box.innerHTML = avatarMarkup(onboardingDraft.nickname);
}

function paintOnboarding(): void {
  if (page !== 'discover') page = 'discover';
  app.innerHTML = onboardingView();
  if (onboardingStep === 3) paintLanPeopleList(app.querySelector<HTMLUListElement>('#ob-people'), false);
  const status = app.querySelector<HTMLElement>('#ob-lan-status');
  if (status) status.textContent = lanStatus;
}

function syncOnboardingDraft(): void {
  const nickname = app.querySelector<HTMLInputElement>('#ob-nickname');
  const username = app.querySelector<HTMLInputElement>('#ob-username');
  const email = app.querySelector<HTMLInputElement>('#ob-email');
  if (nickname) onboardingDraft.nickname = nickname.value;
  if (username) onboardingDraft.username = username.value.trim();
  if (email) onboardingDraft.email = email.value;
}

function paintUsernameState(): void {
  const input = app.querySelector<HTMLInputElement>('#ob-username');
  const state = app.querySelector<HTMLElement>('#ob-username-state');
  if (!input || !state) return;
  const value = input.value.trim();
  const valid = USERNAME_PATTERN.test(value);
  state.textContent = valid ? '可用' : value ? `格式不正确：${USERNAME_HINT}` : '待填写';
  state.className = valid ? 'ok' : 'warn';
}

async function submitOnboarding(): Promise<void> {
  syncOnboardingDraft();
  const draft = onboardingDraft;
  const nickname = draft.nickname.trim();
  const username = draft.username.trim();
  if (!nickname) { onboardingNotice = '请先填写昵称'; paintOnboarding(); return; }
  if (!USERNAME_PATTERN.test(username)) { onboardingNotice = `用户名须为${USERNAME_HINT}`; paintOnboarding(); return; }
  onboardingBusy = true;
  onboardingNotice = '';
  paintOnboarding();
  try {
    const profile = await invoke<Profile>('v1_complete_onboarding', {
      nickname, username, email: draft.email.trim(), recvDir: draft.recvDir || null,
    });
    onboarding = { done: true, profile, qbird_id: qbirdId || shortId(fingerprint) };
    myNickname = profile.nickname;
    let listenNotice = '';
    try {
      // Launch-time auto listening is already running; refresh it so the new profile
      // is broadcast instead of the empty one from startup.
      try { await invoke('v1_refresh_listener'); }
      catch { await invoke('v1_start_chat_service', { port: networkSettings.default_port }); }
      listenNotice = '已开始监听局域网，同事可以发现你。';
    } catch {
      listenNotice = '监听未启动：端口可能被占用，可在局域网用户页重试。';
    }
    onboardingBusy = false;
    page = 'contacts';
    contactNotice = listenNotice;
    render();
    void refreshContacts();
    void refreshProbeStatus();
  } catch (error) {
    onboardingBusy = false;
    onboardingNotice = displayError(error);
    paintOnboarding();
  }
}

/* ---------- LAN people ---------- */

function lanPersonRow(person: LanPerson, allowOpen = true): string {
  const name = person.nickname || person.username || `Qbird 用户 ${person.qbird_id.slice(-4)}`;
  const initial = escapeText(name.slice(0, 1).toUpperCase());
  const handle = person.username ? `<span class="p-user">@${escapeText(person.username)}</span>` : `<span class="p-user">${person.qbird_id}</span>`;
  const meta = `<span>${escapeText(person.address)}</span><span class="sep">·</span><span>${person.verified ? '已核对指纹' : '身份未核对'}</span>`;
  return `<li data-fingerprint="${person.fingerprint}" class="${person.known ? 'is-added' : ''}">
    <span class="av av-40">${initial}</span>
    <div class="p-body"><div class="p-name"><strong>${escapeText(name)}</strong>${handle}</div><div class="p-meta">${meta}</div></div>
    ${person.known
      ? (allowOpen
        ? `<button class="btn-add is-added" data-action="open-direct" data-peer="${person.fingerprint}" data-name="${escapeText(person.nickname || name)}">发消息</button>`
        : '<span class="btn-add is-added" role="status">已添加</span>')
      : `<button class="btn-add" data-action="lan-add" data-address="${person.address}" data-fingerprint="${person.fingerprint}" data-name="${escapeText(name)}" data-username="${person.username ? escapeText(person.username) : ''}" ${lanAddBusy === person.fingerprint ? 'disabled' : ''}>${lanAddBusy === person.fingerprint ? '添加中…' : '添加'}</button>`}
  </li>`;
}

function paintLanPeopleList(list: HTMLUListElement | null, allowOpen = true): void {
  if (!list) return;
  const query = lanQuery.trim().toLowerCase();
  const rows = lanPeople.filter((person) => !query
    || (person.nickname || '').toLowerCase().includes(query)
    || (person.username || '').toLowerCase().includes(query)
    || person.qbird_id.toLowerCase().includes(query));
  list.innerHTML = rows.length
    ? rows.map((person) => lanPersonRow(person, allowOpen)).join('')
    : `<li class="people-empty">${lanLoading ? '正在识别同网段用户…' : lanPeople.length ? '没有匹配的用户' : '暂未发现其他用户。请确认对方已启动 Qbird，且与本机在同一网段。'}</li>`;
}

function discoveryView(): string {
  const since = lanRefreshedAt ? `${Math.max(1, Math.round((Date.now() - lanRefreshedAt) / 1000))} 秒前刷新` : '尚未刷新';
  const subnet = lanPeople[0]?.address.replace(/\.\d+:\d+$/, '.0/24') || '同网段';
  return `<section class="lan-page">
    <header class="lan-head">
      <div><span class="eyebrow">DISCOVERY / 局域网</span><h2>局域网用户</h2>
        <p>他们正在使用 Qbird，并且和你在同一个网络。</p></div>
      <span class="pill ${probeStatus.running ? 'pill-live' : ''}">${probeStatus.running ? `<span class="pulse"></span>监听中 · ${probeStatus.port}` : '未监听'}</span>
    </header>
    <div class="lan-bar">
      <span class="lan-net">网段 <code>${escapeText(subnet)}</code></span>
      <input id="lan-search" type="search" maxlength="64" placeholder="搜索昵称或用户名" aria-label="搜索局域网用户" value="${escapeText(lanQuery)}">
      <button class="outline" data-action="lan-refresh" ${lanLoading ? 'disabled' : ''}>${lanLoading ? '刷新中…' : '立即刷新'}</button>
      ${probeStatus.running
        ? '<button class="outline" data-action="lan-stop">停止监听</button>'
        : '<button class="outline" data-action="lan-start">开始监听</button>'}
    </div>
    <p id="lan-status" class="lan-status" role="status">${escapeText(lanStatus)} · ${since}</p>
    <ul id="lan-list" class="people lan-list"></ul>

    <details class="lan-advanced">
      <summary>其他网段与手动地址</summary>
      <div class="lan-advanced-body">
        <div class="field"><label for="lan-manual">手动添加地址</label>
          <div class="probe-row"><input id="lan-manual" type="text" placeholder="192.168.2.8:42853"><input id="lan-manual-name" type="text" maxlength="24" placeholder="对方昵称（可选）"><button class="outline" data-action="lan-manual-add">探测并添加</button></div>
          <p class="hint">适用于跨 VLAN、VPN 或已知 IP 的同事。添加前会探测地址，确认对方身份与列表一致。</p></div>
        <div class="field"><label for="scan-cidr">扫描指定网段</label>
          <div class="probe-row"><input id="scan-cidr" type="text" maxlength="24" spellcheck="false" placeholder="192.168.2.0/24" value="${escapeText(scanCidr)}"><button class="outline" data-action="scan-start" ${scanBusy ? 'disabled' : ''}>${scanBusy ? '扫描中…' : '开始扫描'}</button></div>
          <p class="hint">限速扫描（4 并发、单地址 400 毫秒），只用于发现候选。结果仍需点「添加」并探测确认。</p>
          <p id="scan-status" class="hint" role="status">${escapeText(scanNotice)}</p>
          <ul class="people">${scanHits.map((hit) => `<li><span class="av av-40">?</span><div class="p-body"><div class="p-name"><strong>${escapeText(hit.address)}</strong></div><div class="p-meta"><span>扫描候选 · ${escapeText(hit.fingerprint.slice(0, 8))}…</span></div></div><button class="btn-add" data-action="lan-add" data-address="${escapeText(hit.address)}" data-fingerprint="${hit.fingerprint}" data-name="Qbird 用户 ${escapeText(hit.fingerprint.slice(0, 4))}">添加</button></li>`).join('')}</ul>
          <label for="scan-block">黑名单（IP 或 CIDR，逗号分隔）</label>
          <div class="probe-row"><input id="scan-block" type="text" maxlength="512" spellcheck="false" value="${escapeText(scanBlacklistDraft)}"><button class="outline" data-action="save-scan-blacklist">保存</button></div>
          <p class="hint" role="status">${escapeText(scanBlacklistNotice)}</p></div>
      </div>
    </details>

    <div class="lan-identity">
      <span class="lan-shield">${icon('lock')}</span>
      <div><strong>关于身份</strong><p>添加时以 Qbird ID 识别用户，昵称和用户名可以随时修改，ID 不变。需要更强确认时，可以核对完整身份指纹。</p></div>
      <button class="outline" data-action="lan-identity">查看我的身份</button>
    </div>
    <p class="hint">${escapeText(lanNotice)}</p>
  </section>`;
}

function paintLan(): void {
  paintLanPeopleList(app.querySelector<HTMLUListElement>('#lan-list'));
  const status = app.querySelector<HTMLElement>('#lan-status');
  if (status) {
    const since = lanRefreshedAt ? `${Math.max(1, Math.round((Date.now() - lanRefreshedAt) / 1000))} 秒前刷新` : '尚未刷新';
    status.textContent = `${lanStatus} · ${since}`;
  }
}

async function refreshLanPeople(force = false): Promise<void> {
  if (!isTauri() || lanLoading) return;
  if (!force && lanRefreshedAt && Date.now() - lanRefreshedAt < 25_000) return;
  lanLoading = true;
  lanStatus = '正在识别同网段用户…';
  if (page === 'discover') paintLan();
  if (!onboarding?.done && onboardingStep === 3) paintOnboarding();
  try {
    lanPeople = await invoke<LanPerson[]>('v1_lan_people');
    lanRefreshedAt = Date.now();
    lanStatus = lanPeople.length ? `发现 ${lanPeople.length} 位用户` : '暂未发现其他用户';
  } catch {
    lanPeople = [];
    lanStatus = '局域网发现不可用，请检查网络和防火墙';
  } finally {
    lanLoading = false;
    if (page === 'discover') paintLan();
    if (!onboarding?.done && onboardingStep === 3) paintOnboarding();
  }
}

async function addLanPerson(button: HTMLButtonElement): Promise<void> {
  if (!isTauri() || lanAddBusy) return;
  const { address, fingerprint, name, username } = button.dataset;
  if (!address || !fingerprint) return;
  lanAddBusy = fingerprint;
  lanNotice = '';
  paintLan();
  paintOnboarding();
  try {
    await invoke('v1_add_contact', { address, fingerprint, displayName: name || 'Qbird 用户', username: username || null });
    const person = lanPeople.find((item) => item.fingerprint === fingerprint);
    if (person) { person.known = true; person.verified = true; }
    lanNotice = `已添加「${name || 'Qbird 用户'}」到联系人。对方也需要添加你才能互发消息。`;
  } catch (error) {
    lanNotice = displayError(error);
  } finally {
    lanAddBusy = '';
    paintLan();
    paintOnboarding();
    if (onboardingStep === 3) paintLanPeopleList(app.querySelector<HTMLUListElement>('#ob-people'), false);
    void refreshContacts();
  }
}

async function pickOnboardingDir(): Promise<void> {
  if (!isTauri()) return;
  const picked = await openFileDialog({ directory: true, multiple: false, title: '选择接收文件保存位置' }).catch(() => null);
  if (!picked || typeof picked !== 'string') return;
  onboardingDraft.recvDir = picked;
  paintOnboarding();
}

async function toggleLanListener(start: boolean): Promise<void> {
  if (!isTauri()) return;
  lanNotice = start ? '正在启动监听…' : '正在停止监听…';
  paintLan();
  try {
    if (start) await invoke('v1_start_chat_service', { port: networkSettings.default_port });
    else await invoke('v1_stop_probe_service');
    await refreshProbeStatus();
    lanNotice = start ? '已开始监听，同事可以发现你。' : '已停止监听，本次运行不再自动重启；待发消息仍保存在本机。';
  } catch (error) {
    lanNotice = displayError(error);
  }
  paintLan();
}

async function addManualLan(): Promise<void> {
  const address = app.querySelector<HTMLInputElement>('#lan-manual')?.value.trim() || '';
  const name = app.querySelector<HTMLInputElement>('#lan-manual-name')?.value.trim() || '';
  if (!address) { lanNotice = '请输入对方的 IP 和端口，例如 192.168.2.8:42853'; paintLan(); return; }
  lanNotice = '正在探测该地址…';
  paintLan();
  try {
    const fingerprint = await invoke<string>('v1_probe_peer', { address });
    await invoke('v1_add_contact', { address, fingerprint, displayName: name || address, username: null });
    lanNotice = `已添加 ${name || address} 到联系人。`;
    await refreshLanPeople(true);
  } catch (error) {
    lanNotice = displayError(error);
  }
  paintLan();
  void refreshContacts();
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

async function loadGroupAvatarIcon(groupId: string, icon: HTMLImageElement): Promise<void> {
  try {
    const bytes = await invoke<number[] | null>('v1_group_avatar', { groupId });
    if (bytes && bytes.length) icon.src = URL.createObjectURL(new Blob([toArrayBuffer(new Uint8Array(bytes))], { type: 'image/png' }));
    else icon.hidden = true;
  } catch { icon.hidden = true; }
}

async function refreshPlugins(): Promise<void> {
  if (!isTauri()) return;
  try { plugins = await invoke<PluginView[]>('v1_list_plugins'); }
  catch { pluginNotice = '无法读取插件列表'; }
}

async function installPlugin(): Promise<void> {
  if (!isTauri()) return;
  const picked = await openFileDialog({ directory: true, multiple: false, title: '选择包含 plugin.json 的插件文件夹' }).catch(() => null);
  if (!picked || typeof picked !== 'string') return;
  pluginNotice = '正在安装…';
  try {
    const view = await invoke<PluginView>('v1_install_plugin', { path: picked });
    pluginNotice = `已安装「${view.name}」，可预览：${view.extensions.join('、') || '无'}`;
  } catch (error) { pluginNotice = displayError(error); }
  await refreshPlugins();
  render();
}

async function togglePlugin(id: string, enabled: boolean): Promise<void> {
  if (!isTauri()) return;
  try {
    await invoke('v1_set_plugin_enabled', { id, enabled });
    pluginNotice = enabled ? '插件已启用。' : '插件已停用。';
  } catch (error) { pluginNotice = displayError(error); }
  await refreshPlugins();
  render();
}

/// Two clicks: the first arms the removal so a stray click cannot delete a plugin.
async function removePlugin(id: string): Promise<void> {
  if (!isTauri()) return;
  if (pluginRemovePending !== id) {
    pluginRemovePending = id;
    pluginNotice = '再次点击“确认移除”会删除该插件目录。';
    render();
    return;
  }
  pluginRemovePending = '';
  try {
    await invoke('v1_remove_plugin', { id });
    pluginNotice = '插件已移除。';
  } catch (error) { pluginNotice = displayError(error); }
  await refreshPlugins();
  render();
}

async function refreshHover(): Promise<void> {
  if (!isTauri()) return;
  try { hover = await invoke<{ show_ip: boolean; show_mac: boolean }>('v1_hover_settings'); }
  catch { hoverNotice = '无法读取悬浮信息设置'; }
}

async function saveHover(): Promise<void> {
  if (!isTauri()) return;
  const showIp = app.querySelector<HTMLInputElement>('#hover-show-ip')?.checked ?? true;
  const showMac = app.querySelector<HTMLInputElement>('#hover-show-mac')?.checked ?? false;
  try {
    hover = await invoke<{ show_ip: boolean; show_mac: boolean }>('v1_set_hover_settings', { showIp, showMac });
    hoverNotice = '悬浮信息设置已保存。';
  } catch (error) { hoverNotice = displayError(error); }
  render();
}

async function refreshImageOpenMode(): Promise<void> {
  if (!isTauri()) return;
  try {
    const mode = await invoke<'lightbox' | 'workspace'>('v1_image_open_mode');
    applyImageOpenMode(mode);
  } catch { imageOpenNotice = '无法读取图片打开方式'; }
}

function applyImageOpenMode(mode: 'lightbox' | 'workspace'): void {
  imageOpenMode = mode;
  direct.imageOpenMode = mode;
  groupChat.imageOpenMode = mode;
}

async function saveImageOpenMode(): Promise<void> {
  if (!isTauri()) return;
  const mode = app.querySelector<HTMLSelectElement>('#image-open-mode')?.value === 'workspace' ? 'workspace' : 'lightbox';
  try {
    applyImageOpenMode(await invoke<'lightbox' | 'workspace'>('v1_set_image_open_mode', { mode }));
    imageOpenNotice = mode === 'workspace' ? '已改为在右侧工作区打开图片。' : '已改为灯箱查看图片。';
  } catch (error) { imageOpenNotice = displayError(error); }
  render();
}

async function refreshFileOpenMode(): Promise<void> {
  if (!isTauri()) return;
  try {
    applyFileOpenMode(await invoke<'workspace' | 'system'>('v1_file_open_mode'));
  } catch { fileOpenNotice = '无法读取附件打开方式'; }
}

function applyFileOpenMode(mode: 'workspace' | 'system'): void {
  fileOpenMode = mode;
  direct.fileOpenMode = mode;
  groupChat.fileOpenMode = mode;
}

async function saveFileOpenMode(): Promise<void> {
  if (!isTauri()) return;
  const mode = app.querySelector<HTMLSelectElement>('#file-open-mode')?.value === 'system' ? 'system' : 'workspace';
  try {
    applyFileOpenMode(await invoke<'workspace' | 'system'>('v1_set_file_open_mode', { mode }));
    fileOpenNotice = mode === 'system' ? '已改为用默认程序打开附件。' : '已改为在右侧工作区预览附件。';
  } catch (error) { fileOpenNotice = displayError(error); }
  render();
}

let workspaceRestored = false;

/// Restores the persisted tab layout once, and only while the app is unlocked so the
/// panel cannot appear over the lock screen.
async function restoreWorkspace(): Promise<void> {
  if (workspaceRestored || !isTauri() || lockState.locked) return;
  workspaceRestored = true;
  await workspace.restore();
}

async function refreshTeam(): Promise<void> {
  if (!isTauri()) return;
  try { team = await invoke<{ name: string; has_logo: boolean }>('v1_team_profile'); }
  catch { teamNotice = '无法读取团队设置'; return; }
  try {
    const bytes = await invoke<number[] | null>('v1_team_logo');
    setTeamLogo(bytes && bytes.length ? new Uint8Array(bytes) : null);
  } catch { setTeamLogo(null); }
}

function setTeamLogo(bytes: Uint8Array | null): void {
  if (teamLogoUrl) { URL.revokeObjectURL(teamLogoUrl); teamLogoUrl = null; }
  teamLogoUrl = bytes && bytes.length ? URL.createObjectURL(new Blob([toArrayBuffer(bytes)], { type: 'image/png' })) : null;
}

async function saveTeamName(): Promise<void> {
  if (!isTauri()) return;
  const name = app.querySelector<HTMLInputElement>('#team-name')?.value ?? '';
  try {
    team = await invoke<{ name: string; has_logo: boolean }>('v1_set_team_name', { name });
    teamNotice = team.name ? '团队名称已保存。' : '已恢复默认名称。';
  } catch (error) { teamNotice = displayError(error); }
  render();
}

async function chooseTeamLogo(): Promise<void> {
  if (!isTauri()) return;
  try {
    const picked = await openFileDialog({ multiple: false, filters: [{ name: '图片', extensions: ['png', 'jpg', 'jpeg'] }] });
    if (!picked || typeof picked !== 'string') return;
    const source = new Uint8Array(await invoke<number[]>('v1_read_image_file', { path: picked }));
    const cropped = await cropSquare(source, 256);
    await invoke('v1_set_team_logo', { bytes: Array.from(cropped) });
    team.has_logo = true; setTeamLogo(cropped); teamNotice = '团队 Logo 已保存。';
  } catch (error) { teamNotice = displayError(error); }
  render();
}

async function clearTeamLogo(): Promise<void> {
  if (!isTauri()) return;
  try {
    await invoke('v1_clear_team_logo');
    team.has_logo = false; setTeamLogo(null); teamNotice = '团队 Logo 已移除。';
  } catch (error) { teamNotice = displayError(error); }
  render();
}

/// Owner-only; the crop keeps the PNG small enough for the 96 KiB wire and storage cap.
async function chooseGroupAvatar(): Promise<void> {
  if (!isTauri() || !groupChat.groupId) return;
  try {
    const picked = await openFileDialog({ multiple: false, filters: [{ name: '图片', extensions: ['png', 'jpg', 'jpeg'] }] });
    if (!picked || typeof picked !== 'string') return;
    const source = new Uint8Array(await invoke<number[]>('v1_read_image_file', { path: picked }));
    const cropped = await cropAvatarInteractively(source, {
      title: '调整群头像',
      hint: '拖动图片调整位置，滚轮或滑块缩放；群头像会压缩到 96 KiB 以内再发送。',
      maxBytes: 96 * 1024,
    });
    if (!cropped) return;
    await invoke('v1_set_group_avatar', { groupId: groupChat.groupId, bytes: Array.from(cropped) });
    await groupChat.loadAvatar();
  } catch (error) { groupChat.notify(displayError(error)); }
}

async function clearGroupAvatar(): Promise<void> {
  if (!isTauri() || !groupChat.groupId) return;
  try {
    await invoke('v1_set_group_avatar', { groupId: groupChat.groupId, bytes: [] });
    await groupChat.loadAvatar();
  } catch (error) { groupChat.notify(displayError(error)); }
}

async function searchGroups(): Promise<void> {
  if (!isTauri()) return;
  const query = app.querySelector<HTMLInputElement>('#group-search-name')?.value.trim() ?? '';
  if (!query) { groupSearchStatus = '请输入群名关键词'; groupSearchHits = []; paintGroupSearch(); return; }
  groupSearchBusy = true; groupSearchHits = []; groupSearchStatus = '正在向已确认联系人查询…'; paintGroupSearch();
  try {
    groupSearchHits = await invoke<GroupSearchHit[]>('v1_search_groups', { query });
    groupSearchStatus = groupSearchHits.length ? `找到 ${groupSearchHits.length} 个群` : '没有找到匹配的群；需要联系人同时在线。';
  } catch (error) { groupSearchStatus = displayError(error); }
  groupSearchBusy = false; paintGroupSearch();
}

async function requestJoin(groupId: string, name: string, owner: string): Promise<void> {
  if (!isTauri()) return;
  try {
    await invoke('v1_request_join_group', { groupId, name, owner });
    joinNotice = `已向群主提交加入「${name}」的申请。`;
  } catch (error) { joinNotice = displayError(error); }
  await refreshJoinRequests();
}

async function refreshJoinRequests(): Promise<void> {
  if (!isTauri()) return;
  try { joinRequests = await invoke<{ incoming: JoinRequest[]; outgoing: JoinRequest[] }>('v1_join_requests', {}); }
  catch { joinNotice = '无法读取入群申请'; }
  paintJoinRequests();
}

async function resolveJoin(id: string, accept: boolean): Promise<void> {
  if (!isTauri()) return;
  try {
    await invoke('v1_resolve_join_request', { id, accept });
    joinNotice = accept ? '已同意加入，正在通知群成员。' : '已拒绝该申请。';
  } catch (error) { joinNotice = displayError(error); }
  await refreshJoinRequests();
  await refreshContacts();
}

function paintJoinRequests(): void {
  const box = app.querySelector<HTMLElement>('#join-requests');
  if (!box) return;
  box.replaceChildren();
  if (!joinRequests.incoming.length && !joinRequests.outgoing.length && !joinNotice) { box.hidden = true; return; }
  box.hidden = false;
  const heading = (text: string): HTMLElement => { const node = document.createElement('p'); node.className = 'file-meta'; node.textContent = text; return node; };
  if (joinRequests.incoming.length) {
    box.append(heading('待处理的入群申请'));
    for (const request of joinRequests.incoming) {
      const row = document.createElement('div'); row.className = 'offer-card';
      const text = document.createElement('div'); text.className = 'offer-text';
      const title = document.createElement('div'); title.className = 'file-title';
      title.textContent = `${request.applicant_name || request.applicant.slice(0, 12)} 申请加入「${request.group_name}」`;
      const meta = document.createElement('small'); meta.className = 'file-meta'; meta.textContent = new Date(request.created_at * 1000).toLocaleString();
      text.append(title, meta);
      const actions = document.createElement('div'); actions.className = 'file-actions';
      const accept = document.createElement('button'); accept.className = 'outline'; accept.textContent = '同意';
      accept.dataset.action = 'accept-join'; accept.dataset.id = request.id;
      const reject = document.createElement('button'); reject.className = 'linkish'; reject.textContent = '拒绝';
      reject.dataset.action = 'reject-join'; reject.dataset.id = request.id;
      actions.append(accept, reject); row.append(text, actions); box.append(row);
    }
  }
  if (joinRequests.outgoing.length) {
    box.append(heading('我提交的申请'));
    for (const request of joinRequests.outgoing) {
      const row = document.createElement('div'); row.className = 'offer-card';
      const title = document.createElement('div'); title.className = 'file-title';
      title.textContent = `${request.group_name} · 等待群主处理`;
      row.append(title); box.append(row);
    }
  }
  if (joinNotice) { const note = document.createElement('p'); note.setAttribute('role', 'status'); note.textContent = joinNotice; box.append(note); }
}

function paintGroupSearch(): void {
  const status = app.querySelector<HTMLElement>('#group-search-status');
  const list = app.querySelector<HTMLUListElement>('#group-search-results');
  if (status) status.textContent = groupSearchStatus;
  if (!list) return;
  list.replaceChildren();
  for (const hit of groupSearchHits) {
    const item = document.createElement('li');
    const title = document.createElement('span');
    title.className = 'contact-title';
    title.textContent = `${hit.name} · ${hit.member_count} 人 · 群主 ${hit.owner.slice(0, 12)}…`;
    item.append(title);
    const actions = document.createElement('div'); actions.className = 'contact-actions';
    const button = document.createElement('button'); button.className = 'outline'; button.textContent = '申请加入';
    button.dataset.action = 'join-group';
    button.dataset.group = hit.group_id; button.dataset.name = hit.name; button.dataset.owner = hit.owner;
    button.disabled = !hit.owner_known;
    button.title = hit.owner_known ? '向群主发送加入申请' : '群主不在本机已确认联系人中，无法直接申请';
    actions.append(button); item.append(actions); list.append(item);
  }
}


/* ---------- contacts page: sidebar sections + card lists ---------- */

function presenceDot(fingerprint: string, label = true): string {
  const online = presence.get(fingerprint) === true;
  return `<span class="presence-dot ${online ? 'online' : ''}" title="${online ? '在线：局域网可见或地址可连接' : '离线：当前不可达'}" aria-label="${online ? '在线' : '离线'}"></span>`
    + (label ? `<span class="presence-text">${online ? '在线' : '离线'}</span>` : '');
}

function contactInitial(name: string): string {
  const trimmed = name.trim();
  return trimmed ? trimmed.slice(0, 1).toUpperCase() : '?';
}

function sideContactRow(contact: Contact): string {
  const name = contact.remark || contact.display_name;
  const sub = contact.username ? `@${contact.username}` : shortId(contact.fingerprint);
  return `<li><button class="side-row" data-action="open-direct" data-peer="${contact.fingerprint}" data-name="${escapeText(name)}" title="${escapeText(name)}">
    <span class="av av-32">${escapeText(contactInitial(name))}</span>
    <span class="side-row-body"><strong>${escapeText(name)}${contact.trust_state === 'verified' ? presenceDot(contact.fingerprint, false) : ''}</strong><small>${escapeText(sub)}${contact.trust_state === 'verified' ? '' : ' · 待确认'}</small></span>
  </button></li>`;
}

function sideGroupRow(group: Group): string {
  return `<li><button class="side-row" data-action="open-group" data-group="${group.id}" data-name="${escapeText(group.name)}" title="${escapeText(group.name)}">
    <span class="av av-32 av-group">群</span>
    <span class="side-row-body"><strong>${escapeText(group.name)}</strong><small>${group.member_count} 位成员</small></span>
  </button></li>`;
}

function contactSidebarView(): string {
  const verified = contactsCache.filter((contact) => contact.trust_state === 'verified');
  const friends = contactsCache.length
    ? contactsCache.map(sideContactRow).join('')
    : '<li class="side-list-empty">还没有好友，去「添加联系人」发现同网段同事。</li>';
  const groupsHtml = groups.length
    ? groups.map(sideGroupRow).join('')
    : '<li class="side-list-empty">还没有群聊。</li>';
  return `<div class="side-section">
      <div class="side-section-head"><span>好友</span><small id="side-contact-count">${verified.length}</small></div>
      <ul id="side-contacts" class="side-list"><li><button class="side-row" data-action="open-self"><span class="av av-32">我</span><span class="side-row-body"><strong>自己</strong><small>本地自聊</small></span></button></li>${friends}</ul>
    </div>
    <div class="side-section">
      <div class="side-section-head"><span>群聊</span><small id="side-group-count">${groups.length}</small></div>
      <ul id="side-groups" class="side-list">${groupsHtml}</ul>
    </div>`;
}

function contactCardRow(contact: Contact): string {
  const name = contact.remark || contact.display_name;
  const verified = contact.trust_state === 'verified';
  const meta = [contact.username ? `@${contact.username}` : null, `QBird ID ${shortId(contact.fingerprint)}`, contact.category ? `分组 ${contact.category}` : null]
    .filter(Boolean).join(' · ');
  return `<li class="contact-card contact-row" draggable="true" data-fingerprint="${contact.fingerprint}" data-name="${escapeText(contact.display_name)}" data-trust="${contact.trust_state}" title="拖到「群聊」里的群可邀请入群">
    <span class="av av-44">${escapeText(contactInitial(name))}</span>
    <div class="contact-card-body">
      <div class="contact-card-title"><strong>${escapeText(name)}</strong>${verified ? presenceDot(contact.fingerprint) : ''}<span class="chip ${verified ? 'chip-ok' : 'chip-warn'}">${verified ? '已添加' : '待确认'}</span></div>
      ${contact.remark && contact.remark !== contact.display_name ? `<div class="contact-card-sub">昵称 ${escapeText(contact.display_name)}</div>` : ''}
      <div class="contact-card-sub">${escapeText(meta)}</div>
      <details class="contact-card-more"><summary>备注与分组</summary>
        <div class="probe-row">
          <input type="text" maxlength="64" placeholder="备注" value="${escapeText(contact.remark ?? '')}" data-remark-for="${contact.fingerprint}" aria-label="联系人备注">
          <input type="text" maxlength="64" placeholder="分组" value="${escapeText(contact.category ?? '')}" data-category-for="${contact.fingerprint}" aria-label="联系人分组">
          <button class="outline" data-action="save-contact-profile" data-peer="${contact.fingerprint}">保存</button>
        </div>
      </details>
    </div>
    <div class="contact-card-actions">
      ${verified
        ? `<button class="outline" data-action="open-direct" data-peer="${contact.fingerprint}" data-name="${escapeText(name)}">发消息</button>`
        : `<button class="outline" data-action="accept-contact" data-peer="${contact.fingerprint}">接受</button><button class="linkish" data-action="decline-contact" data-peer="${contact.fingerprint}">删除</button>`}
    </div>
  </li>`;
}

function groupCardRow(group: Group): string {
  return `<li class="contact-card" data-group-drop="${group.id}">
    <span class="av av-44 av-group">群</span>
    <div class="contact-card-body">
      <div class="contact-card-title"><strong>${escapeText(group.name)}</strong>${group.is_owner ? '<span class="chip">我创建的</span>' : ''}</div>
      <div class="contact-card-sub">${group.member_count} 位成员 · ${escapeText(group.last_body || '暂无消息')}</div>
    </div>
    <div class="contact-card-actions">
      <button class="outline" data-action="open-group" data-group="${group.id}" data-name="${escapeText(group.name)}">打开群聊</button>
    </div>
  </li>`;
}

function groupTabView(): string {
  return `<section class="contact-group-pane">
    <ul id="groups-list" class="contact-cards"></ul>
    <div class="probe-box" id="group-invite-box"><h3>群聊邀请</h3><div id="group-invitations"></div></div>
    <div class="probe-box"><h3>新建群聊</h3>
      <label for="group-name">群聊名称</label><input id="group-name" type="text" maxlength="128" placeholder="例如：项目组">
      <div id="group-members-pick" class="group-pick"></div>
      <button class="outline" data-action="create-group">创建群聊</button>
      <p id="group-create-status" aria-live="polite"></p>
      <small>只能邀请已确认联系人；新成员只能看到加入之后的消息。也可以把左侧好友拖到群卡片上邀请。</small>
    </div>
    <div class="probe-box"><h3>加入与设置</h3>
      <label><input id="auto-join-groups" type="checkbox" ${autoJoinGroups ? 'checked' : ''}> 被拉进群时自动加入</label>
      <div id="join-requests"></div>
      <label for="group-search-name">按群名搜索并申请加入</label>
      <div class="probe-row"><input id="group-search-name" type="text" maxlength="64" placeholder="群名关键词"><button class="outline" data-action="search-groups" ${groupSearchBusy ? 'disabled' : ''}>搜索</button></div>
      <p id="group-search-status" role="status">${escapeText(groupSearchStatus)}</p>
      <ul id="group-search-results" class="discovery-results"></ul>
      <small>只会向已确认且有地址的联系人查询；申请直接发给群主。对方离线时不保证送达。</small>
    </div>
    <div class="probe-box"><h3>已退出的群聊</h3>
      <p class="hint">退出后消息记录仍保留在本机，可随时查看或导出。</p>
      <ul id="left-groups-list" class="discovery-results"></ul>
    </div>
  </section>`;
}

function contactsPageView(): string {
  const verified = contactsCache.filter((contact) => contact.trust_state === 'verified').length;
  return `<section class="contacts-page">
    <header class="contacts-head">
      <div>
        <span class="eyebrow">CONTACTS / 联系人</span>
        <h2>联系人</h2>
        <p>联系人保存在本机；添加是单向的，对方也添加你之后才能互发消息。</p>
      </div>
      <div class="segmented" role="tablist" aria-label="联系人与群聊">
        <button role="tab" class="${contactTab === 'direct' ? 'active' : ''}" data-action="contact-tab" data-tab="direct" aria-selected="${contactTab === 'direct'}">单聊 · ${verified}</button>
        <button role="tab" class="${contactTab === 'group' ? 'active' : ''}" data-action="contact-tab" data-tab="group" aria-selected="${contactTab === 'group'}">群聊 · ${groups.length}</button>
      </div>
    </header>
    ${contactTab === 'direct' ? `<div class="contacts-toolbar">
        <input id="contact-search" type="search" maxlength="64" placeholder="搜索用户名、昵称或备注" value="${escapeText(contactQuery)}" aria-label="搜索联系人">
        <button class="outline" data-action="contacts-add">添加联系人</button>
      </div>
      <p id="contacts-status" role="status" aria-live="polite"></p>
      <ul id="contacts-list" class="contact-cards"></ul>`
    : `<p id="contacts-status" role="status" aria-live="polite"></p>${groupTabView()}`}
  </section>`;
}

function paintContactsSidebar(): void {
  const friendList = app.querySelector<HTMLUListElement>('#side-contacts');
  const groupList = app.querySelector<HTMLUListElement>('#side-groups');
  const friendCount = app.querySelector<HTMLElement>('#side-contact-count');
  const groupCount = app.querySelector<HTMLElement>('#side-group-count');
  const verified = contactsCache.filter((contact) => contact.trust_state === 'verified');
  if (friendCount) friendCount.textContent = String(verified.length);
  if (groupCount) groupCount.textContent = String(groups.length);
  if (friendList) friendList.innerHTML = `<li><button class="side-row" data-action="open-self"><span class="av av-32">我</span><span class="side-row-body"><strong>自己</strong><small>本地自聊</small></span></button></li>`
    + (contactsCache.length ? contactsCache.map(sideContactRow).join('') : '<li class="side-list-empty">还没有好友，点「添加联系人」发现同网段同事。</li>');
  if (groupList) groupList.innerHTML = groups.length ? groups.map(sideGroupRow).join('') : '<li class="side-list-empty">还没有群聊。</li>';
}

function paintContactCards(): void {
  const list = app.querySelector<HTMLUListElement>('#contacts-list');
  if (!list) return;
  const query = contactQuery.trim().toLowerCase();
  const rows = contactsCache.filter((contact) => !query
    || contact.display_name.toLowerCase().includes(query)
    || (contact.username ?? '').toLowerCase().includes(query)
    || (contact.remark ?? '').toLowerCase().includes(query));
  if (query) {
    const status = app.querySelector<HTMLElement>('#contacts-status');
    if (status) status.textContent = rows.length ? `匹配 ${rows.length} 位联系人` : '没有匹配的联系人';
  }
  if (!rows.length) {
    list.innerHTML = `<li class="contact-card-empty">${query ? '没有匹配的联系人。' : '还没有联系人，点右上角「添加联系人」去局域网用户页添加。'}</li>`;
    return;
  }
  list.innerHTML = rows.map(contactCardRow).join('');
}

/// Presence is advisory: LAN-visible or the saved address accepts a connection.
async function refreshPresence(): Promise<void> {
  if (!isTauri()) return;
  try {
    const rows = await invoke<{ fingerprint: string; online: boolean }[]>('v1_contact_presence');
    const next = new Map(rows.map((row) => [row.fingerprint, row.online] as const));
    const changed = next.size !== presence.size || rows.some((row) => presence.get(row.fingerprint) !== row.online);
    presence = next;
    if (!changed) { paintPresenceHeader(); return; }
  } catch { return; }
  paintContactCards();
  paintContactsSidebar();
  paintPresenceHeader();
}

function paintPresenceHeader(): void {
  const dot = app.querySelector<HTMLElement>('#direct-presence');
  if (!dot) return;
  const peer = direct.peer;
  dot.hidden = !peer;
  if (!peer) return;
  const online = presence.get(peer) === true;
  dot.classList.toggle('online', online);
  dot.title = online ? '在线：局域网可见或地址可连接' : '离线：当前不可达';
  const text = app.querySelector<HTMLElement>('#direct-presence-text');
  if (text) text.textContent = online ? '在线' : '离线';
}

async function acceptContact(peer: string): Promise<void> {
  try {
    await invoke('v1_accept_contact', { fingerprint: peer });
    contactNotice = '已接受该联系人，双方现在可以互发消息。';
  } catch (error) { contactNotice = displayError(error); }
  await refreshContacts();
}

async function declineContact(peer: string): Promise<void> {
  if (!window.confirm('删除这条待确认的联系人请求？对方之后仍可再次添加你。')) return;
  try {
    await invoke('v1_decline_contact', { fingerprint: peer });
    contactNotice = '已移除待确认请求。';
  } catch (error) { contactNotice = displayError(error); }
  await refreshContacts();
}

async function refreshContacts(): Promise<void> {
  if (!isTauri()) {
    const status = app.querySelector<HTMLElement>('#contacts-status');
    if (status) status.textContent = '仅桌面应用支持联系人';
    return;
  }
  try {
    const [list, groupList, invites, groupSettings, left] = await Promise.all([
      invoke<Contact[]>('v1_list_contacts'),
      invoke<Group[]>('v1_list_groups'),
      invoke<GroupInvitation[]>('v1_group_invitations'),
      invoke<{ auto_join: boolean }>('v1_group_settings'),
      invoke<Group[]>('v1_list_left_groups'),
    ]);
    contactsCache = list;
    groups = groupList;
    leftGroups = left;
    groupInvites = invites;
    autoJoinGroups = groupSettings.auto_join;
    const status = app.querySelector<HTMLElement>('#contacts-status');
    if (status) {
      const verified = contactsCache.filter((contact) => contact.trust_state === 'verified').length;
      const query = contactQuery.trim();
      const incoming = contactsCache.length - verified;
      status.textContent = contactNotice || (contactsCache.length
        ? `${verified} 位已添加联系人${incoming > 0 ? ` · ${incoming} 条待确认请求` : ''}`
        : '暂无联系人，可前往「局域网用户」添加。');
      contactNotice = '';
    }
    paintContactsSidebar();
    paintContactCards();
    paintGroups(contactsCache);
    paintGroupInvitations();
    paintLeftGroups();
    paintGroupSearch();
    void refreshPresence();
    await refreshJoinRequests();
  } catch {
    const status = app.querySelector<HTMLElement>('#contacts-status');
    if (status) status.textContent = '无法读取联系人';
  }
}

async function refreshGroupInvites(): Promise<void> {
  if (!isTauri()) return;
  try {
    const invites = await invoke<GroupInvitation[]>('v1_group_invitations');
    if (JSON.stringify(invites) !== JSON.stringify(groupInvites)) {
      groupInvites = invites;
      paintGroupInvitations();
    }
  } catch { /* keep the last list on a transient read failure */ }
}

function paintLeftGroups(): void {
  const list = app.querySelector<HTMLUListElement>('#left-groups-list');
  if (!list) return;
  list.replaceChildren();
  if (!leftGroups.length) {
    const empty = document.createElement('li');
    empty.textContent = '没有已退出的群聊。';
    list.append(empty);
    return;
  }
  for (const group of leftGroups) {
    const item = document.createElement('li');
    const info = document.createElement('span');
    info.textContent = `${group.name} · ${group.member_count} 人 · 已退出`;
    const button = document.createElement('button');
    button.className = 'outline';
    button.textContent = '查看历史';
    button.dataset.action = 'open-left-group';
    button.dataset.group = group.id;
    button.dataset.name = group.name;
    item.append(info, button);
    list.append(item);
  }
}

function paintGroupInvitations(): void {
  const host = app.querySelector<HTMLElement>('#group-invitations');
  if (!host) return;
  host.replaceChildren();
  const toggle = app.querySelector<HTMLInputElement>('#auto-join-groups');
  if (toggle) toggle.checked = autoJoinGroups;
  if (!groupInvites.length) {
    const empty = document.createElement('p');
    empty.className = 'hint';
    empty.textContent = '没有待处理的邀请。';
    host.append(empty);
    return;
  }
  for (const invite of groupInvites) {
    const row = document.createElement('div');
    row.className = 'group-invite';
    const text = document.createElement('div');
    const title = document.createElement('strong');
    title.textContent = invite.name;
    const meta = document.createElement('small');
    meta.textContent = `${invite.owner_name || '群主'} 邀请你加入 · ${invite.member_count} 人`;
    text.append(title, meta);
    const accept = document.createElement('button');
    accept.className = 'outline';
    accept.textContent = '同意';
    accept.dataset.action = 'accept-invite';
    accept.dataset.id = invite.id;
    const decline = document.createElement('button');
    decline.className = 'linkish';
    decline.textContent = '拒绝';
    decline.dataset.action = 'decline-invite';
    decline.dataset.id = invite.id;
    const actions = document.createElement('div');
    actions.className = 'group-invite-actions';
    actions.append(accept, decline);
    row.append(text, actions);
    host.append(row);
  }
  const notice = document.createElement('p');
  notice.className = 'hint';
  notice.textContent = invitesNotice;
  host.append(notice);
}

function paintGroups(contacts: Contact[]): void {
  const list = app.querySelector<HTMLUListElement>('#groups-list');
  const pick = app.querySelector<HTMLElement>('#group-members-pick');
  if (list) {
    list.innerHTML = groups.length
      ? groups.map(groupCardRow).join('')
      : '<li class="contact-card-empty">还没有群聊，先在下方创建一个。</li>';
    for (const item of list.querySelectorAll<HTMLElement>('[data-group-drop]')) {
      const groupId = item.dataset.groupDrop ?? '';
      item.addEventListener('dragover', (event) => { event.preventDefault(); item.classList.add('drop-target'); });
      item.addEventListener('dragleave', () => item.classList.remove('drop-target'));
      item.addEventListener('drop', (event) => {
        event.preventDefault(); item.classList.remove('drop-target');
        const fingerprint = event.dataTransfer?.getData('text/plain') ?? '';
        if (fingerprint) void addDroppedMember(groupId, fingerprint);
      });
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

/// Drag-and-drop invite from the contact list; the backend still enforces owner-only.
async function addDroppedMember(groupId: string, fingerprint: string): Promise<void> {
  if (!isTauri()) return;
  const status = app.querySelector<HTMLElement>('#group-create-status');
  try {
    await invoke('v1_add_group_member', { groupId, member: fingerprint });
    if (status) status.textContent = '已邀请该联系人入群，正在通知成员。';
    await refreshContacts();
  } catch (error) { if (status) status.textContent = displayError(error); }
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
  if (lockState.locked) { workspace.suspend(); app.innerHTML = lockView(); return; }
  workspace.resume();
  window.requestAnimationFrame(() => workspace.refreshLayout());
  if (!onboardingLoaded) { app.innerHTML = '<div class="boot">正在准备 Qbird…</div>'; return; }
  if (isTauri() && onboarding && !onboarding.done) { paintOnboarding(); return; }
  const current = pages[page];
  app.innerHTML = `
    <div class="shell">
      <aside class="rail" aria-label="主导航">
        <button class="profile" data-page="profile" aria-label="我的资料" title="我的资料">${avatarMarkup()}</button>
        <nav class="rail-nav" aria-label="主要功能">
          <button data-page="messages" class="${page === 'messages' ? 'active' : ''}" aria-label="消息" title="消息">${icon('messages')}</button>
          <button data-page="contacts" class="${page === 'contacts' ? 'active' : ''}" aria-label="联系人" title="联系人">${icon('contacts')}</button>
          <button data-page="discover" class="${page === 'discover' ? 'active' : ''}" aria-label="局域网用户" title="局域网用户">${icon('discover')}</button>
        </nav>
        <span class="rail-brand">Qbird</span>
        <button class="rail-bottom" data-action="lock-now" aria-label="临时离开锁定" title="临时离开锁定">${icon('lock')}</button>
        <button class="rail-bottom" data-action="menu" aria-label="更多" aria-expanded="${menuOpen}" title="更多">${icon('menu')}</button>
      </aside>
      ${menuOpen ? '<div class="menu" role="menu"><button data-page="settings" role="menuitem">设置</button><button data-page="settings" data-settings-tab="privacy" role="menuitem">聊天备份</button><span>用户备份 · 待实现（将含身份私钥）</span><span>用户还原 · 待实现</span><button data-page="settings" data-settings-tab="privacy" role="menuitem">聊天记录还原</button><button data-page="about" role="menuitem">关于</button></div>' : ''}
      <aside class="sidebar">
        <div class="team"><span class="team-mark">${teamLogoUrl ? `<img class="team-logo" src="${teamLogoUrl}" alt="团队 Logo">` : icon('spark')}</span><div><strong>${team.name ? escapeText(team.name) : 'Qbird 青鸟'}</strong><small>LOCAL NETWORK MESSENGER</small></div></div>
        <div class="side-heading"><div><span class="eyebrow">${current.eyebrow}</span><h1>${current.title}</h1></div></div>
        ${page === 'settings' || page === 'about' ? `<div class="settings-links">${SETTINGS_TABS.map((tab) => `<button data-settings-tab="${tab.id}" class="${page === 'settings' && settingsTab === tab.id ? 'selected' : ''}">${tab.label}</button>`).join('')}<button data-page="about" class="${page === 'about' ? 'selected' : ''}">关于</button></div>` : page === 'messages' ? `<div class="side-search"><input id="global-search" type="search" maxlength="200" placeholder="搜索全部消息" aria-label="搜索全部消息" value="${searchQuery.replace(/&/g, '&amp;').replace(/"/g, '&quot;').replace(/</g, '&lt;')}"></div><div id="search-results" class="search-hits" hidden></div><div id="conversation-list" class="conversation-list">正在读取…</div>` : page === 'contacts' ? contactSidebarView() : '<div class="side-empty">暂无内容</div>'}
        <div class="side-foot"><span class="dot"></span>开发中 · 本地桌面版</div>
      </aside>
      <main class="workspace">
        ${page === 'messages' && groupChat.groupId ? groupChat.view() : page === 'messages' && direct.peer ? direct.view() : page === 'messages' && selfOpen ? selfChatView() : page === 'discover' ? discoveryView() : page === 'settings' ? settingsView() : page === 'profile' ? profileView() : page === 'contacts' ? contactsPageView() : page === 'about' ? `<section class="card about"><span class="eyebrow">ABOUT / 关于</span><h2>Qbird 青鸟</h2><p>面向局域网的本地优先聊天与文件交流工具。</p><dl><div><dt>当前阶段</dt><dd>Windows 开发版 ${appVersion}</dd></div><div><dt>许可证</dt><dd>Apache License 2.0</dd></div><div><dt>平台规划</dt><dd>Windows 首发 · 后续 Linux / macOS</dd></div></dl><p class="notice">开发版未经安全审计，本地数据库未加密。</p></section>` : `<section class="empty"><div class="empty-mark">${icon('spark')}</div><h2>${current.heading}</h2><p>${current.copy}</p></section>`}
      </main>
    </div>`;
  if (page === 'discover') { paintLan(); void refreshLanPeople(); }
  if (page === 'settings') void refreshStorage();
  if (page === 'settings' && settingsTab === 'debug') void refreshDebugReports();
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
  if (page === 'discover') paintLan();
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
  if (!lockState.locked) void restoreWorkspace();
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

function contactTooltip(): HTMLElement {
  let tip = document.getElementById('contact-tip');
  if (!tip) {
    tip = document.createElement('div');
    tip.id = 'contact-tip'; tip.className = 'contact-tip'; tip.hidden = true;
    document.body.append(tip);
  }
  return tip;
}

/// Shows the full contact details on hover; the fingerprint is never truncated here.
function showContactTip(row: HTMLElement): void {
  const tip = contactTooltip();
  const fingerprint = row.dataset.fingerprint ?? '';
  const name = row.dataset.name ?? '';
  const remark = row.querySelector<HTMLInputElement>('input[data-remark-for]')?.value.trim() ?? '';
  const category = row.querySelector<HTMLInputElement>('input[data-category-for]')?.value.trim() ?? '';
  const trust = row.dataset.trust === 'verified' ? '已确认' : '待确认';
  tip.replaceChildren();
  const head = document.createElement('strong'); head.textContent = remark ? `${remark}（${name}）` : name;
  const meta = document.createElement('span'); meta.textContent = `${trust}${category ? ` · ${category}` : ''}`;
  const code = document.createElement('code'); code.textContent = fingerprint;
  tip.append(head, meta, code);
  const box = row.getBoundingClientRect();
  tip.style.left = `${Math.min(box.left, window.innerWidth - 380)}px`;
  tip.style.top = `${Math.min(box.bottom + 6, window.innerHeight - 120)}px`;
  tip.hidden = false;
  if (!fingerprint) return;
  if (hover.show_ip || hover.show_mac) {
    void invoke<{ address: string | null; ip: string | null; mac: string | null }>('v1_contact_network', { peer: fingerprint }).then((info) => {
      if (tip.hidden || tip.dataset.fingerprint !== fingerprint) return;
      if (hover.show_ip) {
        const line = document.createElement('span');
        line.textContent = `IP：${info.ip ?? '未记录地址'}`;
        tip.append(line);
      }
      if (hover.show_mac) {
        const mac = document.createElement('span');
        mac.textContent = `MAC：${info.mac ?? '未获取（跨网段或本机 ARP 无记录）'}`;
        tip.append(mac);
      }
    }).catch(() => {});
  }
  tip.dataset.fingerprint = fingerprint;
}

function escapeText(value: string): string {
  return value.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
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
  probeBusy = true; probeMessage = ''; paintLan();
  try {
    if (start) await invoke('v1_start_chat_service', { port });
    else await invoke('v1_stop_probe_service');
    await refreshProbeStatus();
  } catch (error) { probeMessage = displayError(error); }
  finally { probeBusy = false; if (page === 'discover') paintLan(); }
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
  if (target.id === 'lan-search') { lanQuery = target.value; paintLanPeopleList(app.querySelector<HTMLUListElement>('#lan-list')); return; }
  if (target.id === 'ob-nickname') { onboardingDraft.nickname = target.value; paintOnboardingAvatar(); return; }
  if (target.id === 'ob-username') { onboardingDraft.username = target.value.trim(); paintUsernameState(); return; }
  if (target.id === 'ob-email') { onboardingDraft.email = target.value; return; }
  if (target.id === 'global-search') { searchQuery = target.value; scheduleSearch(); return; }
});
app.addEventListener('keydown', (event) => {
  const target = event.target;
  if (!(target instanceof HTMLTextAreaElement) || !isSendKey(event)) return;
  if (target.id === 'direct-input') { event.preventDefault(); void direct.send(); return; }
  if (target.id === 'group-input') { event.preventDefault(); void groupChat.send(); return; }
  if (target.id === 'self-input') { event.preventDefault(); void sendSelf(); }
});
async function respondToInvite(id: string, accept: boolean): Promise<void> {
  if (!isTauri()) return;
  invitesNotice = accept ? '正在加入…' : '正在拒绝…';
  paintGroupInvitations();
  try {
    if (accept) {
      const groupId = await invoke<string | null>('v1_accept_group_invitation', { id });
      invitesNotice = groupId ? '已加入群聊。' : '邀请已失效或群身份不一致。';
      if (groupId) { await refreshContacts(); await groupChat.open(groupId, groupInvites.find((item) => item.id === id)?.name || '群聊'); }
    } else {
      await invoke<boolean>('v1_decline_group_invitation', { id });
      invitesNotice = '已拒绝邀请，群主会收到退出通知。';
    }
    if (accept) await refreshContacts();
  } catch (error) {
    invitesNotice = displayError(error);
  }
  paintGroupInvitations();
}

app.addEventListener('change', (event) => {
  const target = event.target;
  if (target instanceof HTMLInputElement && target.id === 'auto-accept-contacts') {
    void (async () => {
      try {
        const result = await invoke<{ auto_accept: boolean }>('v1_set_contact_sync', { autoAccept: target.checked });
        autoAcceptContacts = result.auto_accept;
        contactSyncNotice = autoAcceptContacts ? '已开启：同事添加你时自动互相加入。' : '已关闭：对方添加你时不会自动加入联系人。';
      } catch {
        target.checked = !target.checked;
        contactSyncNotice = '无法保存联系人设置';
      }
      const notice = app.querySelector<HTMLElement>('#contact-sync-notice');
      if (notice) notice.textContent = contactSyncNotice;
    })();
    return;
  }
  if (target instanceof HTMLInputElement && target.id === 'auto-join-groups') {
    void (async () => {
      try {
        const result = await invoke<{ auto_join: boolean }>('v1_set_group_auto_join', { autoJoin: target.checked });
        autoJoinGroups = result.auto_join;
        invitesNotice = autoJoinGroups ? '之后被拉进群会自动加入。' : '之后被拉进群会先询问。';
      } catch {
        target.checked = !target.checked;
        invitesNotice = '无法保存群设置';
      }
      paintGroupInvitations();
    })();
    return;
  }
  if (target instanceof HTMLInputElement && target.dataset.pluginToggle) {
    void togglePlugin(target.dataset.pluginToggle, target.checked);
    return;
  }
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
    void direct.open(action.dataset.peer, action.dataset.name || action.dataset.peer)
      .then(() => { paintPresenceHeader(); void refreshPresence(); })
      .catch(error => {
        const status = app.querySelector<HTMLElement>('#contacts-status'); if (status) status.textContent = displayError(error);
      });
    return;
  }
  if (action?.dataset.action === 'hide-direct') { void direct.hide(); return; }
  if (action?.dataset.action === 'contact-tab' && action.dataset.tab) {
    contactTab = action.dataset.tab === 'group' ? 'group' : 'direct';
    contactQuery = '';
    render();
    void refreshContacts();
    return;
  }
  if (action?.dataset.action === 'contacts-add') {
    page = 'discover';
    menuOpen = false;
    render();
    void refreshDiscovery();
    void refreshProbeStatus();
    return;
  }
  if (action?.dataset.action === 'open-url' && action.dataset.url) {
    event.preventDefault();
    workspace.openUrl(action.dataset.url);
    return;
  }
  if (action?.dataset.action === 'direct-send') { void direct.send(); return; }
  if (action?.dataset.action === 'direct-unstage') { direct.unstage(Number(action.dataset.index ?? '-1')); return; }
  if (action?.dataset.action === 'accept-contact' && action.dataset.peer) { void acceptContact(action.dataset.peer); return; }
  if (action?.dataset.action === 'decline-contact' && action.dataset.peer) { void declineContact(action.dataset.peer); return; }
  if (action?.dataset.action === 'group-unstage') { groupChat.unstage(Number(action.dataset.index ?? '-1')); return; }
  if (action?.dataset.action === 'direct-image') { void direct.pick('image'); return; }
  if (action?.dataset.action === 'direct-file') { void direct.pick('file'); return; }
  if (action?.dataset.action === 'direct-older') { void direct.load(true); return; }
  if (action?.dataset.action === 'direct-preview' && action.dataset.id) { direct.openPreviewById(action.dataset.id); return; }
  if (action?.dataset.action === 'direct-reveal' && action.dataset.id) { void direct.reveal(action.dataset.id); return; }
  if (action?.dataset.action === 'save-storage-quota') { void saveQuota(); return; }
  if (action?.dataset.action === 'debug-scan' || action?.dataset.action === 'debug-space') { void refreshDebugReports(); return; }
  if (action?.dataset.action === 'debug-repair' && action.dataset.mode) {
    const mode = action.dataset.mode;
    if (mode === 'restart' || mode === 'rediscover' || mode === 'all') void runNetworkRepair(mode);
    return;
  }
  if (action?.dataset.action === 'backup-export') { void exportBackup(); return; }
  if (action?.dataset.action === 'backup-choose') { void chooseBackup(); return; }
  if (action?.dataset.action === 'backup-inspect') { void inspectBackup(); return; }
  if (action?.dataset.action === 'backup-restore-merge') { void restoreBackup(false); return; }
  if (action?.dataset.action === 'backup-restore-overwrite') { void restoreBackup(true); return; }
  if (action?.dataset.action === 'save-profile') { void saveProfile(); return; }
  if (action?.dataset.action === 'save-composer') { void saveComposer(); return; }
  if (action?.dataset.action === 'save-listen-port') { void saveListenPort(); return; }
  if (action?.dataset.action === 'save-close-action') { void saveCloseAction(); return; }
  if (action?.dataset.action === 'scan-start') { void startScan(); return; }
  if (action?.dataset.action === 'save-scan-blacklist') { void saveScanBlacklist(); return; }
  if (action?.dataset.action === 'probe-scanned' && action.dataset.address) { void probePeer(action.dataset.address); return; }
  if (action?.dataset.action === 'ob-next') {
    syncOnboardingDraft();
    if (onboardingStep === 1) {
      if (!onboardingDraft.nickname.trim()) { onboardingNotice = '请先填写昵称'; paintOnboarding(); return; }
      if (!onboardingDraft.username) onboardingDraft.username = suggestUsername();
      if (!USERNAME_PATTERN.test(onboardingDraft.username)) { onboardingNotice = `用户名须为${USERNAME_HINT}`; paintOnboarding(); return; }
    }
    onboardingStep = Math.min(3, Number(action.dataset.step) || 2);
    onboardingNotice = '';
    paintOnboarding();
    if (onboardingStep === 3) void refreshLanPeople(true);
    return;
  }
  if (action?.dataset.action === 'ob-back') { onboardingStep = Math.max(1, onboardingStep - 1); paintOnboarding(); return; }
  if (action?.dataset.action === 'ob-pick-dir') { void pickOnboardingDir(); return; }
  if (action?.dataset.action === 'ob-reset-dir') { onboardingDraft.recvDir = ''; paintOnboarding(); return; }
  if (action?.dataset.action === 'ob-finish') { void submitOnboarding(); return; }
  if (action?.dataset.action === 'lan-refresh') { void refreshLanPeople(true); return; }
  if (action?.dataset.action === 'lan-start') { void toggleLanListener(true); return; }
  if (action?.dataset.action === 'lan-stop') { void toggleLanListener(false); return; }
  if (action?.dataset.action === 'lan-add') { void addLanPerson(action as HTMLButtonElement); return; }
  if (action?.dataset.action === 'lan-manual-add') { void addManualLan(); return; }
  if (action?.dataset.action === 'lan-identity') { page = 'profile'; render(); return; }
  if (action?.dataset.action === 'open-left-group' && action.dataset.group) {
    const groupId = action.dataset.group, name = action.dataset.name || '群聊';
    direct.clear(); selfOpen = false; page = 'messages';
    void groupChat.open(groupId, name, true).catch((error) => {
      page = 'contacts';
      render();
      const status = app.querySelector<HTMLElement>('#contacts-status');
      if (status) status.textContent = displayError(error);
    });
    return;
  }
  if (action?.dataset.action === 'accept-invite' && action.dataset.id) {
    void respondToInvite(action.dataset.id, true);
    return;
  }
  if (action?.dataset.action === 'decline-invite' && action.dataset.id) {
    void respondToInvite(action.dataset.id, false);
    return;
  }
  if (action?.dataset.action === 'toggle-emoji') {
    const panel = action.closest('.chat-compose')?.querySelector<HTMLElement>('.emoji-panel');
    if (panel) {
      const open = panel.hidden;
      if (open) void loadStickers(panel);
      panel.hidden = !open;
      action.setAttribute('aria-expanded', String(open));
    }
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
  if (action?.dataset.action === 'save-file-accept') { void saveFileAccept(); return; }
  if (action?.dataset.action === 'forget-file-sources') { void forgetFileSources(); return; }
  if (action?.dataset.action === 'choose-recv-direct') { void chooseRecvDir('direct'); return; }
  if (action?.dataset.action === 'choose-recv-group') { void chooseRecvDir('group'); return; }
  if (action?.dataset.action === 'reset-recv-direct') { void resetRecvDir('direct'); return; }
  if (action?.dataset.action === 'reset-recv-group') { void resetRecvDir('group'); return; }
  if (action?.dataset.action === 'accept-file-offer' && action.dataset.id) { void direct.resolveOffer(action.dataset.id, true); return; }
  if (action?.dataset.action === 'reject-file-offer' && action.dataset.id) { void direct.resolveOffer(action.dataset.id, false); return; }
  if (action?.dataset.action === 'accept-file-offer-remember' && action.dataset.id) { void direct.resolveOffer(action.dataset.id, true, { remember: true }); return; }
  if (action?.dataset.action === 'saveas-file-offer' && action.dataset.id) { void direct.saveOfferAs(action.dataset.id); return; }
  if (action?.dataset.action === 'accept-group-file-offer' && action.dataset.id) { void groupChat.resolveOffer(action.dataset.id, true); return; }
  if (action?.dataset.action === 'accept-group-file-offer-remember' && action.dataset.id) { void groupChat.resolveOffer(action.dataset.id, true, { remember: true }); return; }
  if (action?.dataset.action === 'saveas-group-file-offer' && action.dataset.id) { void groupChat.saveOfferAs(action.dataset.id); return; }
  if (action?.dataset.action === 'reject-group-file-offer' && action.dataset.id) { void groupChat.resolveOffer(action.dataset.id, false); return; }
  if (action?.dataset.action === 'group-older') { void groupChat.load(true); return; }
  if (action?.dataset.action === 'hide-group') { void groupChat.hide(); return; }
  if (action?.dataset.action === 'leave-group') { void groupChat.leave(); return; }
  if (action?.dataset.action === 'toggle-group-members') { groupChat.toggleMembers(); return; }
  if (action?.dataset.action === 'add-group-member') { void groupChat.addMember(); return; }
  if (action?.dataset.action === 'remove-group-member' && action.dataset.member) { void groupChat.removeMember(action.dataset.member); return; }
  if (action?.dataset.action === 'create-group') { void createGroup(); return; }
  if (action?.dataset.action === 'install-plugin') { void installPlugin(); return; }
  if (action?.dataset.action === 'remove-plugin' && action.dataset.id) { void removePlugin(action.dataset.id); return; }
  if (action?.dataset.action === 'save-hover-settings') { void saveHover(); return; }
  if (action?.dataset.action === 'save-image-open-mode') { void saveImageOpenMode(); return; }
  if (action?.dataset.action === 'save-file-open-mode') { void saveFileOpenMode(); return; }
  if (action?.dataset.action === 'group-preview-file' && action.dataset.id) { groupChat.openPreviewById(action.dataset.id); return; }
  if (action?.dataset.action === 'export-chat-archive') { void exportChatArchive(); return; }
  if (action?.dataset.action === 'import-chat-archive') { void importChatArchive(); return; }
  if (action?.dataset.action === 'save-team-name') { void saveTeamName(); return; }
  if (action?.dataset.action === 'choose-team-logo') { void chooseTeamLogo(); return; }
  if (action?.dataset.action === 'clear-team-logo') { void clearTeamLogo(); return; }
  if (action?.dataset.action === 'choose-group-avatar') { void chooseGroupAvatar(); return; }
  if (action?.dataset.action === 'clear-group-avatar') { void clearGroupAvatar(); return; }
  if (action?.dataset.action === 'search-groups') { void searchGroups(); return; }
  if (action?.dataset.action === 'join-group' && action.dataset.group) {
    void requestJoin(action.dataset.group, action.dataset.name ?? '', action.dataset.owner ?? ''); return;
  }
  if (action?.dataset.action === 'accept-join' && action.dataset.id) { void resolveJoin(action.dataset.id, true); return; }
  if (action?.dataset.action === 'reject-join' && action.dataset.id) { void resolveJoin(action.dataset.id, false); return; }
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
    void refreshWindowSettings().then(() => { if (page === 'settings' && !lockState.locked) render(); });
    return;
  }
  const nav = target.closest<HTMLButtonElement>('button[data-page]');
  if (nav?.dataset.page && nav.dataset.page in pages) {
    page = nav.dataset.page as Page;
    menuOpen = false;
    render();
    if (page === 'discover') { void refreshDiscovery(); void refreshProbeStatus(); }
    if (page === 'settings') { void refreshStorage(); void refreshWindowSettings().then(() => { if (page === 'settings' && !lockState.locked) render(); }); }
    if (page === 'contacts') void refreshContacts();
  }
});
render();
void refreshOnboarding();
void initLock().then(() => { if (!lockState.locked) void restoreWorkspace(); });
void refreshAvatar().then(() => { if (!lockState.locked) render(); });
void refreshOwnNickname();
void refreshComposer().then(() => { if (!lockState.locked) render(); });
void refreshNetwork().then(() => { if (!lockState.locked) render(); });
void refreshScanState().then(() => { if (!lockState.locked) render(); });
void refreshExperiments().then(() => { if (!lockState.locked) render(); });
void refreshFileAccept().then(() => { if (!lockState.locked) render(); });
void refreshTeam().then(() => { if (!lockState.locked) render(); });
void refreshImageOpenMode().then(() => { if (!lockState.locked) render(); });
void refreshFileOpenMode().then(() => { if (!lockState.locked) render(); });
void refreshHover().then(() => { if (!lockState.locked) render(); });
void refreshWindowSettings().then(() => { if (!lockState.locked) render(); });
void refreshPlugins().then(() => { if (!lockState.locked) render(); });
for (const type of ['pointerdown', 'keydown', 'wheel', 'mousemove'] as const) {
  window.addEventListener(type, () => { lastActivity = Date.now(); }, { passive: true });
}
// Drag a contact row onto a group to invite it; hovering shows the full details.
document.addEventListener('dragstart', (event) => {
  const row = (event.target as HTMLElement | null)?.closest<HTMLElement>('.contact-row');
  if (!row?.dataset.fingerprint || !event.dataTransfer) return;
  event.dataTransfer.setData('text/plain', row.dataset.fingerprint);
  event.dataTransfer.effectAllowed = 'link';
});
document.addEventListener('mouseover', (event) => {
  const row = (event.target as HTMLElement | null)?.closest<HTMLElement>('.contact-row');
  if (row) showContactTip(row);
});
document.addEventListener('mouseout', (event) => {
  const row = (event.target as HTMLElement | null)?.closest<HTMLElement>('.contact-row');
  if (!row) return;
  const next = event.relatedTarget as HTMLElement | null;
  if (next && row.contains(next)) return;
  contactTooltip().hidden = true;
});
window.setInterval(() => { void checkIdleLock(); }, 15_000);
app.addEventListener('conversations-changed', () => { void refreshConversations(); });
window.setInterval(() => {
  if (page === 'contacts' || page === 'messages' || direct.peer) void refreshPresence();
}, 10_000);
window.setInterval(() => {
  if (page === 'discover') void refreshLanPeople();
  void refreshProbeStatus();
  if (page === 'messages') { void refreshConversations(); if (direct.peer) void direct.load(); if (groupChat.groupId) void groupChat.load(); }
  if (page === 'contacts') { void refreshGroupInvites(); void refreshContacts(); }
}, 4000);
if (isTauri()) {
  document.addEventListener('paste', (event) => { void handleImagePaste(event); });
  // A cropped capture lands in the composer staging area, never straight in the chat.
  void listen<string>('qbird://capture-done', async (event) => {
    const path = event.payload;
    if (direct.peer) await direct.stagePaths([path]);
    else if (groupChat.groupId) await groupChat.stagePaths([path]);
  }).catch(() => undefined);
  // The close button is intercepted in Rust; this prompt asks what to do.
  void listen('qbird://close-requested', () => {
    showClosePrompt(() => { void refreshWindowSettings().then(() => { if (page === 'settings') render(); }); });
  }).catch(() => undefined);
  // Dragging files onto the window provides real local paths, so no extra permission is needed.
  void getCurrentWebview().onDragDropEvent((event) => {
    if (event.payload.type !== 'drop') return;
    const paths = event.payload.paths.filter(path => path.length <= 4096);
    if (!paths.length) return;
    if (direct.peer) void direct.stagePaths(paths);
    else if (groupChat.groupId) void groupChat.stagePaths(paths);
  }).catch(() => undefined);
  invoke<string>('identity_fingerprint').then((value) => {
    fingerprint = /^[0-9a-f]{64}$/.test(value) ? value : '无法读取';
    if (page === 'settings') render(); else if (page === 'discover') paintDiscovery();
  }).catch(() => {
    fingerprint = '无法读取';
    if (page === 'settings') render(); else if (page === 'discover') paintDiscovery();
  });
}
