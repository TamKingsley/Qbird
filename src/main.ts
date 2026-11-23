import './style.css';
import { formatStamp, loadChatDisplay, setShowSeconds, getShowSeconds } from './chat_prefs';
import { invoke, isTauri } from '@tauri-apps/api/core';
import { open as openFileDialog, save as saveFileDialog } from '@tauri-apps/plugin-dialog';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { listen } from '@tauri-apps/api/event';
import { version as appVersion } from '../package.json';
import { DirectChat } from './direct_chat';
import { GroupChat } from './group_chat';
import { groupedMessage, messageAvatar } from './message_avatar';
import { composerTools, toolIcon } from './ui_icons';
import { showClosePrompt } from './close_prompt';
import { cropAvatarInteractively } from './avatar_crop';
import { workspace } from './workspace';
import { linkifiedText } from './linkify';
import { recallItems, showContextMenu, type ContextItem } from './context_menu';
import { t, trError, getLang, setLang, syncLangToBackend, LANG_LABEL, type Lang } from './i18n';
import { loadUntilFound } from './search_navigation';

type Page = 'messages' | 'contacts' | 'discover' | 'settings' | 'about' | 'profile';
const root = document.querySelector<HTMLDivElement>('#app');
if (!root) throw new Error('Application root is missing');
const app = root;
let page: Page = 'messages';
let menuOpen = false;
let fingerprint = isTauri() ? t('正在读取…') : t('仅桌面应用可读取');
type Candidate = { service_name: string; addresses: string[]; nickname: string | null; username: string | null };
let candidates: Candidate[] = [];
type LanPerson = {
  service_name: string; address: string; fingerprint: string; qbird_id: string;
  nickname: string | null; username: string | null; known: boolean; verified: boolean;
};
let lanPeople: LanPerson[] = [];
let lanQuery = '';
let lanStatus = isTauri() ? t('正在识别同网段用户…') : t('仅桌面应用支持局域网发现');
let lanLoading = false;
let lanAddBusy = '';
let lanNotice = '';
let lanRefreshedAt = 0;
let discoveryStatus = isTauri() ? t('正在查找…') : t('仅桌面应用支持局域网发现');
let discoveryLoading = false;
type ProbeRequest = { fingerprint: string; remote_ip: string };
type ProbeStatus = { running: boolean; port: number | null; requests: ProbeRequest[] };
let probeStatus: ProbeStatus = { running: false, port: null, requests: [] };
let probePort = '42853';
type NetworkSettings = { default_port: number };
let networkSettings: NetworkSettings = { default_port: 42853 };
let networkNotice = '';
type CloseAction = 'ask' | 'tray' | 'quit';
const CLOSE_ACTION_LABEL: Record<CloseAction, () => string> = {
  ask: () => t('close.optAsk'),
  tray: () => t('close.optTray'),
  quit: () => t('close.optQuit'),
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
let probeNotice = isTauri() ? t('探测服务未启动') : t('仅桌面应用支持身份探测');
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
type ContactRequest = { id: string; direction: 'in' | 'out'; peer: string; nickname: string; username: string | null; address: string | null; created_at: number; status: string; reason: string | null };
type DissolvedGroup = { id: string; name: string; owner: string; dissolved_at: number | null; last_body: string; last_logical: number };
let incomingRequests: ContactRequest[] = [];
let outgoingRequests: ContactRequest[] = [];
let dissolvedGroups: DissolvedGroup[] = [];
let requestNotice = '';
let lanVisible = true;
let contactRequestMode: 'ask' | 'auto' = 'ask';
let privacyNotice = '';
type NoticeMode = 'popup' | 'tray' | 'off';
let noticeSettings: { mode: NoticeMode; raise_window: boolean; preview: boolean } = { mode: 'popup', raise_window: false, preview: true };
let noticeNotice = '';
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
const ACCEPT_MODE_LABEL: Record<FileAcceptSettings['mode'], () => string> = {
  always: () => t('pr.acceptAlways'),
  remember: () => t('pr.acceptRemember'),
  auto: () => t('pr.acceptAuto'),
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
type Conversation = { id: string; last_body: string; last_logical: number; kind: string; unread: number; muted: boolean };
type Group = { id: string; name: string; owner: string; member_count: number; hidden: boolean; is_owner: boolean; last_body: string; last_logical: number };
let groups: Group[] = [];
/// Fixtures so the message list can be reviewed in a browser without the desktop shell.
const demoContacts: Contact[] = [
  { fingerprint: 'a'.repeat(64), display_name: t('林小满'), trust_state: 'verified', username: 'xiaoman', remark: null, category: null },
  { fingerprint: 'c'.repeat(64), display_name: t('周隐'), trust_state: 'verified', username: 'zhouyin', remark: null, category: null },
];
const demoGroupList: Group[] = [
  { id: 'd'.repeat(32), name: t('项目组'), owner: 'a'.repeat(64), member_count: 4, hidden: false, is_owner: false, last_body: t('周隐：明天十点评审'), last_logical: 30 },
];
const demoConversationList: Conversation[] = [
  { id: 'self', last_body: t('记一下：周三交周报'), last_logical: 40, kind: 'self', unread: 0, muted: true },
  { id: 'a'.repeat(64), last_body: t('晚点把接口文档发你'), last_logical: 35, kind: 'direct', unread: 3, muted: false },
  { id: 'd'.repeat(32), last_body: t('周隐：明天十点评审'), last_logical: 30, kind: 'group', unread: 12, muted: true },
];
type LocalMessage = { id: string; sender: string; body: string; logical: number; created_at: number; recalled_at: number | null };
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
type RestoreReport = { mode: string; inserted: number; skipped: number; conflicts: number; snapshot_path: string | null; restored_attachments: number };
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
let searchScopeKey = 'all';
let searchOffset = 0;
let searchHasMore = false;
const searchPageSize = 100;
type LockStatus = { has_password: boolean; locked: boolean; idle_minutes: number };
let lockState: LockStatus = { has_password: false, locked: false, idle_minutes: 0 };
let lockNotice = '';
let lockBusy = false;
let lockInitialized = false;
let lastActivity = Date.now();
const direct = new DirectChat(app, () => { groupChat.clear(); selfOpen = false; page = 'messages'; menuOpen = false; render(); });
const groupChat = new GroupChat(app, () => { direct.clear(); selfOpen = false; page = 'messages'; menuOpen = false; render(); }, () => myNickname);

// The workspace stores only serializable sources; chat modules resolve them back into
// preview content, so the same path works for live opens and restored tabs.
workspace.setResolver(async (source) => {
  if (source.kind === 'web') {
    return { title: source.url, loaded: { kind: 'error', message: t('网页由 iframe 直接加载') } };
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
  } catch { const status = app.querySelector<HTMLElement>('#contacts-status'); if (status) status.textContent = t('无法打开自聊'); }
}

async function hideSelf(): Promise<void> {
  if (!isTauri() || selfBusy) return;
  try {
    await invoke('v1_hide_self');
    selfOpen = false; render();
  } catch { const error = app.querySelector<HTMLElement>('#self-error'); if (error) error.textContent = t('无法隐藏会话'); }
}

async function sendSelf(): Promise<void> {
  const input = app.querySelector<HTMLTextAreaElement>('#self-input');
  const error = app.querySelector<HTMLElement>('#self-error');
  const button = app.querySelector<HTMLButtonElement>('button[data-action="self-send"]');
  if (!isTauri() || !input || !error || selfBusy) return;
  const body = input.value;
  if (!body.trim()) { error.textContent = t('请输入消息'); return; }
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

function appendUnread(button: HTMLButtonElement, count: number, muted = false): void {
  if (count <= 0) return;
  if (muted) {
    // A muted conversation still counts, it just refuses to shout a number.
    const dot = document.createElement('span');
    dot.className = 'unread-dot';
    dot.title = `${count} 条未读（消息免打扰）`;
    dot.setAttribute('aria-label', `${count} 条未读消息，已开启免打扰`);
    button.append(dot);
    return;
  }
  const badge = document.createElement('span');
  badge.className = 'unread-badge';
  badge.textContent = count > 99 ? '99+' : String(count);
  badge.setAttribute('aria-label', `${count} 条未读消息`);
  button.append(badge);
}

/// Small bell with a slash, used only as a hint that a row will not raise a badge.
function muteMark(): HTMLElement {
  const mark = document.createElement('span');
  mark.className = 'mute-mark';
  mark.title = t('消息免打扰');
  mark.setAttribute('aria-label', t('消息免打扰'));
  mark.innerHTML = '<svg viewBox="0 0 24 24" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="M18.1 14.6a5.6 5.6 0 0 1-1.2-3.4v-1.4a4.9 4.9 0 0 0-3.5-4.7V4.4a1.4 1.4 0 0 0-2.8 0v.7a4.9 4.9 0 0 0-3.5 4.7v1.4c0 1.4-.4 2.4-1.2 3.4-.3.4 0 .9.5.9h3.7m3.2 0h4.3c.5 0 .8-.5.5-.9z"/><path d="M10.3 19.2a1.9 1.9 0 0 0 3.4 0M3.8 3.8l16.4 16.4"/></svg>';
  return mark;
}

/// Right-click menu for one row of the message list; only local state changes here.
function conversationMenu(event: MouseEvent, conversation: Conversation, label: string): void {
  event.preventDefault();
  const items: ContextItem[] = [];
  if (conversation.kind !== 'self') {
    items.push({ label: conversation.unread > 0 ? t('标为已读') : t('标为未读'), run: () => { void setConversationUnread(conversation); } });
  }
  items.push({
    label: conversation.muted ? t('开启消息提醒') : t('消息免打扰'),
    run: () => { void setConversationMuted(conversation); },
  });
  if (conversation.kind !== 'self') {
    items.push({ label: t('查看聊天记录'), run: () => { void openRecordsWindow(conversation.kind as 'direct' | 'group', conversation.id); } });
  }
  items.push({ label: t('不显示该会话'), danger: true, run: () => { void hideConversation(conversation, label); } });
  showContextMenu(event, items);
}

async function setConversationMuted(conversation: Conversation): Promise<void> {
  if (!isTauri()) return;
  try {
    await invoke('v1_set_conversation_muted', { kind: conversation.kind, id: conversation.id, muted: !conversation.muted });
    await refreshConversations();
  } catch (error) { workspace.notify(displayError(error)); }
}

async function setConversationUnread(conversation: Conversation): Promise<void> {
  if (!isTauri()) return;
  try {
    if (conversation.unread > 0) {
      // Re-opening is the existing way to clear the counter without inventing state.
      if (conversation.kind === 'direct') await invoke('v1_open_direct', { peer: conversation.id });
      else await invoke('v1_open_group', { groupId: conversation.id });
    } else {
      await invoke('v1_mark_conversation_unread', { kind: conversation.kind, id: conversation.id });
    }
    await refreshConversations();
  } catch (error) { workspace.notify(displayError(error)); }
}

async function hideConversation(conversation: Conversation, label: string): Promise<void> {
  if (!isTauri()) return;
  try {
    if (conversation.kind === 'self') {
      await invoke('v1_hide_self');
      if (selfOpen) { selfOpen = false; selfMessages = []; }
    } else if (conversation.kind === 'direct') {
      await invoke('v1_hide_direct', { peer: conversation.id });
      if (direct.peer === conversation.id) direct.clear();
    } else {
      await invoke('v1_hide_group', { groupId: conversation.id });
      if (groupChat.groupId === conversation.id) groupChat.clear();
    }
    // render() repaints the list and the open pane in one pass.
    render();
    workspace.notify(`已隐藏「${label}」；收到新消息时会重新出现。`);
  } catch (error) { workspace.notify(displayError(error)); }
}

/// WeChat-style "⋯" menu for the open conversation: chat records, export and hide.
function openChatMenu(button: HTMLElement, event: MouseEvent): void {
  const kind = button.dataset.kind as 'self' | 'direct' | 'group';
  const items: ContextItem[] = [
    { label: t('聊天记录'), run: () => { void openRecordsWindow(kind); } },
    ...(kind === 'group' ? [{ label: t('群公告'), run: () => { groupChat.toggleNoticeEditor(); } }] : []),
    { label: t('不显示此会话'), danger: true, run: () => {
      if (kind === 'direct') void direct.hide();
      else if (kind === 'group') void groupChat.hide();
      else void hideSelf();
    } },
  ];
  showContextMenu(event, items);
}

/// Opens (or focuses) the standalone chat-records window for the open conversation.
async function openRecordsWindow(kind: 'self' | 'direct' | 'group', conversationId?: string): Promise<void> {
  const id = conversationId ?? (kind === 'self' ? '' : kind === 'direct' ? direct.peer : groupChat.groupId);
  if (kind !== 'self' && !id) return;
  if (!isTauri()) { workspace.notify(t('聊天记录窗口需要在桌面应用中打开。')); return; }
  try { await invoke('v1_open_records_window', { kind, id }); }
  catch (error) { workspace.notify(displayError(error)); }
}

async function refreshConversations(): Promise<void> {
  const list = app.querySelector<HTMLElement>('#conversation-list');
  if (!list) return;
  if (!isTauri() && !demoMode) { list.textContent = t('请在桌面应用中查看会话'); return; }
  try {
    const [conversations, contacts, groupList] = demoMode
      ? [demoConversationList, demoContacts, demoGroupList]
      : await Promise.all([
        invoke<Conversation[]>('v1_list_conversations'),
        invoke<Contact[]>('v1_list_contacts'),
        invoke<Group[]>('v1_list_groups'),
      ]);
    groups = groupList;
    if (!list.isConnected) return;
    list.replaceChildren();
    if (!conversations.length) { list.textContent = t('暂无会话'); return; }
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
        if (conversation.muted) button.classList.add('muted');
        button.append(name, preview);
        if (conversation.muted) preview.prepend(muteMark());
        appendUnread(button, conversation.unread, conversation.muted);
        button.addEventListener('contextmenu', (event) => conversationMenu(event, conversation, group.name));
        list.append(button);
        continue;
      }
      const contact = contacts.find(item => item.fingerprint === conversation.id && item.trust_state === 'verified');
      if (conversation.id !== 'self' && !contact) continue;
      const button = document.createElement('button');
      button.className = `conversation-row${conversation.id === 'self' ? (selfOpen ? ' selected' : '') : (direct.peer === conversation.id ? ' selected' : '')}`;
      button.dataset.action = conversation.id === 'self' ? 'open-self' : 'open-direct';
      if (conversation.muted) button.classList.add('muted');
      if (contact) { button.dataset.peer = contact.fingerprint; button.dataset.name = contact.display_name; }
      const name = document.createElement('strong'); name.textContent = contact?.display_name ?? t('自己');
      const preview = document.createElement('span'); preview.textContent = conversation.last_body;
      button.append(name, preview);
      if (conversation.muted) preview.prepend(muteMark());
      appendUnread(button, conversation.unread, conversation.muted);
      const label = contact?.display_name ?? t('自己');
      button.addEventListener('contextmenu', (event) => conversationMenu(event, conversation, label));
      list.append(button);
    }
  } catch { if (list.isConnected) list.textContent = t('无法读取会话'); }
}

function paintSelfMessages(scroll: 'bottom' | 'preserve'): void {
  const history = app.querySelector<HTMLElement>('#self-history');
  const list = app.querySelector<HTMLElement>('#self-messages');
  const older = app.querySelector<HTMLButtonElement>('button[data-action="self-older"]');
  if (!history || !list || !older) return;
  const previousHeight = history.scrollHeight, previousTop = history.scrollTop;
  list.replaceChildren();
  let previous: LocalMessage | null = null;
  for (const message of selfMessages) {
    const row = document.createElement('div'); row.className = 'message-row'; row.dataset.id = message.id;
    if (groupedMessage(previous, message)) row.classList.add('grouped');
    const avatar = messageAvatar('self', myNickname || t('我'));
    const column = document.createElement('div'); column.className = 'message-column';
    const body = document.createElement('div'); body.className = 'message-bubble';
    if (message.recalled_at) {
      body.classList.add('recalled'); body.textContent = t('你撤回了一条消息');
      column.append(body); row.append(avatar, column); list.append(row); previous = message;
      continue;
    }
    body.append(linkifiedText(message.body));
    const time = document.createElement('small'); time.className = 'message-time'; time.textContent = formatStamp(message.created_at);
    body.addEventListener('contextmenu', (event) => {
      const items = recallItems(true, Date.now() / 1000 - message.created_at <= 120, () => { void recallSelfMessage(message); });
      items.push(
        { label: t('复制文字'), run: () => { void navigator.clipboard?.writeText(message.body).catch(() => {}); } },
        { label: t('删除'), danger: true, run: () => { void deleteSelfMessage(message.id); } },
      );
      showContextMenu(event, items);
    });
    column.append(time, body); row.append(avatar, column); list.append(row); previous = message;
  }
  older.hidden = !selfHasOlder;
  history.scrollTop = scroll === 'bottom' ? history.scrollHeight : previousTop + history.scrollHeight - previousHeight;
}

async function recallSelfMessage(message: LocalMessage): Promise<void> {
  try {
    const result = await invoke<string>('v1_recall_message', { kind: 'self', id: message.id, groupId: null });
    if (result === 'recalled' || result === 'already') {
      message.recalled_at = Math.floor(Date.now() / 1000); message.body = '';
      paintSelfMessages('preserve'); void refreshConversations();
    } else workspace.notify(result === 'too_late' ? t('超过 2 分钟，不能再撤回') : t('撤回失败'));
  } catch (error) { workspace.notify(trError(error, t('撤回失败'))); }
}

async function deleteSelfMessage(messageId: string): Promise<void> {
  if (!isTauri()) return;
  try {
    await invoke('v1_delete_messages', { kind: 'self', id: '', ids: [messageId] });
    selfMessages = selfMessages.filter((m) => m.id !== messageId);
    paintSelfMessages('preserve');
  } catch (error) { workspace.notify(displayError(error)); }
}

async function loadSelfMessages(older: boolean): Promise<void> {
  if (!isTauri() || selfBusy) return;
  selfBusy = true;
  try {
    const batch = await invoke<LocalMessage[]>('v1_list_self_messages', { limit: 200, offset: older ? selfMessages.length : 0 });
    selfMessages = older ? [...batch.reverse(), ...selfMessages] : batch.reverse();
    selfHasOlder = batch.length === 200;
    paintSelfMessages(older ? 'preserve' : 'bottom');
  } catch { const error = app.querySelector<HTMLElement>('#self-error'); if (error) error.textContent = t('无法读取自聊记录'); }
  finally { selfBusy = false; }
}

function selfChatView(): string {
  return `<section class="self-chat" aria-label="${escapeText(t('自聊'))}">
    <header class="chat-header"><div><strong>自己</strong><small>仅保存在本机</small></div><div class="group-actions"><button class="outline chat-more" data-action="chat-menu" data-kind="self" aria-label="${escapeText(t('会话菜单'))}" title="${escapeText(t('会话菜单'))}">⋯</button></div></header>
    <div class="chat-history" id="self-history" aria-label="${escapeText(t('自聊记录'))}"><button class="load-older" data-action="self-older" hidden>加载更早消息</button><div id="self-messages"></div></div>
    <div class="chat-compose"><label for="self-input">消息</label><textarea id="self-input" rows="3" maxlength="32768" placeholder="${escapeText(t('写点什么…'))}"></textarea>${composerTools('self', 'Enter 发送 · Shift+Enter 换行', false)}<p id="self-error" role="status"></p></div>
  </section>`;
}

type PlaceholderPage = 'messages' | 'contacts' | 'discover' | 'settings' | 'about' | 'profile';
const pages: Record<PlaceholderPage, { title: () => string; eyebrow: string; heading: () => string; copy: () => string }> = {
  messages: { title: () => t('page.messages.title'), eyebrow: 'MESSAGES', heading: () => t('page.messages.heading'), copy: () => t('page.messages.copy') },
  contacts: { title: () => t('page.contacts.title'), eyebrow: 'CONTACTS', heading: () => t('page.contacts.title'), copy: () => t('page.contacts.copy') },
  discover: { title: () => t('page.discover.title'), eyebrow: 'DISCOVERY', heading: () => t('page.discover.title'), copy: () => t('page.discover.copy') },
  settings: { title: () => t('page.settings.title'), eyebrow: 'PREFERENCES', heading: () => t('page.settings.title'), copy: () => t('page.settings.copy') },
  about: { title: () => t('page.about.title'), eyebrow: 'ABOUT', heading: () => t('page.about.heading'), copy: () => t('page.about.copy') },
  profile: { title: () => t('page.profile.title'), eyebrow: 'PROFILE', heading: () => t('page.profile.heading'), copy: () => t('page.profile.copy') }
};

function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) return t('未知');
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
        <button class="outline" data-action="choose-avatar" ${avatarBusy ? 'disabled' : ''}>${avatarUrl ? t('更换头像…') : t('选择头像…')}</button>
        <button class="outline" data-action="clear-avatar" ${avatarUrl && !avatarBusy ? '' : 'disabled'}>移除头像</button>
        <small>选择图片后可预览、拖动与缩放，确认后保存为正方形（最大 512 像素）；已验证好友和共同群成员可通过加密连接获取头像。</small>
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
      <p>同一局域网的同事添加你时，会生成一条好友申请；你可以接受或拒绝，接受后双方才能互发消息。</p>
      <p><label><input id="auto-accept-contacts" type="checkbox" ${autoAcceptContacts ? 'checked' : ''}> 自动接受好友添加（关闭后需要验证信息并由你确认）</label></p>
      <p id="contact-sync-notice" class="hint" role="status">${contactSyncNotice}</p>
    </div>
    <div class="probe-box"><h3>联系人悬浮信息</h3>
      <p>悬停联系人时显示：${hover.show_ip ? 'IP 地址' : t('不显示 IP')}${hover.show_mac ? '、MAC 地址' : '、不显示 MAC'}。</p>
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
  } catch { profileNotice = t('无法读取个人资料'); }
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
      ? t('资料已保存，并已重新广播给同一网段的同事。')
      : t('资料已保存在本机；当前未监听，下次启动应用时会使用新资料广播。');
  } catch (error) { profileNotice = trError(error, t('无法保存资料')); }
  if (notice) notice.textContent = profileNotice;
}

/// Placeholder shown before any avatar is uploaded: the first character of the
/// nickname (Chinese character or Latin initial), never a fixed brand glyph.
function avatarInitial(name: string): string {
  const first = Array.from(name.trim())[0] ?? '';
  if (!first) return t('青');
  return /^[a-z]$/i.test(first) ? first.toUpperCase() : first;
}

function avatarMarkup(nickname: string = myNickname): string {
  return avatarUrl ? `<img src="${escapeText(avatarUrl)}" alt="${escapeText(t('我的头像'))}">` : escapeText(avatarInitial(nickname));
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
  if (!context) throw new Error(t('无法创建画布'));
  context.drawImage(bitmap, (bitmap.width - side) / 2, (bitmap.height - side) / 2, side, side, 0, 0, edge, edge);
  const blob = await new Promise<Blob | null>((resolve) => canvas.toBlob(resolve, 'image/png'));
  if (!blob) throw new Error(t('无法生成头像'));
  return new Uint8Array(await blob.arrayBuffer());
}

async function chooseAvatar(): Promise<void> {
  if (!isTauri() || avatarBusy) return;
  try {
    const picked = await openFileDialog({ multiple: false, filters: [{ name: t('图片'), extensions: ['png', 'jpg', 'jpeg'] }] });
    if (!picked || typeof picked !== 'string') return;
    avatarBusy = true; avatarNotice = t('正在读取图片…'); if (page === 'profile') render();
    const source = new Uint8Array(await invoke<number[]>('v1_read_image_file', { path: picked }));
    // Nothing is written until the user confirms the crop.
    const cropped = await cropAvatarInteractively(source, { title: t('调整头像') });
    if (!cropped) { avatarNotice = t('已取消，头像未修改。'); return; }
    await invoke('v1_set_avatar', { bytes: Array.from(cropped) });
    setAvatarImage(cropped);
    avatarNotice = t('头像已保存，已验证好友和共同群成员可获取。');
  } catch (error) { avatarNotice = displayError(error); }
  finally { avatarBusy = false; if (page === 'profile') render(); }
}

async function clearAvatar(): Promise<void> {
  if (!isTauri() || avatarBusy) return;
  avatarBusy = true;
  try {
    await invoke('v1_clear_avatar');
    setAvatarImage(null);
    avatarNotice = t('头像已移除。');
  } catch (error) { avatarNotice = displayError(error); }
  finally { avatarBusy = false; if (page === 'profile') render(); }
}

function emojiPanelHtml(): string {
  const grid = COMMON_EMOJIS.map((emoji) => `<button type="button" data-action="pick-emoji" data-emoji="${emoji}">${emoji}</button>`).join('');
  return `<div class="emoji-grid">${grid}</div>
    <div class="sticker-head"><strong>我的表情</strong><button type="button" class="outline" data-action="add-sticker">添加…</button><button type="button" class="outline" data-action="toggle-sticker-manage">${stickerManage ? t('完成') : t('管理')}</button></div>
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
  } catch { emojiNotice = t('无法读取表情收藏'); }
  paintStickers(panel);
}

function paintStickers(panel: HTMLElement): void {
  const list = panel.querySelector<HTMLElement>('.sticker-list');
  const notice = panel.querySelector<HTMLElement>('.emoji-notice');
  if (notice) notice.textContent = emojiNotice;
  if (!list) return;
  if (!stickers.length) { list.textContent = t('还没有收藏的表情，点击“添加…”选择 PNG、GIF、JPEG 或 WebP（≤512 KiB）。'); return; }
  list.innerHTML = stickers.map((name) => `<div class="sticker"><button type="button" data-action="use-sticker" data-name="${escapeText(name)}" title="${escapeText(t('发送此表情'))}"><img src="${stickerUrls.get(name) ?? ''}" alt=""></button>${stickerManage ? `<button type="button" class="sticker-remove" data-action="remove-sticker" data-name="${escapeText(name)}" aria-label="${escapeText(t('移除表情'))}">×</button>` : ''}</div>`).join('');
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
  if (!direct.peer) { emojiNotice = t('请先打开一个单聊，再发送表情。'); paintEmojiNotices(); return; }
  try {
    const path = await invoke<string>('v1_sticker_path', { name });
    await direct.queuePaths([path]);
    emojiNotice = t('表情已加入待发送队列。');
  } catch (error) { emojiNotice = displayError(error); }
  paintEmojiNotices();
}

async function addSticker(): Promise<void> {
  if (!isTauri()) return;
  const picked = await openFileDialog({ multiple: false, filters: [{ name: t('表情图片'), extensions: ['png', 'jpg', 'jpeg', 'gif', 'webp'] }] });
  if (!picked || typeof picked !== 'string') return;
  try {
    const name = await invoke<string>('v1_add_sticker', { path: picked });
    stickersLoaded = true;
    if (!stickers.includes(name)) stickers = [...stickers, name].sort();
    emojiNotice = t('已加入我的表情。');
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
    emojiNotice = t('表情已移除。');
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
  } catch { scanBlacklistNotice = t('无法读取扫描黑名单'); }
}

async function startScan(): Promise<void> {
  if (!isTauri() || scanBusy) return;
  const input = app.querySelector<HTMLInputElement>('#scan-cidr');
  const cidr = (input?.value ?? scanCidr).trim();
  scanCidr = cidr;
  const port = Number(probePort);
  if (!Number.isInteger(port) || port < 1 || port > 65535) { scanNotice = t('请先在发现页设置 1–65535 的监听端口'); render(); return; }
  scanBusy = true; scanNotice = t('正在扫描…'); scanHits = [];
  render();
  try {
    scanHits = await invoke<ScanHit[]>('v1_scan_range', { cidr, port });
    scanNotice = scanHits.length ? `发现 ${scanHits.length} 个候选，请用独立渠道核对完整指纹。` : t('本次未发现候选；端口未开放或对方未启动服务。');
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
    scanBlacklistNotice = t('黑名单已保存。');
  } catch (error) { scanBlacklistNotice = displayError(error); }
  if (page === 'discover') render();
}

async function refreshOnboarding(): Promise<void> {
  if (!isTauri()) {
    onboardingLoaded = true;
    if (demoMode) {
      qbirdId = 'QB-2F91-8C3D';
      onboarding = { done: false, profile: { nickname: 'Alex', email: '', username: 'alex_demo' }, qbird_id: qbirdId };
      onboardingDraft = { nickname: 'Alex', username: 'alex_demo', email: '', recvDir: '' };
      lanPeople = [
        { service_name: 'a', address: '192.168.1.23:42853', fingerprint: 'a'.repeat(64), qbird_id: 'QB-AAAA-1111', nickname: t('林小满'), username: 'xiaoman', known: true, verified: true },
        { service_name: 'b', address: '192.168.1.41:42853', fingerprint: 'b'.repeat(64), qbird_id: 'QB-BBBB-2222', nickname: t('周隐'), username: 'zhouyin', known: false, verified: false },
        { service_name: 'c', address: '192.168.1.77:42853', fingerprint: 'c'.repeat(64), qbird_id: 'QB-CCCC-3333', nickname: t('陈默'), username: 'chenmo', known: false, verified: false },
      ];
      lanStatus = t('发现 3 位用户');
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
  } catch { networkNotice = t('status.portReadErr'); }
}

async function saveListenPort(): Promise<void> {
  if (!isTauri()) return;
  const port = Number(app.querySelector<HTMLInputElement>('#listen-port')?.value ?? '0');
  if (!Number.isInteger(port) || port < 1024 || port > 65535) { networkNotice = t('status.portRange'); if (page === 'settings') render(); return; }
  try {
    networkSettings = await invoke<NetworkSettings>('v1_set_network_settings', { port });
    probePort = String(networkSettings.default_port);
    networkNotice = t('status.portSavedMsg');
  } catch (error) { networkNotice = displayError(error); }
  if (page === 'settings') render();
}

async function refreshWindowSettings(): Promise<void> {
  if (!isTauri()) return;
  try { windowSettings = await invoke<{ close_action: CloseAction }>('v1_window_settings'); }
  catch { closeActionNotice = t('status.closeReadErr2'); }
}

async function saveCloseAction(): Promise<void> {
  if (!isTauri()) return;
  const value = app.querySelector<HTMLSelectElement>('#close-action')?.value;
  if (value !== 'ask' && value !== 'tray' && value !== 'quit') return;
  try {
    windowSettings = await invoke<{ close_action: CloseAction }>('v1_set_window_settings', { closeAction: value });
    closeActionNotice = t('status.closeSaved');
  } catch (error) { closeActionNotice = displayError(error); }
  if (page === 'settings') render();
}

async function refreshComposer(): Promise<void> {
  if (!isTauri()) return;
  try { composer = await invoke<ComposerSettings>('v1_composer_settings'); }
  catch { composerNotice = t('status.composerReadErr'); }
}

async function saveComposer(): Promise<void> {
  if (!isTauri()) return;
  const sendShortcut = app.querySelector<HTMLSelectElement>('#send-shortcut')?.value ?? 'enter';
  const rows = Number(app.querySelector<HTMLInputElement>('#composer-rows')?.value ?? '3');
  if (!Number.isInteger(rows) || rows < 2 || rows > 12) { composerNotice = t('status.rowsRange'); if (page === 'settings') render(); return; }
  try {
    composer = await invoke<ComposerSettings>('v1_set_composer_settings', { sendShortcut, rows });
    composerNotice = t('status.composerSaved');
  } catch (error) { composerNotice = displayError(error); }
  if (page === 'settings') render();
}

type SettingsTab = 'general' | 'privacy' | 'backup' | 'workspace' | 'experimental' | 'debug';
let settingsTab: SettingsTab = 'general';
const SETTINGS_TABS: { id: SettingsTab; label: () => string }[] = [
  { id: 'general', label: () => t('tab.general') },
  { id: 'privacy', label: () => t('tab.privacy') },
  { id: 'backup', label: () => t('tab.backup') },
  { id: 'workspace', label: () => t('tab.workspace') },
  { id: 'experimental', label: () => t('tab.experimental') },
  { id: 'debug', label: () => t('tab.debug') },
];

function settingsView(): string {
  const tab = SETTINGS_TABS.find((item) => item.id === settingsTab) ?? SETTINGS_TABS[0];
  return `<section class="card settings-card">
    <span class="eyebrow">${t('settings.eyebrow')}</span><h2>${tab.label()}</h2>
    ${settingsTab === 'privacy' ? privacySettingsHtml() : settingsTab === 'backup' ? backupSettingsHtml() : settingsTab === 'workspace' ? workspaceSettingsHtml() : settingsTab === 'experimental' ? experimentalSettingsHtml() : settingsTab === 'debug' ? debugSettingsHtml() : generalSettingsHtml()}
  </section>`;
}

function generalSettingsHtml(): string {
  const used = storageInfo ? formatBytes(storageInfo.used) : t('读取中…');
  const quota = storageInfo ? formatBytes(storageInfo.quota) : t('读取中…');
  const quotaMib = storageInfo ? Math.round(storageInfo.quota / (1024 * 1024)) : '';
  const langs = (Object.keys(LANG_LABEL) as Lang[]).map((id) => `<option value="${id}" ${getLang() === id ? 'selected' : ''}>${LANG_LABEL[id]}</option>`).join('');
  return `<p>${t('general.open')}</p>
    <div class="probe-box"><h3>${t('settings.language')}</h3>
      <label for="app-language">${t('settings.language')}</label>
      <div class="probe-row"><select id="app-language">${langs}</select></div>
      <small>${t('settings.languageHint')}</small>
    </div>
    <div class="probe-box"><h3>${t('settings.chatDisplay')}</h3>
      <p><label><input id="chat-show-seconds" type="checkbox" ${getShowSeconds() ? 'checked' : ''}> ${t('settings.showSeconds')}</label></p>
      <small>${t('settings.showSecondsHint')}</small>
    </div>
    <div class="probe-box"><h3>${t('general.storage')}</h3>
      <p>${t('general.storageUsed')} <strong id="storage-used">${used}</strong>，${t('general.quotaLimit')} <strong id="storage-quota">${quota}</strong>。</p>
      <label for="storage-quota-mib">${t('general.quotaLabel')}</label>
      <div class="probe-row"><input id="storage-quota-mib" type="number" min="256" max="65536" value="${quotaMib}" ${storageBusy ? 'disabled' : ''}><button class="outline" data-action="save-storage-quota" ${storageBusy ? 'disabled' : ''}>${t('general.saveQuota')}</button></div>
      <p id="storage-notice" role="status">${storageNotice}</p>
      <small>${t('general.storageHint')}</small>
    </div>
    <div class="probe-box"><h3>${t('general.port')}</h3>
      <p>${t('general.portCurrent')} <strong>${networkSettings.default_port}</strong>，${t('general.portDesc')}</p>
      <label for="listen-port">${t('general.portLabel')}</label>
      <div class="probe-row"><input id="listen-port" type="number" min="1024" max="65535" value="${networkSettings.default_port}"><button class="outline" data-action="save-listen-port">${t('general.savePort')}</button></div>
      <p id="network-notice" role="status">${networkNotice}</p>
      <small>${t('general.portHint')}</small>
    </div>
    <div class="probe-box"><h3>${t('general.notice')}</h3>
      <p>${t('general.noticeCurrent')}：<strong>${noticeSettings.mode === 'popup' ? t('general.noticePopup') : noticeSettings.mode === 'tray' ? t('general.noticeTray') : t('general.noticeOff')}</strong></p>
      <label for="notice-mode">${t('general.noticeWhen')}</label>
      <select id="notice-mode">
        <option value="popup" ${noticeSettings.mode === 'popup' ? 'selected' : ''}>${t('general.noticePopup')}</option>
        <option value="tray" ${noticeSettings.mode === 'tray' ? 'selected' : ''}>${t('general.noticeTray')}</option>
        <option value="off" ${noticeSettings.mode === 'off' ? 'selected' : ''}>${t('general.noticeOff')}</option>
      </select>
      <p><label><input id="notice-raise" type="checkbox" ${noticeSettings.raise_window ? 'checked' : ''}> ${t('general.noticeRaise')}</label></p>
      <p><label><input id="notice-preview" type="checkbox" ${noticeSettings.preview ? 'checked' : ''}> ${t('general.noticePreview')}</label></p>
      <div class="probe-row"><button class="outline" data-action="save-notice-settings">${t('general.saveNotice')}</button></div>
      <p id="notice-settings-status" role="status">${noticeNotice}</p>
      <small>浮窗只在本机显示，不上传任何内容；应用锁生效时浮窗不显示消息摘要。</small>
    </div>
    <p>本机身份指纹（SHA-256）：<br><code class="fingerprint">${fingerprint}</code></p>
    <div class="probe-box"><h3>${t('close.section')}</h3>
      <p>${t('close.current')}：<strong>${CLOSE_ACTION_LABEL[windowSettings.close_action]()}</strong></p>
      <label for="close-action">${t('close.whenClick')}</label>
      <select id="close-action">
        <option value="ask" ${windowSettings.close_action === 'ask' ? 'selected' : ''}>${t('close.optAsk')}</option>
        <option value="tray" ${windowSettings.close_action === 'tray' ? 'selected' : ''}>${t('close.optTray')}</option>
        <option value="quit" ${windowSettings.close_action === 'quit' ? 'selected' : ''}>${t('close.optQuit')}</option>
      </select>
      <div class="probe-row"><button class="outline" data-action="save-close-action">${t('close.save')}</button></div>
      <p id="close-action-notice" role="status">${closeActionNotice}</p>
      <small>${t('close.hint')}</small>
    </div>
    <button data-page="about" class="outline">${t('about.aboutQbird')}</button>`;
}

type ExportTarget = { kind: string; target: string; name: string };
let exportTargets: ExportTarget[] = [];
let exportTargetsLoaded = false;
let exportTargetsLoading = false;
const exportSelected = new Set<string>();
let exportIncludeText = true;
let exportIncludeImages = true;
let exportIncludeFiles = true;
let exportBusy = false;
let exportWizardOpen = false;

function exportKey(item: ExportTarget): string { return `${item.kind}|${item.target}`; }

async function loadExportTargets(): Promise<void> {
  if (!isTauri() || exportTargetsLoaded || exportTargetsLoading) return;
  exportTargetsLoading = true;
  try {
    exportTargets = await invoke<ExportTarget[]>('v1_list_export_targets');
    exportTargetsLoaded = true;
  } catch (error) { exportTargets = []; workspace.notify(displayError(error)); }
  finally { exportTargetsLoading = false; }
  if ((page === 'settings' && settingsTab === 'backup') || page === 'messages') render();
}

/// Runs the export wizard: one .qchat file per selected conversation in the
/// chosen directory, encrypted with the archive password.
async function runExportWizard(): Promise<void> {
  if (!isTauri() || exportBusy) return;
  const password = app.querySelector<HTMLInputElement>('#chat-archive-password')?.value ?? '';
  if (password.length < 8) { chatArchiveNotice = t('聊天记录密码至少 8 个字符'); render(); return; }
  if (!exportSelected.size) { chatArchiveNotice = t('请先选择要导出的会话'); render(); return; }
  if (!exportIncludeText && !exportIncludeImages && !exportIncludeFiles) { chatArchiveNotice = t('至少选择一种导出内容'); render(); return; }
  const picked = await openFileDialog({ multiple: false, directory: true, title: t('选择导出目录') }).catch(() => null);
  if (!picked || typeof picked !== 'string') return;
  const selections = exportTargets.filter((item) => exportSelected.has(exportKey(item)))
    .map((item) => ({ kind: item.kind, target: item.target }));
  exportBusy = true; chatArchiveNotice = t('正在导出…'); render();
  try {
    const report = await invoke<{ exported: number; skipped: string[]; dir: string }>('v1_export_chat_archives',
      { selections, password, dir: picked, includeText: exportIncludeText, includeImages: exportIncludeImages, includeFiles: exportIncludeFiles });
    chatArchiveNotice = report.skipped.length
      ? `${t('已导出')} ${report.exported} ${t('个会话到')} ${report.dir}；${t('失败')} ${report.skipped.length} ${t('个')}：${report.skipped.join('；')}`
      : `${t('已导出')} ${report.exported} ${t('个会话到')} ${report.dir}。`;
  } catch (error) { chatArchiveNotice = displayError(error); }
  finally { exportBusy = false; if (page === 'settings') render(); }
}

function backupSettingsHtml(): string {
  const backupSummary = backupInfo ? t('pr.verifiedBackup', { n: backupInfo.message_count, time: new Date(backupInfo.created_at * 1000).toLocaleString(), chat: backupInfo.include_chat ? t('pr.withChat') : t('pr.noChat') }) : t('pr.noBackupYet');
  const targetRows = exportTargets.map((item) => {
    const key = exportKey(item);
    const checked = exportSelected.has(key) ? 'checked' : '';
    const kindLabel = item.kind === 'self' ? t('自聊') : item.kind === 'direct' ? t('单聊') : t('群聊');
    return `<label class="export-target"><input type="checkbox" data-export-target="${escapeText(key)}" ${checked}> <span>${escapeText(item.name)}</span><small>${kindLabel}</small></label>`;
  }).join('');
  return `<p>${t('备份使用密码加密并校验完整性；恢复前会校验备份文件。合并恢复会跳过相同同 ID 且内容一致的消息；覆盖恢复会先在本机保存一份恢复前快照。')}</p>
    <div class="probe-box"><h3>${t('pr.backup')}</h3>
      <p>${t('pr.backupIntro')}${backupSummary}</p>
      <label for="backup-password">${t('pr.backupPassword')}</label>
      <input id="backup-password" type="password" maxlength="256" autocomplete="new-password" value="${backupPassword.replace(/&/g, '&amp;').replace(/"/g, '&quot;').replace(/</g, '&lt;')}">
      <p><label><input id="backup-include-chat" type="checkbox" checked> ${t('pr.includeChat')}</label></p>
      <div class="probe-row"><button class="outline" data-action="backup-export" ${backupBusy ? 'disabled' : ''}>${t('pr.exportBackup')}</button><button class="outline" data-action="backup-choose" ${backupBusy ? 'disabled' : ''}>${t('pr.chooseBackup')}</button><button class="outline" data-action="backup-inspect" ${backupBusy || !backupPath ? 'disabled' : ''}>${t('pr.inspect')}</button></div>
      <p id="backup-path">${backupPath ? t('pr.currentFile', { path: backupPath }) : t('pr.mergeNote')}</p>
      <div class="probe-row"><button class="outline" data-action="backup-restore-merge" ${backupBusy || !backupPath ? 'disabled' : ''}>${t('pr.merge')}</button><button class="outline danger" data-action="backup-restore-overwrite" ${backupBusy || !backupPath ? 'disabled' : ''}>${t('pr.overwrite')}</button></div>
      <p id="backup-notice" role="status">${backupNotice}</p>
      <small>${t('pr.backupHint')}</small>
    </div>
    <div class="probe-box"><h3>${t('导出会话')}</h3>
      <p>${t('按联系人（单聊/群聊/自聊）选择会话，导出全部聊天记录；每个会话分别生成加密的 .qchat 文件。')}</p>
      <label for="chat-archive-password">${t('pr.archivePassword')}</label>
      <input id="chat-archive-password" type="password" maxlength="256" autocomplete="new-password">
      <div class="probe-row">
        <button class="outline" data-action="toggle-export-wizard">${exportWizardOpen ? t('收起会话列表') : t('选择要导出的会话…')}</button>
      </div>
      ${exportWizardOpen ? `<div class="export-wizard">
        <div class="export-targets">${exportTargetsLoaded ? (targetRows || `<p>${t('暂无可导出的会话')}</p>`) : t('读取中…')}</div>
        <label class="export-target"><input type="checkbox" id="export-select-all"> ${t('全选')}</label>
        <div class="probe-row export-content-options">
          <label><input type="checkbox" id="export-include-text" ${exportIncludeText ? 'checked' : ''}> ${t('文字消息')}</label>
          <label><input type="checkbox" id="export-include-images" ${exportIncludeImages ? 'checked' : ''}> ${t('图片')}</label>
          <label><input type="checkbox" id="export-include-files" ${exportIncludeFiles ? 'checked' : ''}> ${t('其他文件')}</label>
        </div>
        <div class="probe-row"><button class="outline" data-action="export-chat-archives" ${exportBusy ? 'disabled' : ''}>${t('导出所选会话')}</button></div>
      </div>` : ''}
      <p id="chat-archive-notice" role="status">${chatArchiveNotice}</p>
      <small>${t('聊天记录密码至少 8 个字符')}</small>
    </div>
    <div class="probe-box"><h3>${t('导入聊天记录')}</h3>
      <p>${t('pr.archiveIntro')}</p>
      <label for="chat-archive-import-password">${t('请输入导出时使用的密码')}</label>
      <input id="chat-archive-import-password" type="password" maxlength="256" autocomplete="off">
      <div class="probe-row"><button class="outline" data-action="import-chat-archive" ${chatArchiveBusy ? 'disabled' : ''}>${t('pr.importArchive')}</button></div>
      <p id="chat-archive-import-notice" role="status">${chatArchiveNotice}</p>
      <small>${t('pr.archiveHint')}</small>
    </div>`;
}

function privacySettingsHtml(): string {
  const idle = lockState.idle_minutes ? t('pr.idleMinutes', { n: lockState.idle_minutes }) : t('pr.off');
  const remembered = fileAccept.remembered.length ? t('pr.remembered', { n: fileAccept.remembered.length }) : t('pr.noneRemembered');
  const backupSummary = backupInfo ? t('pr.verifiedBackup', { n: backupInfo.message_count, time: new Date(backupInfo.created_at * 1000).toLocaleString(), chat: backupInfo.include_chat ? t('pr.withChat') : t('pr.noChat') }) : t('pr.noBackupYet');
  return `<div class="probe-box"><h3>${t('pr.requests')}</h3>
      <p>${t('pr.currentPolicy')}：<strong>${contactRequestMode === 'auto' ? t('pr.autoAccept') : t('pr.needConfirm')}</strong>；${t('pr.lanVisibility')}：<strong>${lanVisible ? t('pr.visible') : t('pr.hidden')}</strong>。</p>
      <select id="contact-request-mode">
        <option value="ask" ${contactRequestMode === 'ask' ? 'selected' : ''}>${t('pr.optAsk')}</option>
        <option value="auto" ${contactRequestMode === 'auto' ? 'selected' : ''}>${t('pr.optAuto')}</option>
      </select>
      <p><label><input id="lan-visible" type="checkbox" ${lanVisible ? 'checked' : ''}> ${t('pr.lanVisible')}</label></p>
      <div class="probe-row"><button class="outline" data-action="save-contact-privacy">${t('pr.save')}</button></div>
      <p id="contact-privacy-status" role="status">${privacyNotice}</p>
      <small>${t('pr.hint')}</small>
    </div>
    <div class="probe-box"><h3>${t('pr.appLock')}</h3>
      <p>${t('pr.appLockState')}：<strong>${lockState.has_password ? t('pr.hasPassword') : t('pr.noPassword')}</strong>；${t('pr.idleAuto')}：${idle}。</p>
      <label for="lock-current">${t('pr.currentPassword')}${lockState.has_password ? '' : t('pr.noPasswordYet')}</label>
      <input id="lock-current" type="password" maxlength="128" autocomplete="current-password">
      <label for="lock-new">${t('pr.newPassword')}</label>
      <input id="lock-new" type="password" maxlength="128" autocomplete="new-password">
      <div class="probe-row"><button class="outline" data-action="save-lock-password" ${lockBusy ? 'disabled' : ''}>${t('pr.saveLockPassword')}</button></div>
      <label for="lock-idle">${t('pr.idleLabel')}</label>
      <div class="probe-row"><input id="lock-idle" type="number" min="0" max="240" value="${lockState.idle_minutes}" ${lockBusy ? 'disabled' : ''}><button class="outline" data-action="save-lock-idle" ${lockBusy ? 'disabled' : ''}>${t('pr.saveIdle')}</button></div>
      <p id="lock-notice" role="status">${lockNotice}</p>
      <small>${t('pr.lockHint')}</small>
    </div>
    <div class="probe-box"><h3>${t('pr.fileRecv')}</h3>
      <p>${t('pr.currentPolicy')}：<strong>${ACCEPT_MODE_LABEL[fileAccept.mode]()}</strong>；${remembered}。</p>
      <label for="file-accept-mode">${t('pr.acceptModeLabel')}</label>
      <select id="file-accept-mode">
        <option value="always" ${fileAccept.mode === 'always' ? 'selected' : ''}>${t('pr.acceptAlways')}</option>
        <option value="remember" ${fileAccept.mode === 'remember' ? 'selected' : ''}>${t('pr.acceptRemember')}</option>
        <option value="auto" ${fileAccept.mode === 'auto' ? 'selected' : ''}>${t('pr.acceptAuto')}</option>
      </select>
      <div class="probe-row"><button class="outline" data-action="save-file-accept">${t('pr.saveAccept')}</button><button class="outline" data-action="forget-file-sources" ${fileAccept.remembered.length ? '' : 'disabled'}>${t('pr.forget')}</button></div>
      <label>${t('pr.directDir')}</label>
      <p><code class="recv-dir">${escapeText(fileRecv.direct || t('side.reading'))}</code>${fileRecv.direct_custom ? t('pr.custom') : t('pr.default')}</p>
      <div class="probe-row"><button class="outline" data-action="choose-recv-direct">${t('pr.changeDirect')}</button><button class="outline" data-action="reset-recv-direct" ${fileRecv.direct_custom ? '' : 'disabled'}>${t('ob.resetDefault')}</button></div>
      <label>${t('pr.groupDir')}</label>
      <p><code class="recv-dir">${escapeText(fileRecv.group || t('side.reading'))}</code>${fileRecv.group_custom ? t('pr.custom') : t('pr.default')}</p>
      <div class="probe-row"><button class="outline" data-action="choose-recv-group">${t('pr.changeGroup')}</button><button class="outline" data-action="reset-recv-group" ${fileRecv.group_custom ? '' : 'disabled'}>${t('ob.resetDefault')}</button></div>
      <p id="file-accept-notice" role="status">${fileAcceptNotice}</p>
      <small>${t('pr.recvHint')}</small>
    </div>
`;
}

function workspaceSettingsHtml(): string {
  return `<div class="probe-box"><h3>${t('ws.input')}</h3>
      <label for="send-shortcut">${t('ws.sendMethod')}</label>
      <select id="send-shortcut">
        <option value="enter" ${composer.send_shortcut === 'enter' ? 'selected' : ''}>${t('ws.enterSend')}</option>
        <option value="ctrl+enter" ${composer.send_shortcut === 'ctrl+enter' ? 'selected' : ''}>${t('ws.ctrlEnterSend')}</option>
      </select>
      <label for="composer-rows">${t('ws.rowsLabel')}</label>
      <div class="probe-row"><input id="composer-rows" type="number" min="2" max="12" value="${composer.rows}"><button class="outline" data-action="save-composer">${t('ws.saveComposer')}</button></div>
      <p id="composer-notice" role="status">${composerNotice}</p>
      <small>${t('ws.inputHint')}</small>
    </div>
    <div class="probe-box"><h3>${t('ws.team')}</h3>
      <p>${t('ws.teamName')} ${team.name ? `<strong>${escapeText(team.name)}</strong>` : t('ws.unset')}；${t('ws.teamLogo')}：${team.has_logo ? t('ws.set') : t('ws.unsetShort')}。</p>
      <label for="team-name">${t('ws.teamNameLabel')}</label>
      <div class="probe-row"><input id="team-name" type="text" maxlength="64" value="${escapeText(team.name)}"><button class="outline" data-action="save-team-name">${t('ws.saveName')}</button></div>
      <div class="probe-row"><button class="outline" data-action="choose-team-logo">${t('ws.chooseLogo')}</button><button class="outline" data-action="clear-team-logo" ${team.has_logo ? '' : 'disabled'}>${t('ws.removeLogo')}</button></div>
      <p id="team-notice" role="status">${teamNotice}</p>
      <small>${t('ws.teamHint')}</small>
    </div>
    <div class="probe-box"><h3>${t('ws.rightPanel')}</h3>
      <p>${t('ws.imageMode')}：${imageOpenMode === 'workspace' ? t('ws.inWorkspace') : t('ws.inLightbox')}；${t('ws.fileMode')}：${fileOpenMode === 'system' ? t('ws.withSystem') : t('ws.inWorkspace')}。</p>
      <label for="image-open-mode">${t('ws.whenImage')}</label>
      <select id="image-open-mode">
        <option value="lightbox" ${imageOpenMode === 'lightbox' ? 'selected' : ''}>${t('ws.lightboxOpt')}</option>
        <option value="workspace" ${imageOpenMode === 'workspace' ? 'selected' : ''}>${t('ws.workspaceOpt')}</option>
      </select>
      <div class="probe-row"><button class="outline" data-action="save-image-open-mode">${t('ws.saveImageMode')}</button></div>
      <p id="image-open-notice" role="status">${imageOpenNotice}</p>
      <label for="file-open-mode">${t('ws.whenFile')}</label>
      <select id="file-open-mode">
        <option value="workspace" ${fileOpenMode === 'workspace' ? 'selected' : ''}>${t('ws.inWorkspace')}</option>
        <option value="system" ${fileOpenMode === 'system' ? 'selected' : ''}>${t('ws.withSystem')}</option>
      </select>
      <div class="probe-row"><button class="outline" data-action="save-file-open-mode">${t('ws.saveFileMode')}</button></div>
      <p id="file-open-notice" role="status">${fileOpenNotice}</p>
      <p>${t('ws.panelDesc')}</p>
      <small>${t('ws.panelHint')}</small>
    </div>
    <div class="probe-box"><h3>${t('ws.emoji')}</h3>
      <p>${t('ws.emojiDesc')}</p>
      <small>${t('ws.emojiHint')}</small>
    </div>
`;
}

function experimentalSettingsHtml(): string {
  return `<div class="probe-box"><h3>${t('exp.title')}</h3>
      <p><label><input id="experiment-group-file-quote" type="checkbox" ${experiments.group_file_quote ? 'checked' : ''}> ${t('exp.groupQuote')}</label></p>
      <p>${t('exp.groupQuoteDesc')}</p>
      <div class="probe-row"><button class="outline" data-action="save-experiment">${t('exp.save')}</button></div>
      <p id="experiment-notice" role="status">${experimentNotice}</p>
      <small>${t('exp.agentNote')}</small>
    </div>
    <div class="probe-box"><h3>${t('exp.plugins')}</h3>
      <p>${t('exp.pluginIntro')}</p>
      <ul id="plugin-list" class="discovery-results">${plugins.map(plugin => `<li><span class="contact-title">${escapeText(plugin.name)} ${escapeText(plugin.version)} · ${escapeText(plugin.extensions.join(' / ')) || t('exp.noExt')}</span><div class="contact-actions"><label><input type="checkbox" data-plugin-toggle="${escapeText(plugin.id)}" ${plugin.enabled ? 'checked' : ''}> ${t('exp.enable')}</label><button class="linkish" data-action="remove-plugin" data-id="${escapeText(plugin.id)}">${pluginRemovePending === plugin.id ? t('exp.confirmRemove') : t('exp.remove')}</button></div></li>`).join('') || `<li>${t('exp.noPlugins')}</li>`}</ul>
      <div class="probe-row"><button class="outline" data-action="install-plugin">${t('exp.install')}</button></div>
      <p id="plugin-notice" role="status">${pluginNotice}</p>
      <small>${t('exp.pluginHint')}</small>
    </div>`;
}

function diagLine(label: string, value: string, tone: 'ok' | 'warn' | 'muted' = 'muted'): string {
  return `<div class="diag-row"><span class="diag-label">${escapeText(label)}</span><span class="diag-value ${tone}">${escapeText(value)}</span></div>`;
}

function diskText(disk: DiskSpace): string {
  if (disk.free_bytes === null || disk.total_bytes === null) return `${t('未获取')}${disk.error ? `${t('：')}${disk.error}` : ''}`;
  return `${t('可用')} ${formatBytes(disk.free_bytes)} / ${t('共')} ${formatBytes(disk.total_bytes)}`;
}

function networkReportHtml(): string {
  if (!networkReport) return `<p class="hint">${t('尚未检测。点击“重新检测”读取本机网络状态。')}</p>`;
  const report = networkReport;
  const service = report.service_running ? `${t('运行中 · 端口')} ${report.service_port ?? t('未知')}` : t('未启动');
  const chat = report.service_running
    ? (report.chat_enabled ? t('已启用（可接收消息和文件）') : t('未启用（仅身份探测）'))
    : t('不可用（服务未启动）');
  const portOk = report.default_port_available || (report.service_running && report.service_port === report.default_port);
  const port = `${report.default_port} · ${report.default_port_available ? t('空闲') : (portOk ? t('由本机监听占用') : t('被其他程序占用'))}`;
  const discovery = report.discovery_started ? `${t('已启动 · 候选')} ${report.lan_candidates} ${t('个')}` : t('未启动');
  return `<div class="diag-grid">
      ${diagLine(t('平台 / 主机'), `${report.platform} · ${report.hostname}`)}
      ${diagLine(t('本机 IPv4'), report.local_ipv4.length ? report.local_ipv4.join('、') : t('未获取'), report.local_ipv4.length ? 'ok' : 'warn')}
      ${diagLine(t('监听服务'), service, report.service_running ? 'ok' : 'warn')}
      ${diagLine(t('聊天接收'), chat, report.chat_enabled ? 'ok' : 'warn')}
      ${diagLine(t('默认端口'), port, portOk ? 'ok' : 'warn')}
      ${diagLine(t('局域网发现'), discovery, report.lan_candidates > 0 ? 'ok' : 'muted')}
      ${diagLine(t('已确认联系人'), String(report.contacts))}
    </div>
    ${report.notes.length ? `<ul class="diag-notes">${report.notes.map((note) => `<li>${escapeText(note)}</li>`).join('')}</ul>` : `<p class="diag-ok">${t('未发现明显网络异常。')}</p>`}`;
}

function recvDirLine(label: string, dir: RecvDirReport): string {
  const suffix = dir.custom ? t('（自定义）') : t('（默认）');
  const usage = dir.exists ? formatBytes(dir.usage.bytes) : t('目录不存在');
  return diagLine(label, `${dir.path}${suffix} · ${usage} · ${diskText(dir.disk)}`, dir.exists ? 'muted' : 'warn');
}

function spaceReportHtml(): string {
  if (!spaceReport) return `<p class="hint">${t('尚未检测。点击“重新检测空间”读取本地占用与磁盘余量。')}</p>`;
  const report = spaceReport;
  const percent = report.attachments_quota > 0
    ? Math.min(100, Math.round((report.attachments_bytes / report.attachments_quota) * 100))
    : 0;
  return `<div class="diag-grid">
      ${diagLine(t('应用数据目录'), `${report.data_dir}（${formatBytes(report.data_dir_usage.bytes)}）`)}
      ${diagLine(t('数据目录磁盘'), diskText(report.data_dir_disk))}
      ${diagLine(t('数据库文件'), formatBytes(report.database_bytes))}
      ${diagLine(t('附件占用'), `${formatBytes(report.attachments_bytes)} / ${formatBytes(report.attachments_quota)}（${percent}%）`)}
      <progress max="100" value="${percent}"></progress>
      ${recvDirLine(t('单聊接收目录'), report.direct)}
      ${recvDirLine(t('群聊接收目录'), report.group)}
    </div>
    ${report.notes.length ? `<ul class="diag-notes">${report.notes.map((note) => `<li>${escapeText(note)}</li>`).join('')}</ul>` : `<p class="diag-ok">${t('空间状态正常。')}</p>`}`;
}

let logText = '';
let logVisible = false;

function debugSettingsHtml(): string {
  return `<p>${t('用于排查“看不到同事、收不到消息或文件”的本机自检。检测只读取本机状态，不上传任何内容。')}</p>
    <div class="probe-box"><h3>${t('网络检测与修复')}</h3>
      <div class="probe-row">
        <button class="outline" data-action="debug-scan" ${diagnosticsLoading ? 'disabled' : ''}>${t('重新检测')}</button>
        <button class="outline" data-action="debug-repair" data-mode="restart" ${repairBusy ? 'disabled' : ''}>${t('重启聊天服务')}</button>
        <button class="outline" data-action="debug-repair" data-mode="rediscover" ${repairBusy ? 'disabled' : ''}>${t('重建局域网发现')}</button>
        <button class="outline" data-action="debug-repair" data-mode="all" ${repairBusy ? 'disabled' : ''}>${t('一键修复')}</button>
      </div>
      <p id="debug-network-notice" role="status">${escapeText(diagnosticsNotice)}</p>
      <div id="debug-network-report">${networkReportHtml()}</div>
      <small>${t('“重启聊天服务”会短暂断开监听后按原端口恢复，原端口不可用时改用默认端口；“重建局域网发现”只刷新广播浏览。两者都不会改动身份、联系人和聊天记录。')}</small>
    </div>
    <div class="probe-box"><h3>${t('日志')}</h3>
      <div class="probe-row">
        <button class="outline" data-action="debug-log-view">${t('查看最新日志')}</button>
        <button class="outline" data-action="debug-log-dir">${t('打开日志目录')}</button>
      </div>
      <pre id="debug-log-view" class="debug-log" hidden></pre>
      <small>${t('日志保存在本机数据目录的 logs 文件夹，仅用于排查问题；提交问题反馈时可附上日志，但请注意其中可能包含本机路径等信息。')}</small>
    </div>
    <div class="probe-box"><h3>${t('空间检测')}</h3>
      <div class="probe-row"><button class="outline" data-action="debug-space" ${diagnosticsLoading ? 'disabled' : ''}>${t('重新检测空间')}</button></div>
      <div id="debug-space-report">${spaceReportHtml()}</div>
      <small>${t('统计为本地文件实际占用。自定义接收目录位于应用数据目录之外时不受存储配额限制，但同样受磁盘容量限制；空间不足通常是“对方发送失败”或“接收失败”的原因之一。')}</small>
    </div>`;
}

async function loadDebugData(): Promise<void> {
  const [network, space] = await Promise.all([
    invoke<NetworkReport>('v1_network_diagnostics'),
    invoke<SpaceReport>('v1_space_diagnostics'),
  ]);
  networkReport = network;
  spaceReport = space;
  try { logText = await invoke<string>('v1_read_logs'); } catch { logText = ''; }
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
  const logBox = app.querySelector<HTMLElement>('#debug-log-view');
  if (logBox) { logBox.hidden = !logVisible; logBox.textContent = logText || t('暂无日志。'); }
}

async function runNetworkRepair(mode: 'restart' | 'rediscover' | 'all'): Promise<void> {
  if (!isTauri() || repairBusy) return;
  repairBusy = true;
  diagnosticsNotice = t('正在执行修复…');
  paintDebugReports();
  try {
    const report = await invoke<RepairReport>('v1_network_repair', { action: mode });
    const summary = report.steps.map((step) => `${step.ok ? '✓' : '✗'} ${step.step}${t('：')}${step.detail}`).join(t('；'));
    diagnosticsNotice = `${t('修复完成：')}${summary}`;
    try { await loadDebugData(); }
    catch { diagnosticsNotice += t('；修复后重新检测失败，请手动重新检测。'); }
  } catch (error) {
    diagnosticsNotice = `${t('修复失败：')}${displayError(error)}`;
  } finally {
    repairBusy = false;
    paintDebugReports();
  }
}

async function takeScreenshot(): Promise<void> {
  if (!isTauri()) return;
  if (!direct.peer && !groupChat.groupId) { groupChat.notify(t('请先打开一个单聊或群聊再截图。')); return; }
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
  if (!extension) { groupChat.notify(t('仅支持粘贴 PNG/BMP 图片；JPG 请先另存为文件再拖入。')); return; }
  try {
    const bytes = Array.from(new Uint8Array(await file.arrayBuffer()));
    if (!bytes.length || bytes.length > 16 * 1024 * 1024) { groupChat.notify(t('粘贴的图片过大（上限 16 MiB）')); return; }
    const path = await invoke<string>('v1_save_temp_image', { bytes, extension });
    if (direct.peer) await direct.stagePaths([path]);
    else if (groupChat.groupId) await groupChat.stagePaths([path]);
  } catch (error) { groupChat.notify(displayError(error)); }
}

async function refreshFileAccept(): Promise<void> {
  if (!isTauri()) return;
  try { fileAccept = await invoke<FileAcceptSettings>('v1_file_accept_settings'); }
  catch { fileAcceptNotice = t('status.fileAcceptReadErr'); }
  try { fileRecv = await invoke<FileRecvSettings>('v1_file_recv_settings'); }
  catch { fileAcceptNotice = t('status.recvDirReadErr'); }
}

async function saveFileAccept(): Promise<void> {
  if (!isTauri()) return;
  const mode = app.querySelector<HTMLSelectElement>('#file-accept-mode')?.value ?? 'always';
  try {
    fileAccept = await invoke<FileAcceptSettings>('v1_set_file_accept_policy', { mode });
    fileAcceptNotice = `${t('pr.acceptSaved')}${ACCEPT_MODE_LABEL[fileAccept.mode]()}。`;
  } catch (error) { fileAcceptNotice = displayError(error); }
  if (page === 'settings') render();
}

async function forgetFileSources(): Promise<void> {
  if (!isTauri()) return;
  try {
    fileAccept = await invoke<FileAcceptSettings>('v1_forget_file_sources');
    fileAcceptNotice = t('status.forgotSources');
  } catch (error) { fileAcceptNotice = displayError(error); }
  if (page === 'settings') render();
}

async function chooseRecvDir(kind: 'direct' | 'group'): Promise<void> {
  if (!isTauri()) return;
  try {
    const picked = await openFileDialog({ directory: true, multiple: false, title: kind === 'direct' ? t('选择单聊文件接收目录') : t('选择群聊文件接收目录') });
    if (typeof picked !== 'string') return;
    fileRecv = await invoke<FileRecvSettings>('v1_set_file_recv_dir', { kind, path: picked });
    fileAcceptNotice = t('status.recvDirUpdated', { kind: kind === 'direct' ? t('status.kindDirect') : t('status.kindGroup') });
  } catch (error) { fileAcceptNotice = displayError(error); }
  if (page === 'settings') render();
}

async function resetRecvDir(kind: 'direct' | 'group'): Promise<void> {
  if (!isTauri()) return;
  try {
    fileRecv = await invoke<FileRecvSettings>('v1_set_file_recv_dir', { kind, path: null });
    fileAcceptNotice = t('status.recvDirReset', { kind: kind === 'direct' ? t('status.kindDirect') : t('status.kindGroup') });
  } catch (error) { fileAcceptNotice = displayError(error); }
  if (page === 'settings') render();
}

async function refreshExperiments(): Promise<void> {
  if (!isTauri()) return;
  try { experiments = await invoke<ExperimentSettings>('v1_experiment_settings'); }
  catch { experimentNotice = t('status.expReadErr'); }
}

async function saveExperiments(): Promise<void> {
  if (!isTauri()) return;
  const enabled = app.querySelector<HTMLInputElement>('#experiment-group-file-quote')?.checked ?? false;
  try {
    experiments = await invoke<ExperimentSettings>('v1_set_experiment_settings', { groupFileQuote: enabled });
    experimentNotice = enabled ? t('status.quoteOn') : t('status.quoteOff');
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
  } catch { if (page === 'settings') storageNotice = t('status.storageReadErr'); }
}

async function saveQuota(): Promise<void> {
  const input = app.querySelector<HTMLInputElement>('#storage-quota-mib');
  const notice = app.querySelector<HTMLElement>('#storage-notice');
  if (!isTauri() || !input || storageBusy) return;
  const mib = Number(input.value);
  if (!Number.isInteger(mib) || mib < 256 || mib > 65536) { if (notice) notice.textContent = t('请输入 256–65536 之间的整数（MiB）'); return; }
  storageBusy = true; if (notice) notice.textContent = t('正在保存…');
  try {
    storageInfo = await invoke<StorageUsage>('v1_set_storage_quota', { bytes: mib * 1024 * 1024 });
    storageNotice = t('status.quotaSavedMsg');
  } catch (error) { storageNotice = trError(error, t('无法保存配额')); }
  finally {
    storageBusy = false;
    if (page === 'settings') { render(); }
  }
}

async function importChatArchive(): Promise<void> {
  if (!isTauri() || chatArchiveBusy) return;
  const password = app.querySelector<HTMLInputElement>('#chat-archive-import-password')?.value ?? '';
  if (password.length < 8) { chatArchiveNotice = t('请输入导出时使用的密码'); render(); return; }
  const picked = await openFileDialog({ multiple: false, filters: [{ name: 'Qbird 聊天记录', extensions: ['qchat'] }] }).catch(() => null);
  if (!picked || typeof picked !== 'string') return;
  chatArchiveBusy = true; chatArchiveNotice = t('正在导入…'); render();
  try {
    const report = await invoke<{ kind: string; name: string; inserted: number; skipped: number; conflicts: number; restored_attachments: number }>('v1_import_chat_archive',
      { password, path: picked });
    chatArchiveNotice = `已导入「${report.name}」：新增 ${report.inserted} 条，跳过重复 ${report.skipped} 条，恢复附件 ${report.restored_attachments} 个${report.conflicts ? `，冲突 ${report.conflicts} 条（未覆盖）` : ''}。`;
    if (groupChat.groupId) void groupChat.load(false, true);
    if (direct.peer) void direct.load(false, true);
  } catch (error) { chatArchiveNotice = displayError(error); }
  finally { chatArchiveBusy = false; if (page === 'settings') render(); }
}

async function exportBackup(): Promise<void> {
  if (!isTauri() || backupBusy) return;
  if (backupPassword.length < 8) { backupNotice = t('status.backupPwShort'); if (page === 'settings') render(); return; }
  const includeChat = app.querySelector<HTMLInputElement>('#backup-include-chat')?.checked ?? true;
  const date = new Date();
  const stamp = `${date.getFullYear()}${String(date.getMonth() + 1).padStart(2, '0')}${String(date.getDate()).padStart(2, '0')}`;
  const target = await saveFileDialog({
    title: t('导出 Qbird 备份'),
    defaultPath: `qbird-backup-${stamp}.qbird`,
    filters: [{ name: 'Qbird 备份', extensions: ['qbird'] }],
  }).catch(() => null);
  if (!target) return;
  backupBusy = true; backupNotice = t('status.backupGenerating');
  try {
    const info = await invoke<BackupInfo>('v1_create_backup', { password: backupPassword, path: target, includeChat });
    backupInfo = info; backupPath = target;
    backupNotice = t('status.backupGenerated', { n: info.message_count });
  } catch (error) { backupNotice = trError(error, t('status.backupExportErr')); }
  finally { backupBusy = false; if (page === 'settings') render(); }
}

async function chooseBackup(): Promise<void> {
  if (!isTauri() || backupBusy) return;
  const selected = await openFileDialog({
    title: t('选择 Qbird 备份'),
    multiple: false,
    filters: [{ name: 'Qbird 备份', extensions: ['qbird'] }],
  }).catch(() => null);
  if (!selected || Array.isArray(selected)) return;
  backupPath = selected; backupInfo = null;
  backupNotice = t('已选择备份文件；输入密码后可校验或恢复。');
  if (page === 'settings') render();
}

async function inspectBackup(): Promise<void> {
  if (!isTauri() || backupBusy || !backupPath) return;
  backupBusy = true; backupNotice = t('正在校验…');
  try {
    backupInfo = await invoke<BackupInfo>('v1_inspect_backup', { password: backupPassword, path: backupPath });
    backupNotice = t('密码正确，备份完整性校验通过。');
  } catch (error) { backupInfo = null; backupNotice = trError(error, t('校验失败')); }
  finally { backupBusy = false; if (page === 'settings') render(); }
}

async function restoreBackup(overwrite: boolean): Promise<void> {
  if (!isTauri() || backupBusy || !backupPath) return;
  if (overwrite && !window.confirm(t('覆盖恢复会先保存一份恢复前快照，然后用备份替换联系人与聊天数据。确认继续？'))) return;
  backupBusy = true; backupNotice = overwrite ? t('正在覆盖恢复…') : t('正在合并恢复…');
  try {
    const report = await invoke<RestoreReport>('v1_restore_backup', { password: backupPassword, path: backupPath, overwrite });
    backupNotice = report.conflicts > 0
      ? `发现 ${report.conflicts} 条相同 ID 但内容不同的消息，已中止恢复，未修改任何数据。`
      : `恢复完成：${report.mode === 'overwrite' ? t('覆盖') : t('合并')}，新增 ${report.inserted} 条消息${report.skipped ? `，跳过 ${report.skipped} 条已有消息` : ''}${report.restored_attachments ? `，恢复附件 ${report.restored_attachments} 个文件` : ''}。${report.snapshot_path ? t('恢复前快照已保存在本机数据目录。') : ''}`;
    await refreshConversations();
  } catch (error) { backupNotice = trError(error, t('恢复失败，现有数据未被修改')); }
  finally { backupBusy = false; if (page === 'settings') render(); }
}

function shortId(value: string): string {
  const upper = (value || '').toUpperCase();
  return upper.length >= 8 ? `QB-${upper.slice(0, 4)}-${upper.slice(4, 8)}` : 'QB-0000-0000';
}

const USERNAME_PATTERN = /^[A-Za-z_][A-Za-z0-9_]{1,19}$/;
const USERNAME_HINT = '2–20 位英文字母、数字或下划线，不能以数字开头';

/// A random, rule-compliant suggestion. The nickname is deliberately not used
  /// here: two colleagues called "Alex" would otherwise end up with the same
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
    [1, t('ob.step'), t('ob.stepSub1')],
    [2, t('ob.step2'), t('ob.stepSub2')],
    [3, t('ob.step3'), t('ob.stepSub3')],
  ] as const;
  return steps.map(([step, title, sub]) => `
    <li class="${onboardingStep === step ? 'current' : ''} ${onboardingStep > step ? 'done' : ''}">
      <span>0${step}</span><strong>${title}</strong><small>${sub}</small>
    </li>`).join('');
}

function onboardingBrand(): string {
  return `<aside class="ob-brand">
    <div class="ob-brand-top"><span class="ob-logo">Q</span><div><strong>${t('page.about.heading')}</strong><small>${t('ob.brandSub')}</small></div></div>
    <div class="ob-brand-copy">
      <p class="ob-quote">${t('ob.quote1')}<br>${t('ob.quote2')}</p>
      <p class="ob-desc">${t('ob.desc')}</p>
    </div>
    <ol class="ob-steps">${onboardingSteps()}</ol>
    <p class="ob-brand-foot">${t('ob.foot')}</p>
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
        <h1>${t('ob.s1.h1')}</h1>
        <p class="lede">${t('ob.s1.lede')}</p>
        <div class="ob-avatar-row">
          <div class="ob-avatar">${avatarMarkup(draft.nickname || onboarding?.profile.nickname || myNickname)}</div>
          <div><strong class="ob-avatar-name">${escapeText(draft.nickname || t('ob.s1.avatar'))}</strong><p class="hint">${t('ob.s1.avatarHint')}</p></div>
          <span class="ob-id">${qbirdId || 'QB-…'}</span>
        </div>
        <div class="field"><label for="ob-nickname">${t('ob.nickname')} <span class="req">${t('ob.required')}</span></label>
          <input id="ob-nickname" type="text" maxlength="24" autocomplete="off" value="${escapeText(draft.nickname)}" placeholder="${escapeText(t('例如：Alex'))}">
          <p class="hint">${t('ob.nicknameHint')}</p></div>
        <div class="field"><label for="ob-username">${t('ob.username')} <span class="req">${t('ob.required')}</span></label>
          <div class="input-prefix"><span>@</span><input id="ob-username" type="text" maxlength="20" autocomplete="off" spellcheck="false" value="${escapeText(draft.username)}"></div>
          <p class="hint">局域网内用来搜索你：${USERNAME_HINT}，不能以数字开头。 <span id="ob-username-state" class="${USERNAME_PATTERN.test(draft.username) ? 'ok' : 'warn'}">${USERNAME_PATTERN.test(draft.username) ? t('可用') : t('待填写')}</span></p></div>
        <div class="field"><label for="ob-email">${t('ob.email')} <span class="opt">${t('ob.optional')}</span></label>
          <input id="ob-email" type="email" maxlength="128" autocomplete="off" value="${escapeText(draft.email)}" placeholder="${t('ob.emailPlaceholder')}"></div>
        <div class="ob-actions"><span class="hint">${t('ob.s1.actionsHint')}</span>
          <button class="ob-primary" data-action="ob-next" data-step="2">${t('ob.next')}</button></div>
      </div>` : step === 2 ? `
      <div class="ob-page">
        <p class="eyebrow">STEP 2 / 3</p>
        <h1>${t('ob.s2.h1')}</h1>
        <p class="lede">${t('ob.s2.lede')} <code>data</code>${t('ob.s2.ledeTail')}</p>
        <div class="field"><label for="ob-path">${t('ob.s2.pathLabel')}</label>
          <div class="ob-path-row"><input id="ob-path" type="text" readonly value="${escapeText(dir || t('读取中…'))}"><button class="ob-ghost" data-action="ob-pick-dir">${t('ob.change')}</button><button class="ob-text" data-action="ob-reset-dir">${t('ob.resetDefault')}</button></div>
          <p class="hint">${draft.recvDir ? t('ob.s2.customPrefix') : ''}${t('ob.s2.pathHint')}</p></div>
        <div class="ob-callout"><strong>${t('ob.s2.callout')}</strong><p>${t('ob.s2.calloutBody')}</p></div>
        <div class="ob-actions"><button class="ob-text" data-action="ob-back">${t('ob.back')}</button><button class="ob-primary" data-action="ob-next" data-step="3">${t('ob.next')}</button></div>
      </div>` : `
      <div class="ob-page">
        <p class="eyebrow">STEP 3 / 3</p>
        <h1>${t('ob.s3.h1')}</h1>
        <p class="lede">${t('ob.s3.lede')}</p>
        <div class="ob-scan"><span class="pulse"></span><span id="ob-lan-status">${escapeText(lanStatus)}</span><span class="demo-tag">${t('ob.s3.unverified')}</span></div>
        <ul class="people" id="ob-people">${lanPeople.length ? '' : `<li class="people-empty">${lanLoading ? t('ob.s3.scanning') : t('ob.s3.none')}</li>`}</ul>
        <p class="hint">${t('ob.s3.hint')}</p>
        <p id="ob-lan-notice" class="hint" role="status">${escapeText(lanNotice)}</p>
        <p class="ob-notice" role="status">${escapeText(onboardingNotice)}</p>
        <div class="ob-actions"><button class="ob-text" data-action="ob-finish">${t('ob.later')}</button>
          <button class="ob-primary" data-action="ob-finish" ${onboardingBusy ? 'disabled' : ''}>${onboardingBusy ? t('ob.finishing') : t('ob.finish')}</button></div>
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
  state.textContent = valid ? t('可用') : value ? `格式不正确：${USERNAME_HINT}` : t('待填写');
  state.className = valid ? 'ok' : 'warn';
}

async function submitOnboarding(): Promise<void> {
  syncOnboardingDraft();
  const draft = onboardingDraft;
  const nickname = draft.nickname.trim();
  const username = draft.username.trim();
  if (!nickname) { onboardingNotice = t('请先填写昵称'); paintOnboarding(); return; }
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
      listenNotice = t('已开始监听局域网，同事可以发现你。');
    } catch {
      listenNotice = t('监听未启动：端口可能被占用，可在局域网用户页重试。');
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
  const meta = `<span>${escapeText(person.address)}</span><span class="sep">·</span><span>${person.known ? t('已添加') : t('身份未核对')}</span>`;
  return `<li data-fingerprint="${person.fingerprint}" class="${person.known ? 'is-added' : ''}">
    <span class="av av-40">${initial}</span>
    <div class="p-body"><div class="p-name"><strong>${escapeText(name)}</strong>${handle}</div><div class="p-meta">${meta}</div></div>
    ${person.known
      ? (allowOpen
        ? `<button class="btn-add is-added" data-action="open-direct" data-peer="${person.fingerprint}" data-name="${escapeText(person.nickname || name)}">发消息</button>`
        : '<span class="btn-add is-added" role="status">已添加</span>')
      : `<button class="btn-add" data-action="lan-add" data-address="${person.address}" data-fingerprint="${person.fingerprint}" data-name="${escapeText(name)}" data-username="${person.username ? escapeText(person.username) : ''}" ${lanAddBusy === person.fingerprint ? 'disabled' : ''}>${lanAddBusy === person.fingerprint ? t('添加中…') : t('添加')}</button>`}
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
    : `<li class="people-empty">${lanLoading ? t('正在识别同网段用户…') : lanPeople.length ? t('没有匹配的用户') : t('暂未发现其他用户。请确认对方已启动 Qbird，且与本机在同一网段。')}</li>`;
}

function discoveryView(): string {
  const since = lanRefreshedAt ? `${Math.max(1, Math.round((Date.now() - lanRefreshedAt) / 1000))} 秒前刷新` : t('尚未刷新');
  const subnet = lanPeople[0]?.address.replace(/\.\d+:\d+$/, '.0/24') || t('同网段');
  return `<section class="lan-page">
    <header class="lan-head">
      <div><span class="eyebrow">DISCOVERY / 局域网</span><h2>局域网用户</h2>
        <p>他们正在使用 Qbird，并且和你在同一个网络。</p></div>
      <span class="pill ${probeStatus.running ? 'pill-live' : ''}">${probeStatus.running ? `<span class="pulse"></span>监听中 · ${probeStatus.port}` : t('未监听')}</span>
    </header>
    <div class="lan-bar">
      <span class="lan-net">网段 <code>${escapeText(subnet)}</code></span>
      <input id="lan-search" type="search" maxlength="64" placeholder="${escapeText(t('搜索昵称或用户名'))}" aria-label="${escapeText(t('搜索局域网用户'))}" value="${escapeText(lanQuery)}">
      <button class="outline" data-action="lan-refresh" ${lanLoading ? 'disabled' : ''}>${lanLoading ? t('刷新中…') : t('立即刷新')}</button>
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
          <div class="probe-row"><input id="lan-manual" type="text" placeholder="192.168.2.8:42853"><input id="lan-manual-name" type="text" maxlength="24" placeholder="${escapeText(t('对方昵称（可选）'))}"><button class="outline" data-action="lan-manual-add">探测并添加</button></div>
          <p class="hint">适用于跨 VLAN、VPN 或已知 IP 的同事。添加前会探测地址，确认对方身份与列表一致。</p></div>
        <div class="field"><label for="scan-cidr">扫描指定网段</label>
          <div class="probe-row"><input id="scan-cidr" type="text" maxlength="24" spellcheck="false" placeholder="192.168.2.0/24" value="${escapeText(scanCidr)}"><button class="outline" data-action="scan-start" ${scanBusy ? 'disabled' : ''}>${scanBusy ? t('扫描中…') : t('开始扫描')}</button></div>
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
    <p id="lan-notice" class="hint" role="status">${escapeText(lanNotice)}</p>
  </section>`;
}

function paintLan(): void {
  paintLanPeopleList(app.querySelector<HTMLUListElement>('#lan-list'));
  paintLanPeopleList(app.querySelector<HTMLUListElement>('#ob-people'), false);
  for (const selector of ['#lan-notice', '#ob-lan-notice']) {
    const notice = app.querySelector<HTMLElement>(selector);
    if (notice) notice.textContent = lanNotice;
  }
  const status = app.querySelector<HTMLElement>('#lan-status');
  if (status) {
    const since = lanRefreshedAt ? `${Math.max(1, Math.round((Date.now() - lanRefreshedAt) / 1000))} 秒前刷新` : t('尚未刷新');
    status.textContent = `${lanStatus} · ${since}`;
  }
}

async function refreshLanPeople(force = false): Promise<void> {
  if (!isTauri() || lanLoading) return;
  if (!force && lanRefreshedAt && Date.now() - lanRefreshedAt < 25_000) return;
  lanLoading = true;
  lanStatus = t('正在识别同网段用户…');
  if (page === 'discover') paintLan();
  if (!onboarding?.done && onboardingStep === 3) paintOnboarding();
  try {
    lanPeople = await invoke<LanPerson[]>('v1_lan_people');
    lanRefreshedAt = Date.now();
    lanStatus = lanPeople.length ? `发现 ${lanPeople.length} 位用户` : t('暂未发现其他用户');
  } catch {
    lanPeople = [];
    lanStatus = t('局域网发现不可用，请检查网络和防火墙');
  } finally {
    lanLoading = false;
    if (page === 'discover') paintLan();
    if (!onboarding?.done && onboardingStep === 3) paintOnboarding();
  }
}

type AddContactArgs = { address: string; fingerprint: string; displayName: string; username: string | null; senderNickname: string | null; senderUsername: string | null };

function askContactReason(): Promise<string | null> {
  if (lockState.locked) return Promise.resolve(null);
  return new Promise((resolve) => {
    const backdrop = document.createElement('div');
    backdrop.className = 'close-dialog-backdrop';
    backdrop.innerHTML = `<section class="close-dialog" role="dialog" aria-modal="true" aria-labelledby="contact-reason-title">
      <h2 id="contact-reason-title">填写验证信息</h2><p>对方开启了好友验证，请告诉对方你是谁。</p>
      <input id="contact-reason-input" type="text" maxlength="200" aria-label="验证信息" style="width:100%;margin-top:14px">
      <p id="contact-reason-error" role="alert"></p><div class="close-dialog-actions"><button class="outline" data-cancel>取消</button><button class="outline" data-submit>发送申请</button></div></section>`;
    document.body.append(backdrop);
    const input = backdrop.querySelector<HTMLInputElement>('#contact-reason-input')!;
    const close = (value: string | null): void => { document.removeEventListener('qbird:lock', onLock); backdrop.remove(); resolve(value); };
    const onLock = (): void => close(null);
    document.addEventListener('qbird:lock', onLock);
    backdrop.querySelector<HTMLButtonElement>('[data-cancel]')!.onclick = () => close(null);
    const submit = (): void => {
      const value = input.value.trim();
      if (!value || new TextEncoder().encode(value).length > 600 || /[\u0000-\u001f\u007f]/.test(value)) {
        backdrop.querySelector<HTMLElement>('#contact-reason-error')!.textContent = t('请输入有效的验证信息，最长 600 字节。'); return;
      }
      close(value);
    };
    backdrop.querySelector<HTMLButtonElement>('[data-submit]')!.onclick = submit;
    input.addEventListener('keydown', (event) => { if (event.key === 'Enter') submit(); if (event.key === 'Escape') close(null); });
    input.focus();
  });
}

async function addContactWithVerification(args: AddContactArgs): Promise<'accepted' | 'pending' | 'cancelled'> {
  let result = await invoke<string>('v1_add_contact', { ...args, reason: null });
  if (result === 'verify_required') {
    const reason = await askContactReason();
    if (reason === null) return 'cancelled';
    result = await invoke<string>('v1_add_contact', { ...args, reason });
  }
  if (result === 'accepted' || result === 'pending') return result;
  throw new Error(t('对方设置已变化，请重试添加。'));
}

async function addLanPerson(button: HTMLButtonElement): Promise<void> {
  if (!isTauri() || lanAddBusy) return;
  const { address, fingerprint, name, username } = button.dataset;
  if (!address || !fingerprint) return;
  lanAddBusy = fingerprint;
  lanNotice = '';
  paintLan();
  try {
    const args = {
      address, fingerprint, displayName: name || `Qbird ${fingerprint.slice(0, 8)}`, username: username || null,
      senderNickname: onboarding && !onboarding.done ? onboardingDraft.nickname : null,
      senderUsername: onboarding && !onboarding.done ? onboardingDraft.username : null,
    };
    const result = await addContactWithVerification(args);
    if (result === 'accepted') {
      const person = lanPeople.find((item) => item.fingerprint === fingerprint);
      if (person) { person.known = true; person.verified = true; }
      lanNotice = `已添加「${name || 'Qbird 用户'}」。`;
    } else if (result === 'pending') lanNotice = t('好友申请已发送，等待对方确认。');
    else lanNotice = t('已取消好友申请。');
  } catch (error) {
    lanNotice = displayError(error);
  } finally {
    lanAddBusy = '';
    paintLan();
    void refreshContacts();
  }
}

async function pickOnboardingDir(): Promise<void> {
  if (!isTauri()) return;
  const picked = await openFileDialog({ directory: true, multiple: false, title: t('选择接收文件保存位置') }).catch(() => null);
  if (!picked || typeof picked !== 'string') return;
  onboardingDraft.recvDir = picked;
  paintOnboarding();
}

async function toggleLanListener(start: boolean): Promise<void> {
  if (!isTauri()) return;
  lanNotice = start ? t('正在启动监听…') : t('正在停止监听…');
  paintLan();
  try {
    if (start) await invoke('v1_start_chat_service', { port: networkSettings.default_port });
    else await invoke('v1_stop_probe_service');
    await refreshProbeStatus();
    lanNotice = start ? t('已开始监听，同事可以发现你。') : t('已停止监听，本次运行不再自动重启；待发消息仍保存在本机。');
  } catch (error) {
    lanNotice = displayError(error);
  }
  paintLan();
}

async function addManualLan(): Promise<void> {
  const address = app.querySelector<HTMLInputElement>('#lan-manual')?.value.trim() || '';
  const name = app.querySelector<HTMLInputElement>('#lan-manual-name')?.value.trim() || '';
  if (!address) { lanNotice = t('请输入对方的 IP 和端口，例如 192.168.2.8:42853'); paintLan(); return; }
  lanNotice = t('正在探测该地址…');
  paintLan();
  try {
    const fingerprint = await invoke<string>('v1_probe_peer', { address });
    const result = await addContactWithVerification({ address, fingerprint, displayName: name || `Qbird ${fingerprint.slice(0, 8)}`, username: null, senderNickname: null, senderUsername: null });
    lanNotice = result === 'accepted' ? `已添加 ${name || address} 到联系人。` : result === 'pending' ? t('好友申请已发送，等待对方确认。') : t('已取消好友申请。');
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
    message.textContent = t('请选择探测结果，填写名称和独立渠道取得的完整匹配指纹，并勾选确认。'); return;
  }
  const button = app.querySelector<HTMLButtonElement>('button[data-action="confirm-contact"]');
  if (button?.disabled) return;
  if (button) button.disabled = true;
  try {
    await invoke('v1_confirm_contact', { fingerprint: selectedFingerprint, checkedFingerprint: checked, displayName: name });
    message.textContent = t('联系人已在本机确认；对方仍须单独确认你的指纹。');
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
    if (status) status.textContent = t('备注与分组已保存（仅本机可见，不影响信任）。');
    await refreshContacts();
  } catch (error) { if (status) status.textContent = trError(error, t('无法保存备注或分组')); }
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
  catch { pluginNotice = t('无法读取插件列表'); }
}

async function installPlugin(): Promise<void> {
  if (!isTauri()) return;
  const picked = await openFileDialog({ directory: true, multiple: false, title: t('选择包含 plugin.json 的插件文件夹') }).catch(() => null);
  if (!picked || typeof picked !== 'string') return;
  pluginNotice = t('正在安装…');
  try {
    const view = await invoke<PluginView>('v1_install_plugin', { path: picked });
    pluginNotice = `已安装「${view.name}」，可预览：${view.extensions.join('、') || t('无')}`;
  } catch (error) { pluginNotice = displayError(error); }
  await refreshPlugins();
  render();
}

async function togglePlugin(id: string, enabled: boolean): Promise<void> {
  if (!isTauri()) return;
  try {
    await invoke('v1_set_plugin_enabled', { id, enabled });
    pluginNotice = enabled ? t('插件已启用。') : t('插件已停用。');
  } catch (error) { pluginNotice = displayError(error); }
  await refreshPlugins();
  render();
}

/// Two clicks: the first arms the removal so a stray click cannot delete a plugin.
async function removePlugin(id: string): Promise<void> {
  if (!isTauri()) return;
  if (pluginRemovePending !== id) {
    pluginRemovePending = id;
    pluginNotice = t('再次点击“确认移除”会删除该插件目录。');
    render();
    return;
  }
  pluginRemovePending = '';
  try {
    await invoke('v1_remove_plugin', { id });
    pluginNotice = t('插件已移除。');
  } catch (error) { pluginNotice = displayError(error); }
  await refreshPlugins();
  render();
}

async function refreshHover(): Promise<void> {
  if (!isTauri()) return;
  try { hover = await invoke<{ show_ip: boolean; show_mac: boolean }>('v1_hover_settings'); }
  catch { hoverNotice = t('status.hoverReadErr'); }
}

async function saveHover(): Promise<void> {
  if (!isTauri()) return;
  const showIp = app.querySelector<HTMLInputElement>('#hover-show-ip')?.checked ?? true;
  const showMac = app.querySelector<HTMLInputElement>('#hover-show-mac')?.checked ?? false;
  try {
    hover = await invoke<{ show_ip: boolean; show_mac: boolean }>('v1_set_hover_settings', { showIp, showMac });
    hoverNotice = t('status.hoverSaved');
  } catch (error) { hoverNotice = displayError(error); }
  render();
}

async function refreshImageOpenMode(): Promise<void> {
  if (!isTauri()) return;
  try {
    const mode = await invoke<'lightbox' | 'workspace'>('v1_image_open_mode');
    applyImageOpenMode(mode);
  } catch { imageOpenNotice = t('status.imageModeReadErr'); }
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
    imageOpenNotice = mode === 'workspace' ? t('已改为在右侧工作区打开图片。') : t('已改为灯箱查看图片。');
  } catch (error) { imageOpenNotice = displayError(error); }
  render();
}

async function refreshFileOpenMode(): Promise<void> {
  if (!isTauri()) return;
  try {
    applyFileOpenMode(await invoke<'workspace' | 'system'>('v1_file_open_mode'));
  } catch { fileOpenNotice = t('status.fileModeReadErr'); }
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
    fileOpenNotice = mode === 'system' ? t('已改为用默认程序打开附件。') : t('已改为在右侧工作区预览附件。');
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
  catch { teamNotice = t('status.teamReadErr'); return; }
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
    teamNotice = team.name ? t('团队名称已保存。') : t('已恢复默认名称。');
  } catch (error) { teamNotice = displayError(error); }
  render();
}

async function chooseTeamLogo(): Promise<void> {
  if (!isTauri()) return;
  try {
    const picked = await openFileDialog({ multiple: false, filters: [{ name: t('图片'), extensions: ['png', 'jpg', 'jpeg'] }] });
    if (!picked || typeof picked !== 'string') return;
    const source = new Uint8Array(await invoke<number[]>('v1_read_image_file', { path: picked }));
    const cropped = await cropSquare(source, 256);
    await invoke('v1_set_team_logo', { bytes: Array.from(cropped) });
    team.has_logo = true; setTeamLogo(cropped); teamNotice = t('团队 Logo 已保存。');
  } catch (error) { teamNotice = displayError(error); }
  render();
}

async function clearTeamLogo(): Promise<void> {
  if (!isTauri()) return;
  try {
    await invoke('v1_clear_team_logo');
    team.has_logo = false; setTeamLogo(null); teamNotice = t('团队 Logo 已移除。');
  } catch (error) { teamNotice = displayError(error); }
  render();
}

/// Owner-only; the crop keeps the PNG small enough for the 96 KiB wire and storage cap.
async function chooseGroupAvatar(): Promise<void> {
  if (!isTauri() || !groupChat.groupId) return;
  try {
    const picked = await openFileDialog({ multiple: false, filters: [{ name: t('图片'), extensions: ['png', 'jpg', 'jpeg'] }] });
    if (!picked || typeof picked !== 'string') return;
    const source = new Uint8Array(await invoke<number[]>('v1_read_image_file', { path: picked }));
    const cropped = await cropAvatarInteractively(source, {
      title: t('调整群头像'),
      hint: t('拖动图片调整位置，滚轮或滑块缩放；群头像会压缩到 96 KiB 以内再发送。'),
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
  if (!query) { groupSearchStatus = t('请输入群名关键词'); groupSearchHits = []; paintGroupSearch(); return; }
  groupSearchBusy = true; groupSearchHits = []; groupSearchStatus = t('正在向已确认联系人查询…'); paintGroupSearch();
  try {
    groupSearchHits = await invoke<GroupSearchHit[]>('v1_search_groups', { query });
    groupSearchStatus = groupSearchHits.length ? `找到 ${groupSearchHits.length} 个群` : t('没有找到匹配的群；需要联系人同时在线。');
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
  catch { joinNotice = t('无法读取入群申请'); }
  paintJoinRequests();
}

async function resolveJoin(id: string, accept: boolean): Promise<void> {
  if (!isTauri()) return;
  try {
    await invoke('v1_resolve_join_request', { id, accept });
    joinNotice = accept ? t('已同意加入，正在通知群成员。') : t('已拒绝该申请。');
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
    box.append(heading(t('待处理的入群申请')));
    for (const request of joinRequests.incoming) {
      const row = document.createElement('div'); row.className = 'offer-card';
      const text = document.createElement('div'); text.className = 'offer-text';
      const title = document.createElement('div'); title.className = 'file-title';
      title.textContent = `${request.applicant_name || request.applicant.slice(0, 12)} 申请加入「${request.group_name}」`;
      const meta = document.createElement('small'); meta.className = 'file-meta'; meta.textContent = new Date(request.created_at * 1000).toLocaleString();
      text.append(title, meta);
      const actions = document.createElement('div'); actions.className = 'file-actions';
      const accept = document.createElement('button'); accept.className = 'outline'; accept.textContent = t('同意');
      accept.dataset.action = 'accept-join'; accept.dataset.id = request.id;
      const reject = document.createElement('button'); reject.className = 'linkish'; reject.textContent = t('拒绝');
      reject.dataset.action = 'reject-join'; reject.dataset.id = request.id;
      actions.append(accept, reject); row.append(text, actions); box.append(row);
    }
  }
  if (joinRequests.outgoing.length) {
    box.append(heading(t('我提交的申请')));
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
    const button = document.createElement('button'); button.className = 'outline'; button.textContent = t('申请加入');
    button.dataset.action = 'join-group';
    button.dataset.group = hit.group_id; button.dataset.name = hit.name; button.dataset.owner = hit.owner;
    button.disabled = !hit.owner_known;
    button.title = hit.owner_known ? t('向群主发送加入申请') : t('群主不在本机已确认联系人中，无法直接申请');
    actions.append(button); item.append(actions); list.append(item);
  }
}


/* ---------- contacts page: sidebar sections + card lists ---------- */

function presenceDot(fingerprint: string, label = true): string {
  const online = presence.get(fingerprint) === true;
  return `<span class="presence-dot ${online ? 'online' : ''}" title="${online ? t('在线：局域网可见或地址可连接') : t('离线：当前不可达')}" aria-label="${online ? t('在线') : t('离线')}"></span>`
    + (label ? `<span class="presence-text">${online ? t('在线') : t('离线')}</span>` : '');
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
  const requests = incomingRequests.length
    ? `<div class="side-section" id="side-requests">
      <div class="side-section-head"><span>待处理申请</span><small>${incomingRequests.length}</small></div>
      <ul class="side-list">${incomingRequests.map((request) => `<li class="side-row request-row" data-id="${request.id}">
        <span class="av av-32">${escapeText(contactInitial(request.nickname || request.peer.slice(0, 4)))}</span>
        <span class="side-row-body"><strong>${escapeText(request.nickname || request.peer.slice(0, 8))}</strong><small>${escapeText(request.reason || t('请求添加好友'))}</small></span>
        <span class="request-actions"><button class="outline" data-action="accept-request" data-id="${request.id}">接受</button><button class="linkish" data-action="reject-request" data-id="${request.id}">拒绝</button></span>
      </li>`).join('')}</ul>
    </div>`
    : '';
  return `${requests}<div class="side-section">
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
  return `<li class="contact-card contact-row" draggable="true" data-fingerprint="${escapeText(contact.fingerprint)}" data-name="${escapeText(contact.display_name)}" data-trust="${escapeText(contact.trust_state)}" title="${escapeText(t('拖到「群聊」里的群可邀请入群'))}">
    <span class="av av-44">${escapeText(contactInitial(name))}</span>
    <div class="contact-card-body">
      <div class="contact-card-title"><strong>${escapeText(name)}</strong>${verified ? presenceDot(contact.fingerprint) : ''}<span class="chip ${verified ? 'chip-ok' : 'chip-warn'}">${verified ? t('已添加') : t('待确认')}</span></div>
      ${contact.remark && contact.remark !== contact.display_name ? `<div class="contact-card-sub">昵称 ${escapeText(contact.display_name)}</div>` : ''}
      <div class="contact-card-sub">${escapeText(meta)}</div>
      <details class="contact-card-more"><summary>备注与分组</summary>
        <div class="probe-row">
          <input type="text" maxlength="64" placeholder="${escapeText(t('备注'))}" value="${escapeText(contact.remark ?? '')}" data-remark-for="${escapeText(contact.fingerprint)}" aria-label="${escapeText(t('联系人备注'))}">
          <input type="text" maxlength="64" placeholder="${escapeText(t('分组'))}" value="${escapeText(contact.category ?? '')}" data-category-for="${escapeText(contact.fingerprint)}" aria-label="${escapeText(t('联系人分组'))}">
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
      <div class="contact-card-sub">${group.member_count} 位成员 · ${escapeText(group.last_body || t('暂无消息'))}</div>
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
      <label for="group-name">群聊名称</label><input id="group-name" type="text" maxlength="128" placeholder="${escapeText(t('例如：项目组'))}">
      <div id="group-members-pick" class="group-pick"></div>
      <button class="outline" data-action="create-group">创建群聊</button>
      <p id="group-create-status" aria-live="polite"></p>
      <small>只能邀请已确认联系人；新成员只能看到加入之后的消息。也可以把左侧好友拖到群卡片上邀请。</small>
    </div>
    <div class="probe-box"><h3>加入与设置</h3>
      <label><input id="auto-join-groups" type="checkbox" ${autoJoinGroups ? 'checked' : ''}> 被拉进群时自动加入</label>
      <div id="join-requests"></div>
      <label for="group-search-name">按群名搜索并申请加入</label>
      <div class="probe-row"><input id="group-search-name" type="text" maxlength="64" placeholder="${escapeText(t('群名关键词'))}"><button class="outline" data-action="search-groups" ${groupSearchBusy ? 'disabled' : ''}>搜索</button></div>
      <p id="group-search-status" role="status">${escapeText(groupSearchStatus)}</p>
      <ul id="group-search-results" class="discovery-results"></ul>
      <small>只会向已确认且有地址的联系人查询；申请直接发给群主。对方离线时不保证送达。</small>
    </div>
    <div class="probe-box"><h3>已退出的群聊</h3>
      <p class="hint">退出后消息记录仍保留在本机，可随时查看或导出。</p>
      <ul id="left-groups-list" class="discovery-results"></ul>
    </div>
    <div class="probe-box"><h3>无效群（已解散）</h3>
      <p class="hint">群主解散后成员都会被移出，本机记录保留；只有你主动删除才会清空，删除会同时删除已下载的群文件。</p>
      <ul id="dissolved-groups-list" class="discovery-results"></ul>
    </div>
  </section>`;
}

function contactsPageView(): string {
  const verified = contactsCache.filter((contact) => contact.trust_state === 'verified').length;
  return `<section class="contacts-page">
    <header class="contacts-head">
      <div>
        <span class="eyebrow">CONTACTS / 通讯录</span>
        <h2>通讯录</h2>
        <p>联系人保存在本机；添加对方需要对方同意，双方确认后才能互发消息。</p>
      </div>
      <div class="segmented" role="tablist" aria-label="${escapeText(t('联系人与群聊'))}">
        <button role="tab" class="${contactTab === 'direct' ? 'active' : ''}" data-action="contact-tab" data-tab="direct" aria-selected="${contactTab === 'direct'}">单聊 · ${verified}</button>
        <button role="tab" class="${contactTab === 'group' ? 'active' : ''}" data-action="contact-tab" data-tab="group" aria-selected="${contactTab === 'group'}">群聊 · ${groups.length}</button>
      </div>
    </header>
    <div class="contacts-body" id="contacts-body">
    ${contactTab === 'direct' ? `<div class="contacts-toolbar">
        <input id="contact-search" type="search" maxlength="64" placeholder="${escapeText(t('搜索用户名、昵称或备注'))}" value="${escapeText(contactQuery)}" aria-label="${escapeText(t('搜索联系人'))}">
        <button class="outline" data-action="contacts-add">添加联系人</button>
      </div>
      <p id="contacts-status" role="status" aria-live="polite"></p>
      <ul id="contacts-list" class="contact-cards"></ul>`
    : `<p id="contacts-status" role="status" aria-live="polite"></p>${groupTabView()}`}
    </div>
  </section>`;
}

/// The contacts head is fixed and the list scrolls under it; show a hairline only once
/// something is actually hidden behind it, so the card keeps its quiet resting look.
function watchContactsScroll(): void {
  const body = app.querySelector<HTMLElement>('#contacts-body');
  const head = app.querySelector<HTMLElement>('.contacts-head');
  if (!body || !head) return;
  const sync = (): void => { head.classList.toggle('stuck', body.scrollTop > 2); };
  body.addEventListener('scroll', sync, { passive: true });
  sync();
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
    if (status) status.textContent = rows.length ? `匹配 ${rows.length} 位联系人` : t('没有匹配的联系人');
  }
  if (!rows.length) {
    list.innerHTML = `<li class="contact-card-empty">${query ? t('没有匹配的联系人。') : t('还没有联系人，点右上角「添加联系人」去局域网用户页添加。')}</li>`;
    return;
  }
  list.innerHTML = rows.map(contactCardRow).join('');
  for (const card of list.querySelectorAll<HTMLElement>('.contact-card[data-fingerprint]')) {
    card.addEventListener('contextmenu', (event) => {
      event.preventDefault();
      const contact = contactsCache.find((c) => c.fingerprint === card.dataset.fingerprint);
      if (!contact) return;
      const name = contact.remark || contact.display_name;
      const items: ContextItem[] = [
        { label: t('发消息'), run: () => { page = 'messages'; render(); void direct.open(contact.fingerprint, name); } },
        { label: t('删除联系人'), danger: true, run: () => { void removeContact(contact, name); } },
      ];
      showContextMenu(event, items);
    });
  }
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
  dot.title = online ? t('在线：局域网可见或地址可连接') : t('离线：当前不可达');
  const text = app.querySelector<HTMLElement>('#direct-presence-text');
  if (text) text.textContent = online ? t('在线') : t('离线');
}

async function acceptContact(peer: string): Promise<void> {
  try {
    await invoke('v1_accept_contact', { fingerprint: peer });
    contactNotice = t('已接受该联系人，双方现在可以互发消息。');
  } catch (error) { contactNotice = displayError(error); }
  await refreshContacts();
}

async function declineContact(peer: string): Promise<void> {
  if (!window.confirm(t('删除这条待确认的联系人请求？对方之后仍可再次添加你。'))) return;
  try {
    await invoke('v1_decline_contact', { fingerprint: peer });
    contactNotice = t('已移除待确认请求。');
  } catch (error) { contactNotice = displayError(error); }
  await refreshContacts();
}

/// Removes a confirmed contact from the address book; local chat history stays.
async function removeContact(contact: Contact, label: string): Promise<void> {
  if (!isTauri()) return;
  if (!window.confirm(`${t('删除联系人')}「${label}」？聊天记录会保留在本机。`)) return;
  try {
    await invoke('v1_remove_contact', { fingerprint: contact.fingerprint });
    contactNotice = `${t('已删除')}「${label}」`;
    await refreshContacts();
  } catch (error) { contactNotice = displayError(error); }
  if (page === 'contacts') render();
}

/// Leaves a group from the address book card context menu.
async function leaveGroupFromCard(group: Group): Promise<void> {
  if (!window.confirm(`${t('确定退出群聊')}「${group.name}」？历史记录会保留在本机。`)) return;
  try {
    await invoke('v1_leave_group', { groupId: group.id });
    contactNotice = `${t('已退出')}「${group.name}」`;
    await refreshContacts();
  } catch (error) { contactNotice = displayError(error); }
  if (page === 'contacts') render();
}

async function refreshContacts(): Promise<void> {
  if (!isTauri()) {
    const status = app.querySelector<HTMLElement>('#contacts-status');
    if (status) status.textContent = t('仅桌面应用支持联系人');
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
        : t('暂无联系人，可前往「局域网用户」添加。'));
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
    if (status) status.textContent = t('无法读取联系人');
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

const deferredContactRequests = new Set<string>();
let contactRequestDialogOpen = false;
let contactRequestsRefreshing = false;
let contactRequestsSyncing = false;

function showContactRequest(request: ContactRequest): void {
  contactRequestDialogOpen = true;
  const backdrop = document.createElement('div');
  backdrop.className = 'close-dialog-backdrop';
  backdrop.innerHTML = `<section class="close-dialog" role="dialog" aria-modal="true" aria-labelledby="incoming-contact-title">
    <h2 id="incoming-contact-title">好友申请</h2><p class="request-name"></p><p class="request-reason" style="white-space:pre-wrap;overflow-wrap:anywhere;margin-top:10px"></p>
    <p class="request-error" role="alert"></p><div class="close-dialog-actions"><button class="outline" data-later>稍后处理</button><button class="outline" data-reject>拒绝</button><button class="outline" data-accept>同意</button></div></section>`;
  backdrop.querySelector<HTMLElement>('.request-name')!.textContent = `${request.nickname || request.peer.slice(0, 8)}${request.username ? ' @' + request.username : ''} 请求添加你为好友`;
  backdrop.querySelector<HTMLElement>('.request-reason')!.textContent = `验证信息：${request.reason || ''}`;
  const close = (): void => { document.removeEventListener('qbird:lock', close); backdrop.remove(); contactRequestDialogOpen = false; };
  document.addEventListener('qbird:lock', close);
  backdrop.querySelector<HTMLButtonElement>('[data-later]')!.onclick = () => { deferredContactRequests.add(request.id); close(); };
  const respond = async (accept: boolean): Promise<void> => {
    for (const button of backdrop.querySelectorAll<HTMLButtonElement>('button')) button.disabled = true;
    try {
      await invoke('v1_respond_contact_request', { id: request.id, accept });
      close();
      await refreshContacts();
      await refreshContactRequests();
    } catch (error) {
      backdrop.querySelector<HTMLElement>('.request-error')!.textContent = displayError(error);
      for (const button of backdrop.querySelectorAll<HTMLButtonElement>('button')) button.disabled = false;
    }
  };
  backdrop.querySelector<HTMLButtonElement>('[data-reject]')!.onclick = () => { void respond(false); };
  backdrop.querySelector<HTMLButtonElement>('[data-accept]')!.onclick = () => { void respond(true); };
  document.body.append(backdrop);
  backdrop.querySelector<HTMLButtonElement>('[data-accept]')!.focus();
}

async function refreshContactRequests(): Promise<void> {
  if (!isTauri()) return;
  if (contactRequestsRefreshing) return;
  contactRequestsRefreshing = true;
  try {
    const [incoming, outgoing] = await Promise.all([
      invoke<ContactRequest[]>('v1_contact_requests', { direction: 'in' }),
      invoke<ContactRequest[]>('v1_contact_requests', { direction: 'out' }),
    ]);
    const pendingIn = incoming.filter((item) => item.status === 'pending');
    const pendingOut = outgoing.filter((item) => item.status === 'pending');
    if (JSON.stringify(pendingIn) !== JSON.stringify(incomingRequests)) {
      incomingRequests = pendingIn;
      if (page === 'contacts') render();
    }
    outgoingRequests = pendingOut;
    const next = pendingIn.find((item) => !deferredContactRequests.has(item.id));
    if (next && !contactRequestDialogOpen && lockInitialized && !lockState.locked) showContactRequest(next);
    if (pendingOut.length && !contactRequestsSyncing) {
      contactRequestsSyncing = true;
      void invoke<boolean>('v1_sync_contact_requests').then((changed) => {
        if (changed) { void refreshContacts(); void refreshContactRequests(); }
      }).catch(() => undefined).finally(() => { contactRequestsSyncing = false; });
    }
  } catch { /* keep the last list on a transient read failure */ }
  finally { contactRequestsRefreshing = false; }
}

async function refreshDissolvedGroups(): Promise<void> {
  if (!isTauri()) return;
  try {
    const list = await invoke<DissolvedGroup[]>('v1_list_dissolved_groups');
    if (JSON.stringify(list) !== JSON.stringify(dissolvedGroups)) {
      dissolvedGroups = list;
      paintDissolvedGroups();
    }
  } catch { /* keep the last list */ }
}

function paintDissolvedGroups(): void {
  const list = app.querySelector<HTMLUListElement>('#dissolved-groups-list');
  if (!list) return;
  list.replaceChildren();
  if (!dissolvedGroups.length) {
    const empty = document.createElement('li');
    empty.textContent = t('没有已解散的群聊。');
    list.append(empty);
    return;
  }
  for (const group of dissolvedGroups) {
    const item = document.createElement('li');
    const info = document.createElement('span');
    const when = group.dissolved_at ? new Date(group.dissolved_at * 1000).toLocaleDateString() : '';
    info.textContent = `${group.name} · 已解散${when ? ` · ${when}` : ''}`;
    const open = document.createElement('button');
    open.className = 'outline'; open.textContent = t('查看历史');
    open.dataset.action = 'open-dissolved-group';
    open.dataset.group = group.id;
    open.dataset.name = group.name;
    const remove = document.createElement('button');
    remove.className = 'linkish danger'; remove.textContent = t('删除记录');
    remove.dataset.action = 'delete-dissolved-group';
    remove.dataset.group = group.id;
    remove.dataset.name = group.name;
    item.append(info, open, remove);
    list.append(item);
  }
}

async function respondContactRequest(id: string, accept: boolean): Promise<void> {
  if (!isTauri()) return;
  try {
    await invoke('v1_respond_contact_request', { id, accept });
    requestNotice = accept ? t('已接受好友申请。') : t('已拒绝好友申请。');
    incomingRequests = incomingRequests.filter((item) => item.id !== id);
    if (page === 'contacts') paintContactsSidebar();
    await refreshContacts();
  } catch (error) {
    requestNotice = displayError(error);
  }
}

async function deleteDissolvedGroup(groupId: string, name: string): Promise<void> {
  if (!isTauri()) return;
  if (!window.confirm(`删除「${name}」的本地记录？消息与已下载的群文件都会被删除，无法恢复。`)) return;
  try {
    await invoke('v1_delete_group_history', { groupId });
    await refreshDissolvedGroups();
    if (page === 'contacts') paintLeftGroups();
  } catch (error) {
    const status = app.querySelector<HTMLElement>('#contacts-status');
    if (status) status.textContent = displayError(error);
  }
}

async function refreshPrivacyState(): Promise<void> {
  if (!isTauri()) return;
  try {
    lanVisible = await invoke<boolean>('v1_lan_visibility');
    const sync = await invoke<{ auto_accept: boolean }>('v1_contact_sync_settings');
    contactRequestMode = sync.auto_accept ? 'auto' : 'ask';
  } catch { /* keep defaults */ }
}

async function refreshNoticeState(): Promise<void> {
  if (!isTauri()) return;
  try {
    noticeSettings = await invoke<{ mode: NoticeMode; raise_window: boolean; preview: boolean }>('v1_notice_settings');
  } catch { /* keep defaults */ }
}

async function saveNoticeSettings(): Promise<void> {
  if (!isTauri()) return;
  const mode = (app.querySelector<HTMLSelectElement>('#notice-mode')?.value || 'popup') as NoticeMode;
  const raise = app.querySelector<HTMLInputElement>('#notice-raise')?.checked ?? false;
  const preview = app.querySelector<HTMLInputElement>('#notice-preview')?.checked ?? true;
  try {
    noticeSettings = await invoke('v1_set_notice_settings', { mode, raiseWindow: raise, preview });
    noticeNotice = t('status.noticeSaved');
  } catch (error) {
    noticeNotice = displayError(error);
  }
  if (page === 'settings') render();
}

async function saveContactPrivacy(): Promise<void> {
  if (!isTauri()) return;
  const mode = app.querySelector<HTMLSelectElement>('#contact-request-mode')?.value === 'auto' ? 'auto' : 'ask';
  const visible = app.querySelector<HTMLInputElement>('#lan-visible')?.checked ?? true;
  try {
    await invoke('v1_set_contact_sync', { autoAccept: mode === 'auto' });
    lanVisible = await invoke<boolean>('v1_set_lan_visibility', { visible });
    contactRequestMode = mode;
    privacyNotice = t('status.saved');
  } catch (error) {
    privacyNotice = displayError(error);
  }
  if (page === 'settings') render();
}

async function handleIncomingNotice(payload: { kind: 'direct' | 'group'; id: string; preview: string }): Promise<void> {
  if (noticeSettings.mode === 'off') return;
  if (document.hasFocus()) return;
  let name = payload.kind === 'group' ? t('群消息') : t('新的消息');
  if (payload.kind === 'direct') {
    const contact = contactsCache.find((item) => item.fingerprint === payload.id);
    if (contact) name = contact.remark || contact.display_name;
  } else {
    const group = groups.find((item) => item.id === payload.id);
    if (group) name = group.name;
  }
  void invoke('v1_flash_tray', { label: name }).catch(() => undefined);
  if (noticeSettings.mode === 'popup') {
    const preview = noticeSettings.preview ? payload.preview : t('发来一条消息');
    void invoke('v1_show_notice', { payload: { ...payload, name, preview } }).catch(() => undefined);
  }
  if (noticeSettings.raise_window) {
    const window = getCurrentWindow();
    await window.show().catch(() => undefined);
    await window.setFocus().catch(() => undefined);
  }
}

async function openFromNotice(payload: { kind: 'direct' | 'group'; id: string }): Promise<void> {
  if (payload.kind === 'direct') {
    const contact = contactsCache.find((item) => item.fingerprint === payload.id);
    direct.clear(); groupChat.clear(); selfOpen = false; page = 'messages';
    render();
    await direct.open(payload.id, contact?.remark || contact?.display_name || payload.id.slice(0, 8));
  } else {
    const group = groups.find((item) => item.id === payload.id);
    direct.clear(); selfOpen = false; page = 'messages';
    render();
    await groupChat.open(payload.id, group?.name || t('群聊'));
  }
}

function paintLeftGroups(): void {
  const list = app.querySelector<HTMLUListElement>('#left-groups-list');
  if (!list) return;
  list.replaceChildren();
  if (!leftGroups.length) {
    const empty = document.createElement('li');
    empty.textContent = t('没有已退出的群聊。');
    list.append(empty);
    return;
  }
  for (const group of leftGroups) {
    const item = document.createElement('li');
    const info = document.createElement('span');
    info.textContent = `${group.name} · ${group.member_count} 人 · 已退出`;
    const button = document.createElement('button');
    button.className = 'outline';
    button.textContent = t('查看历史');
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
    empty.textContent = t('没有待处理的邀请。');
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
    meta.textContent = `${invite.owner_name || t('群主')} 邀请你加入 · ${invite.member_count} 人`;
    text.append(title, meta);
    const accept = document.createElement('button');
    accept.className = 'outline';
    accept.textContent = t('同意');
    accept.dataset.action = 'accept-invite';
    accept.dataset.id = invite.id;
    const decline = document.createElement('button');
    decline.className = 'linkish';
    decline.textContent = t('拒绝');
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
    for (const card of list.querySelectorAll<HTMLElement>('[data-group-drop]')) {
      card.addEventListener('contextmenu', (event) => {
        event.preventDefault();
        const group = groups.find((g) => g.id === card.dataset.groupDrop);
        if (!group) return;
        showContextMenu(event, [
          { label: t('发消息'), run: () => { page = 'messages'; render(); void groupChat.open(group.id, group.name); } },
          { label: t('退出群聊'), danger: true, run: () => { void leaveGroupFromCard(group); } },
        ]);
      });
    }
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
    if (!verified.length) { pick.textContent = t('需要先在发现页面确认至少一位联系人。'); return; }
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
    if (status) status.textContent = t('已邀请该联系人入群，正在通知成员。');
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
  if (!name) { status.textContent = t('请输入群聊名称'); return; }
  if (!members.length) { status.textContent = t('请至少选择一位已确认联系人'); return; }
  try {
    const created = await invoke<Group>('v1_create_group', { name, members });
    input.value = '';
    for (const node of app.querySelectorAll<HTMLInputElement>('input[data-group-member]')) node.checked = false;
    status.textContent = t('群聊已创建；邀请会在对方在线时送达。');
    selfOpen = false; page = 'messages';
    await groupChat.open(created.id, created.name);
  } catch (error) { status.textContent = displayError(error); }
}

function render(): void {
  if (lockState.locked) { document.dispatchEvent(new Event('qbird:lock')); workspace.suspend(); app.innerHTML = lockView(); return; }
  workspace.resume();
  window.requestAnimationFrame(() => workspace.refreshLayout());
  if (!onboardingLoaded) { app.innerHTML = `<div class="boot">${t('boot.preparing')}</div>`; return; }
  if (isTauri() && onboarding && !onboarding.done) { paintOnboarding(); return; }
  const current = pages[page];
  app.innerHTML = `
    <div class="shell">
      <aside class="rail" aria-label="${t('nav.mainNav')}">
        <button class="profile" data-page="profile" aria-label="${t('nav.profile')}" title="${t('nav.profile')}">${avatarMarkup()}</button>
        <nav class="rail-nav" aria-label="${t('nav.primary')}">
          <button data-page="messages" class="${page === 'messages' ? 'active' : ''}" aria-label="${t('nav.messages')}" title="${t('nav.messages')}">${icon('messages')}</button>
          <button data-page="contacts" class="${page === 'contacts' ? 'active' : ''}" aria-label="${t('nav.contacts')}" title="${t('nav.contacts')}">${icon('contacts')}</button>
          <button data-page="discover" class="${page === 'discover' ? 'active' : ''}" aria-label="${t('nav.discover')}" title="${t('nav.discover')}">${icon('discover')}</button>
        </nav>
        <span class="rail-brand">Qbird</span>
        <button class="rail-bottom" data-action="lock-now" aria-label="${t('nav.lock')}" title="${t('nav.lock')}">${icon('lock')}</button>
        <button class="rail-bottom" data-action="menu" aria-label="${t('nav.more')}" aria-expanded="${menuOpen}" title="${t('nav.more')}">${icon('menu')}</button>
      </aside>
      ${menuOpen ? `<div class="menu" role="menu"><button data-page="settings" role="menuitem">${t('menu.settings')}</button><button data-page="settings" data-settings-tab="privacy" role="menuitem">${t('menu.chatBackup')}</button><span>${t('menu.userBackupTodo')}</span><span>${t('menu.userRestoreTodo')}</span><button data-page="settings" data-settings-tab="privacy" role="menuitem">${t('menu.chatRestore')}</button><button data-page="about" role="menuitem">${t('menu.about')}</button></div>` : ''}
      <aside class="sidebar">
        <div class="team"><span class="team-mark">${teamLogoUrl ? `<img class="team-logo" src="${escapeText(teamLogoUrl)}" alt="${escapeText(t('团队 Logo'))}">` : icon('spark')}</span><div><strong>${team.name ? escapeText(team.name) : 'Qbird'}</strong><small>LOCAL NETWORK MESSENGER</small></div></div>
        <div class="side-heading"><div><span class="eyebrow">${current.eyebrow}</span><h1>${current.title()}</h1></div></div>
        ${page === 'settings' || page === 'about' ? `<div class="settings-links">${SETTINGS_TABS.map((tab) => `<button data-settings-tab="${tab.id}" class="${page === 'settings' && settingsTab === tab.id ? 'selected' : ''}">${tab.label()}</button>`).join('')}<button data-page="about" class="${page === 'about' ? 'selected' : ''}">${t('menu.about')}</button></div>` : page === 'messages' ? `<div class="side-search"><input id="global-search" type="search" maxlength="200" placeholder="${t('side.searchAll')}" aria-label="${t('side.searchAll')}" value="${searchQuery.replace(/&/g, '&amp;').replace(/"/g, '&quot;').replace(/</g, '&lt;')}"><select id="search-conversation" aria-label="${t('搜索范围')}">${searchScopeOptionsHtml()}</select></div><div id="search-results" class="search-hits" hidden></div><div id="conversation-list" class="conversation-list">${t('side.reading')}</div>` : page === 'contacts' ? contactSidebarView() : `<div class="side-empty">${t('side.empty')}</div>`}
        <div class="side-foot"><span class="dot"></span>${t('side.localDesktop')}</div>
      </aside>
      <main class="workspace">
        ${page === 'messages' && groupChat.groupId ? groupChat.view() : page === 'messages' && direct.peer ? direct.view() : page === 'messages' && selfOpen ? selfChatView() : page === 'discover' ? discoveryView() : page === 'settings' ? settingsView() : page === 'profile' ? profileView() : page === 'contacts' ? contactsPageView() : page === 'about' ? `<section class="card about"><span class="eyebrow">${t('about.eyebrow')}</span><h2>${t('page.about.heading')}</h2><p>${t('about.tagline')}</p><dl><div><dt>${t('about.stage')}</dt><dd>${t('about.stageValue')} ${appVersion}</dd></div><div><dt>${t('about.license')}</dt><dd>Apache License 2.0</dd></div><div><dt>${t('about.platform')}</dt><dd>${t('about.platformValue')}</dd></div></dl></section>` : `<section class="empty"><div class="empty-mark">${icon('spark')}</div><h2>${current.heading()}</h2><p>${current.copy()}</p></section>`}
      </main>
    </div>`;
  if (page === 'contacts') watchContactsScroll();
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
  setText('#remote-fingerprint', remoteFingerprint || t('暂无'));
  setText('#remote-target', observedAddress);
  setText('#selected-fingerprint', selectedFingerprint || t('请先选择探测结果'));
  const requests = app.querySelector<HTMLUListElement>('#probe-requests');
  if (requests) {
    requests.replaceChildren();
    for (const request of probeStatus.requests) {
      const item = document.createElement('li');
      item.textContent = `${request.remote_ip} · ${request.fingerprint}（未确认）`;
      const button = document.createElement('button');
      button.className = 'outline'; button.textContent = t('选择此指纹核对');
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
    discoveryStatus = candidates.length ? `发现 ${candidates.length} 个候选服务` : t('暂未发现候选服务');
  } catch {
    candidates = [];
    discoveryStatus = t('局域网发现不可用，请检查网络和防火墙');
  } finally {
    discoveryLoading = false;
    if (page === 'discover') paintDiscovery();
  }
}

async function refreshProbeStatus(): Promise<void> {
  if (!isTauri() || page !== 'discover') return;
  try {
    probeStatus = await invoke<ProbeStatus>('v1_probe_status');
    probeNotice = probeStatus.running ? `聊天与探测监听中：TCP ${probeStatus.port}` : probeStatus.port !== null ? t('监听异常停止，请先停止再重试') : t('聊天与探测服务未启动');
  } catch { probeNotice = t('无法读取探测服务状态'); }
  if (page === 'discover') paintLan();
}

function lockView(): string {
  return `<div class="lock-screen"><section class="card lock-card" aria-label="${escapeText(t('应用已锁定'))}">
    <span class="eyebrow">LOCKED / 已锁定</span><h2>Qbird</h2>
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
  catch { lockNotice = t('status.lockReadErr2'); }
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
  } catch (error) { lockNotice = trError(error, t('解锁失败')); }
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
    lockNotice = next ? t('status.lockPwSaved') : t('status.lockPwRemoved');
  } catch (error) { lockNotice = trError(error, t('status.lockSaveErr')); }
  finally { lockBusy = false; if (page === 'settings') render(); }
}

async function saveLockIdle(): Promise<void> {
  if (!isTauri() || lockBusy) return;
  const input = app.querySelector<HTMLInputElement>('#lock-idle');
  const minutes = Number(input?.value ?? '0');
  if (!Number.isInteger(minutes) || minutes < 0 || minutes > 240) { lockNotice = t('status.idleRange'); if (page === 'settings') render(); return; }
  lockBusy = true;
  try {
    lockState = await invoke<LockStatus>('v1_set_lock_idle', { minutes });
    lockNotice = minutes ? `无操作 ${minutes} 分钟后将自动锁定。` : t('空闲自动锁定已关闭。');
  } catch (error) { lockNotice = trError(error, t('status.idleSaveErr')); }
  finally { lockBusy = false; if (page === 'settings') render(); }
}

function searchScopeOptionsHtml(): string {
  const options = exportTargets.map((item) => {
    const key = `${item.kind}|${item.target}`;
    const kindLabel = item.kind === 'self' ? t('自聊') : item.kind === 'direct' ? t('单聊') : t('群聊');
    return `<option value="${escapeText(key)}" ${searchScopeKey === key ? 'selected' : ''}>${escapeText(kindLabel)} · ${escapeText(item.name)}</option>`;
  }).join('');
  return `<option value="all" ${searchScopeKey === 'all' ? 'selected' : ''}>${t('全部会话')}</option>${options}`;
}

function scheduleSearch(): void {
  searchOffset = 0;
  searchHasMore = false;
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
  const [conversationKind, conversationId] = searchScopeKey === 'all' ? [null, null] : searchScopeKey.split('|');
  try {
    const hits = await invoke<SearchHit[]>('v1_search_all', {
      query, category: 'all', conversationKind, conversationId, limit: searchPageSize + 1, offset: searchOffset,
    });
    if (sequence === searchSeq) {
      searchHasMore = hits.length > searchPageSize;
      searchHits = hits.slice(0, searchPageSize);
    }
  } catch {
    if (sequence === searchSeq) { searchHits = []; searchError = t('搜索失败，请稍后重试'); }
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
  if (searchBusy) { results.textContent = t('正在搜索…'); return; }
  if (searchError) { results.textContent = searchError; return; }
  if (!searchHits.length) { results.textContent = t('没有匹配的消息'); return; }
  for (const hit of searchHits) {
    const button = document.createElement('button');
    button.className = 'search-hit';
    button.dataset.action = 'open-hit';
    button.dataset.kind = hit.conversation_kind;
    button.dataset.id = hit.conversation_id;
    button.dataset.message = hit.message_id;
    button.dataset.name = hit.conversation_name;
    const head = document.createElement('strong');
    const kindLabel = hit.conversation_kind === 'self' ? t('自聊') : hit.conversation_kind === 'direct' ? t('单聊') : t('群聊');
    head.textContent = `${kindLabel} · ${hit.conversation_name}`;
    const body = document.createElement('span');
    body.textContent = `${hit.sender_name || ''}：${hit.body}`;
    const time = document.createElement('small');
    time.textContent = new Date(hit.created_at * 1000).toLocaleString();
    button.append(head, body, time);
    results.append(button);
  }
  const pager = document.createElement('div');
  pager.className = 'search-pagination';
  const previous = document.createElement('button');
  previous.className = 'outline';
  previous.textContent = t('上一页');
  previous.disabled = searchOffset <= 0 || searchBusy;
  previous.addEventListener('click', () => { searchOffset = Math.max(0, searchOffset - searchPageSize); void runSearch(); });
  const pageLabel = document.createElement('span');
  pageLabel.textContent = t('side.searchPage', { n: Math.floor(searchOffset / searchPageSize) + 1 });
  const next = document.createElement('button');
  next.className = 'outline';
  next.textContent = t('下一页');
  next.disabled = !searchHasMore || searchBusy;
  next.addEventListener('click', () => { searchOffset += searchPageSize; void runSearch(); });
  pager.append(previous, pageLabel, next);
  results.append(pager);
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
    await groupChat.open(id, name || t('群聊'));
    await groupChat.jumpTo(messageId);
  }
}

async function jumpSelf(messageId: string): Promise<void> {
  const found = () => selfMessages.some(message => message.id === messageId);
  await loadUntilFound(found, () => selfHasOlder, () => selfMessages.length, () => loadSelfMessages(true));
  const node = app.querySelector<HTMLElement>(`#self-messages [data-id="${messageId}"]`);
  if (!node) { const status = app.querySelector<HTMLElement>('#self-error'); if (status) status.textContent = t('该消息暂不在本机记录中'); return; }
  node.scrollIntoView({ block: 'center' });
  node.classList.add('jump-target');
  window.setTimeout(() => node.classList.remove('jump-target'), 1600);
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
  const trust = row.dataset.trust === 'verified' ? t('已确认') : t('待确认');
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
        line.textContent = `IP：${info.ip ?? t('未记录地址')}`;
        tip.append(line);
      }
      if (hover.show_mac) {
        const mac = document.createElement('span');
        mac.textContent = `MAC：${info.mac ?? t('未获取（跨网段或本机 ARP 无记录）')}`;
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
  return trError(error, t('操作失败，请检查网络后重试'));
}

async function setProbeService(start: boolean): Promise<void> {
  if (!isTauri() || probeBusy) return;
  const port = Number(probePort);
  if (start && (!Number.isInteger(port) || port < 1 || port > 65535)) {
    probeMessage = t('请输入 1–65535 的端口'); paintDiscovery(); return;
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
  probeBusy = true; probeMessage = t('正在探测…'); paintDiscovery();
  try {
    const value = await invoke<string>('v1_probe_peer', { address: targetAddress });
    if (!/^[0-9a-f]{64}$/.test(value)) throw new Error('invalid fingerprint');
    remoteFingerprint = value; observedAddress = targetAddress;
    probeMessage = value === fingerprint ? t('该地址使用与本机相同的身份密钥') : t('请双方通过可靠渠道核对完整指纹；此结果尚未建立信任');
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
  if (target.id === 'global-search') {
    searchQuery = target.value;
    scheduleSearch();
    if (!exportTargetsLoaded) void loadExportTargets();
    return;
  }
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
  invitesNotice = accept ? t('正在加入…') : t('正在拒绝…');
  paintGroupInvitations();
  try {
    if (accept) {
      const groupId = await invoke<string | null>('v1_accept_group_invitation', { id });
      invitesNotice = groupId ? t('已加入群聊。') : t('邀请已失效或群身份不一致。');
      if (groupId) { await refreshContacts(); await groupChat.open(groupId, groupInvites.find((item) => item.id === id)?.name || t('群聊')); }
    } else {
      await invoke<boolean>('v1_decline_group_invitation', { id });
      invitesNotice = t('已拒绝邀请，群主会收到退出通知。');
    }
    if (accept) await refreshContacts();
  } catch (error) {
    invitesNotice = displayError(error);
  }
  paintGroupInvitations();
}

app.addEventListener('change', (event) => {
  const target = event.target;
  if (target instanceof HTMLSelectElement && target.id === 'app-language') { setLang(target.value as Lang); syncLangToBackend(getLang()); render(); return; }
  if (target instanceof HTMLSelectElement && target.id === 'search-conversation') {
    searchScopeKey = target.value;
    scheduleSearch();
    return;
  }
  if (target instanceof HTMLInputElement && target.dataset.exportTarget) {
    const key = target.dataset.exportTarget;
    if (target.checked) exportSelected.add(key); else exportSelected.delete(key);
    return;
  }
  if (target instanceof HTMLInputElement && target.id === 'export-select-all') {
    exportSelected.clear();
    if (target.checked) for (const item of exportTargets) exportSelected.add(exportKey(item));
    render();
    return;
  }
  if (target instanceof HTMLInputElement && target.id === 'export-include-text') {
    exportIncludeText = target.checked;
    return;
  }
  if (target instanceof HTMLInputElement && target.id === 'export-include-images') {
    exportIncludeImages = target.checked;
    return;
  }
  if (target instanceof HTMLInputElement && target.id === 'export-include-files') {
    exportIncludeFiles = target.checked;
    return;
  }
  if (target instanceof HTMLInputElement && target.id === 'chat-show-seconds') {
    const value = target.checked;
    setShowSeconds(value);
    void invoke('v1_set_chat_display', { showSeconds: value }).catch(() => undefined);
    render();
    return;
  }
  if (target instanceof HTMLInputElement && target.id === 'auto-accept-contacts') {
    void (async () => {
      try {
        const result = await invoke<{ auto_accept: boolean }>('v1_set_contact_sync', { autoAccept: target.checked });
        autoAcceptContacts = result.auto_accept;
        contactSyncNotice = autoAcceptContacts ? t('已开启：同事添加你时自动互相加入。') : t('已关闭：对方需填写验证信息，等待你确认。');
      } catch {
        target.checked = !target.checked;
        contactSyncNotice = t('无法保存联系人设置');
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
        invitesNotice = autoJoinGroups ? t('之后被拉进群会自动加入。') : t('之后被拉进群会先询问。');
      } catch {
        target.checked = !target.checked;
        invitesNotice = t('无法保存群设置');
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
    const groupId = action.dataset.group, name = action.dataset.name || t('群聊');
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
    // Each tab starts at the top so the switch never keeps the other tab's scroll offset.
    const body = app.querySelector<HTMLElement>('#contacts-body');
    if (body) body.scrollTop = 0;
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
  if (action?.dataset.action === 'debug-log-view') {
    logVisible = !logVisible;
    if (logVisible) { void invoke<string>('v1_read_logs').then((text) => { logText = text; paintDebugReports(); }).catch(() => paintDebugReports()); }
    else paintDebugReports();
    return;
  }
  if (action?.dataset.action === 'debug-log-dir') { void invoke('v1_open_logs_dir').catch((error) => workspace.notify(displayError(error))); return; }
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
      if (!onboardingDraft.nickname.trim()) { onboardingNotice = t('请先填写昵称'); paintOnboarding(); return; }
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
    const groupId = action.dataset.group, name = action.dataset.name || t('群聊');
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
  if (action?.dataset.action === 'save-notice-settings') { void saveNoticeSettings(); return; }
  if (action?.dataset.action === 'save-contact-privacy') { void saveContactPrivacy(); return; }
  if (action?.dataset.action === 'accept-request' && action.dataset.id) { void respondContactRequest(action.dataset.id, true); return; }
  if (action?.dataset.action === 'reject-request' && action.dataset.id) { void respondContactRequest(action.dataset.id, false); return; }
  if (action?.dataset.action === 'open-dissolved-group' && action.dataset.group) {
    const groupId = action.dataset.group, name = action.dataset.name || t('群聊');
    direct.clear(); selfOpen = false; page = 'messages';
    void groupChat.open(groupId, name, true, true).catch((error) => {
      page = 'contacts'; render();
      const status = app.querySelector<HTMLElement>('#contacts-status');
      if (status) status.textContent = displayError(error);
    });
    return;
  }
  if (action?.dataset.action === 'delete-dissolved-group' && action.dataset.group) {
    void deleteDissolvedGroup(action.dataset.group, action.dataset.name || t('该群'));
    return;
  }
  if (action?.dataset.action === 'toggle-group-members') { groupChat.toggleMembers(); return; }
  if (action?.dataset.action === 'group-notice-edit' || action?.dataset.action === 'group-notice-close') { groupChat.toggleNoticeEditor(); return; }
  if (action?.dataset.action === 'group-notice-save') {
    const input = app.querySelector<HTMLTextAreaElement>('#group-notice-input');
    if (input) void groupChat.saveNotice(input.value.trim());
    return;
  }
  if (action?.dataset.action === 'group-notice-clear') {
    if (window.confirm(t('确定清除群公告吗？所有成员的公告都会被清除。'))) void groupChat.saveNotice('');
    return;
  }
  if (action?.dataset.action === 'add-group-member') { void groupChat.addMember(); return; }
  if (action?.dataset.action === 'remove-group-member' && action.dataset.member) { void groupChat.removeMember(action.dataset.member); return; }
  if (action?.dataset.action === 'create-group') { void createGroup(); return; }
  if (action?.dataset.action === 'install-plugin') { void installPlugin(); return; }
  if (action?.dataset.action === 'remove-plugin' && action.dataset.id) { void removePlugin(action.dataset.id); return; }
  if (action?.dataset.action === 'save-hover-settings') { void saveHover(); return; }
  if (action?.dataset.action === 'save-image-open-mode') { void saveImageOpenMode(); return; }
  if (action?.dataset.action === 'save-file-open-mode') { void saveFileOpenMode(); return; }
  if (action?.dataset.action === 'group-preview-file' && action.dataset.id) { groupChat.openPreviewById(action.dataset.id); return; }
  if (action?.dataset.action === 'toggle-export-wizard') {
    exportWizardOpen = !exportWizardOpen;
    if (exportWizardOpen && !exportTargetsLoaded) void loadExportTargets();
    render();
    return;
  }
  if (action?.dataset.action === 'export-chat-archives') { void runExportWizard(); return; }
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
  if (action?.dataset.action === 'chat-menu') { openChatMenu(action as HTMLElement, event); return; }
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
if (isTauri()) void loadChatDisplay().then(() => render());
void refreshOnboarding();
// One-time notice after an install migrated legacy data, so old records are
// not mistaken for pre-seeded content.
if (isTauri()) {
  void invoke<string | null>('v1_migration_notice').then((source) => {
    if (source) workspace.notify(t('检测到本次安装从旧数据目录迁移了聊天数据，旧记录因此可见：') + source);
  }).catch(() => undefined);
}
// Prefer the language persisted on the Rust side (authoritative for the tray).
if (isTauri()) {
  void import('@tauri-apps/api/core').then(({ invoke }) => invoke<string>('v1_get_language'))
    .then((lang) => {
      if ((lang === 'zh-CN' || lang === 'zh-TW' || lang === 'en') && lang !== getLang()) { setLang(lang); render(); }
      else if (lang === 'zh-CN' && getLang() !== 'zh-CN') { syncLangToBackend(getLang()); }
      else { syncLangToBackend(getLang()); }
    })
    .catch(() => undefined);
}
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
app.addEventListener('conversations-changed', () => {
  void refreshConversations();
  // A synced announcement updates silently; refresh the open group's notice.
  if (groupChat.groupId) void groupChat.loadNotice();
});
window.setInterval(() => {
  if (page === 'contacts' || page === 'messages' || direct.peer) void refreshPresence();
}, 10_000);
window.setInterval(() => {
  if (page === 'discover') void refreshLanPeople();
  void refreshProbeStatus();
  if (page === 'messages') { void refreshConversations(); if (direct.peer) void direct.load(); if (groupChat.groupId) void groupChat.load(); }
  void refreshContactRequests();
  if (page === 'contacts') { void refreshGroupInvites(); void refreshContacts(); void refreshDissolvedGroups(); }
}, 4000);
if (isTauri()) {
  document.addEventListener('paste', (event) => { void handleImagePaste(event); });
  // A cropped capture lands in the composer staging area, never straight in the chat.
  void listen<string>('qbird://capture-done', async (event) => {
    const path = event.payload;
    if (direct.peer) await direct.stagePaths([path]);
    else if (groupChat.groupId) await groupChat.stagePaths([path]);
  }).catch(() => undefined);
  // Incoming-message notices: the backend only forwards, the frontend applies settings.
  void listen<{ kind: 'direct' | 'group'; id: string; preview: string }>('qbird://notice', (event) => {
    void handleIncomingNotice(event.payload);
  }).catch(() => undefined);
  void listen<{ kind: 'direct' | 'group'; id: string }>('qbird://open-chat', (event) => {
    void openFromNotice(event.payload);
  }).catch(() => undefined);
  void refreshNoticeState();
  void refreshPrivacyState();
  void refreshContactRequests();
  void refreshDissolvedGroups();
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
    fingerprint = /^[0-9a-f]{64}$/.test(value) ? value : t('无法读取');
    if (page === 'settings') render(); else if (page === 'discover') paintDiscovery();
  }).catch(() => {
    fingerprint = t('无法读取');
    if (page === 'settings') render(); else if (page === 'discover') paintDiscovery();
  });
}
