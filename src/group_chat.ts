import { invoke, isTauri } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { composerTools } from './ui_icons';
import { showImage } from './image_viewer';
import { base64ToBytes } from './plugin_view';
import { showContextMenu } from './context_menu';
import { workspace, type LoadedTab } from './workspace';
import { linkifiedText } from './linkify';
import { t, trError } from './i18n';
import { formatStamp } from './chat_prefs';
import { loadUntilFound } from './search_navigation';

type TextPreview = { name: string; kind: string; text: string };

type GroupMessage = {
  id: string; group_id: string; sender: string; sender_name: string; body: string;
  created_at: number; logical: number; pending: number;
  reply_to: string | null; reply_sender: string | null; reply_preview: string | null;
  mentions: string[];
  kind: string; file_name: string | null; file_size: number | null; file_sha256: string | null;
  file_status: string | null; file_transferred: number | null; file_error: string | null;
  recalled_at: number | null;
};
type GroupMember = { group_id: string; member: string; display_name: string; role: string; joined_at: number; active: boolean };
type Contact = { fingerprint: string; display_name: string; trust_state: string };
type Cursor = Pick<GroupMessage, 'logical' | 'sender' | 'id'>;
const cursorOf = (message: GroupMessage): Cursor => ({ logical: message.logical, sender: message.sender, id: message.id });
const compare = (a: Cursor, b: Cursor): number => a.logical - b.logical || (a.sender < b.sender ? -1 : a.sender > b.sender ? 1 : a.id < b.id ? -1 : a.id > b.id ? 1 : 0);
type SearchCategory = 'all' | 'text' | 'file' | 'image';
type Quoted = { id: string; name: string; preview: string };
type FileOffer = { message_id: string; peer: string; name: string; total: number; created_at: number; status: string; group_id?: string | null };
type Staged = { path: string; name: string; image: boolean; preview: string | null };

/// Group chat: outbound messages are queued per member by the backend, so the UI only
/// shows the pending count and never claims delivery on its own. Replies keep a short
/// local quote, and @ mentions are resolved against the active member list.
export class GroupChat {
  groupId = '';
  readOnly = false;
  private name = '';
  private owner = '';
  private isOwner = false;
  private dissolved = false;
  notice = '';
  noticeOpen = false;
  private self = '';
  private messages: GroupMessage[] = [];
  private members: GroupMember[] = [];
  private contacts: Contact[] = [];
  private hasOlder = false;
  private loading = false;
  private sending = false;
  private generation = 0;
  private drafts = new Map<string, string>();
  private refreshBefore: Cursor | null = null;
  private membersOpen = false;
  private quoted: Quoted | null = null;
  private mentionQuery: string | null = null;
  private mentioned = new Map<string, string>();
  private searchOpen = false;
  private searchQuery = '';
  private searchCategory: SearchCategory = 'all';
  private searchMember = '';
  private searchResults: GroupMessage[] = [];
  private searchBusy = false;
  private searchStatus = '';
  private thumbs = new Map<string, string>();
  private thumbPending = new Set<string>();
  private attaching = false;
  private avatarUrl: string | null = null;
  private avatarBytes: Uint8Array | null = null;
  private batches = new Map<string, string>();
  private batchCaptions = new Map<string, string>();
  private renderedBatches = new Set<string>();
  private offers: FileOffer[] = [];
  /// Attachments picked, dropped or pasted but not sent yet, so text and files
  /// leave together only when the user presses Send.
  private staged: Staged[] = [];
  /// 'lightbox' keeps the in-app viewer; 'workspace' opens the right panel instead.
  imageOpenMode: 'lightbox' | 'workspace' = 'lightbox';
  /// What clicking a file card does; the workspace previews it or the system app opens it.
  fileOpenMode: 'workspace' | 'system' = 'workspace';

  constructor(private root: HTMLElement, private onChange: () => void) {}

  view(): string {
    return `<section class="self-chat" aria-label="${t('群聊')}">
      <header class="chat-header"><div class="chat-identity"><span class="chat-avatar" id="group-avatar">${t('群')}</span><div><strong id="group-name"></strong><small id="group-subtitle">${t('群聊')}</small></div></div><div class="group-actions"><button class="outline" id="group-avatar-button" data-action="choose-group-avatar" hidden>${t('群头像')}</button><button class="outline" id="group-avatar-remove" data-action="clear-group-avatar" hidden>${t('移除头像')}</button><button class="outline chat-more" data-action="chat-menu" data-kind="group" aria-label="${t('会话菜单')}" title="${t('会话菜单')}">⋯</button><button class="outline" data-action="toggle-group-search">${t('搜索')}</button><button class="outline" data-action="toggle-group-members">${t('成员')}</button><button class="outline" id="group-leave" data-action="leave-group" ${this.readOnly ? 'hidden' : ''}>${this.isOwner ? t('解散群聊') : t('退出群聊')}</button></div></header>
      <div class="group-notice-bar" id="group-notice-bar" hidden><span class="notice-tag">${t('公告')}</span><span class="notice-text" id="group-notice-text"></span><button class="outline" data-action="group-notice-edit"></button></div>
      <div class="group-notice-editor" id="group-notice-editor" hidden><label for="group-notice-input">${t('群公告')}</label><textarea id="group-notice-input" rows="4" maxlength="2048" placeholder="${t('写下群公告，所有成员都会看到')}"></textarea><p id="group-notice-status" role="status"></p><div class="notice-actions"><button class="outline" data-action="group-notice-save">${t('保存')}</button><button class="outline danger" data-action="group-notice-clear">${t('清除公告')}</button><button class="outline" data-action="group-notice-close">${t('关闭')}</button></div></div>
      <div class="group-search" id="group-search" hidden>
        <div class="search-row">
          <input id="group-search-query" type="search" maxlength="200" placeholder="${t('搜索聊天记录')}" aria-label="${t('搜索聊天记录')}">
          <select id="group-search-category" aria-label="${t('搜索类别')}"><option value="all">${t('全部')}</option><option value="text">${t('文字')}</option><option value="file">${t('文件')}</option><option value="image">${t('图片')}</option></select>
          <select id="group-search-member" aria-label="${t('按成员筛选')}"><option value="">${t('全部成员')}</option></select>
          <button class="outline" data-action="group-search-run">${t('搜索')}</button>
          <button class="outline" data-action="group-search-close">${t('关闭')}</button>
        </div>
        <p id="group-search-status" role="status"></p>
        <ul id="group-search-results" class="search-results"></ul>
      </div>
      <div class="chat-history" id="group-history" aria-label="${t('群聊记录')}"><div class="group-members" id="group-members" hidden></div><button class="load-older" data-action="group-older" hidden>${t('加载更早消息')}</button><div id="group-offers" class="direct-offers" hidden></div><div id="group-messages"></div></div>
      <p class="readonly-note" id="group-readonly-note" ${this.readOnly ? '' : 'hidden'}>${this.dissolved ? t('这个群已解散（无效群），只能查看历史记录；记录会一直保留，除非主动删除。') : t('你已退出这个群，只能查看历史记录；记录会一直保留，除非主动删除。')}</p>
      <div class="chat-compose" ${this.readOnly ? 'hidden' : ''}>
        <div class="reply-bar" id="reply-bar" hidden><span id="reply-bar-text"></span><button class="outline" data-action="group-cancel-reply">${t('取消引用')}</button></div>
        <label for="group-input">${t('消息')}</label>
        <div class="compose-staging" id="group-staging" aria-label="${t('待发送附件')}" hidden></div>
        <textarea id="group-input" rows="3" maxlength="32768" placeholder="${t('发消息给群里的人…输入 @ 可选择成员')}"></textarea>
        <div class="mention-picker" id="mention-picker" hidden></div>
        ${composerTools('group', t('Enter 发送 · Shift+Enter 换行'))}
        <p id="group-file-status" role="status"></p>
        <p id="group-error" role="status"></p>
      </div>
    </section>`;
  }

  clear(): void {
    this.groupId = ''; this.readOnly = false; this.generation++; this.membersOpen = false; this.notice = ''; this.noticeOpen = false; this.quoted = null;
    this.setGroupAvatar(null);
    this.thumbs.clear(); this.thumbPending.clear(); this.offers = [];
    this.staged = []; this.paintStaging();
    this.mentionQuery = null; this.mentioned.clear(); this.closeSearch();
  }

  draft(value: string): void {
    if (!this.groupId) return;
    this.drafts.set(this.groupId, value);
    const match = /(?:^|\s)@([^\s@]{0,32})$/.exec(value);
    this.mentionQuery = match ? match[1] : null;
    this.paintMentionPicker();
  }

  async open(groupId: string, name: string, readOnly = false, dissolved = false): Promise<void> {
    if (!isTauri()) return;
    const generation = ++this.generation;
    if (readOnly) {
      const known = await invoke<boolean>('v1_open_group_history', { groupId });
      if (!known) throw new Error(t('本机没有这个群聊的记录'));
    } else {
      await invoke('v1_open_group', { groupId });
    }
    if (generation !== this.generation) return;
    this.readOnly = readOnly;
    this.dissolved = dissolved;
    this.groupId = groupId; this.name = name; this.messages = []; this.hasOlder = false;
    this.refreshBefore = null; this.loading = false; this.membersOpen = false; this.noticeOpen = false;
    this.quoted = null; this.mentionQuery = null; this.mentioned.clear(); this.closeSearch();
    this.onChange();
    if (!this.self) { try { this.self = await invoke<string>('identity_fingerprint'); } catch { this.self = ''; } }
    await Promise.all([this.load(false, true), this.loadMembers(), this.loadAvatar(), this.loadOffers()]);
    void this.loadNotice();
  }

  async load(older = false, initial = false): Promise<void> {
    if (!isTauri() || !this.groupId || this.loading) return;
    const groupId = this.groupId, generation = this.generation;
    const first = this.messages[0];
    const before: Cursor | null = older && first ? cursorOf(first) : null;
    if (older && !before) return;
    this.loading = true;
    try {
      const batch = await invoke<GroupMessage[]>('v1_list_group', { groupId, limit: 200, before });
      if (generation !== this.generation || groupId !== this.groupId) return;
      let historical: GroupMessage[] = [];
      if (!older && !initial && this.refreshBefore && first) {
        historical = await invoke<GroupMessage[]>('v1_list_group', { groupId, limit: 200, before: this.refreshBefore });
        if (generation !== this.generation || groupId !== this.groupId) return;
      }
      if (!older) {
        const tail = (historical.length ? historical : batch).at(-1);
        this.refreshBefore = first && tail && compare(first, tail) < 0 ? cursorOf(tail) : null;
      }
      const merged = new Map(this.messages.map(message => [message.id, message]));
      for (const message of [...batch, ...historical]) merged.set(message.id, message);
      const next = [...merged.values()].sort(compare);
      const changed = next.length !== this.messages.length || next.some((item, i) => item.id !== this.messages[i]?.id
        || item.pending !== this.messages[i]?.pending
        || item.file_status !== this.messages[i]?.file_status
        || item.file_transferred !== this.messages[i]?.file_transferred);
      this.messages = next;
      await this.loadBatches();
      if (older || initial) this.hasOlder = batch.length === 200;
      if (changed || older || initial) this.paint(initial ? 'bottom' : older ? 'preserve' : 'auto');
    } catch { if (generation === this.generation) this.status('#group-error', t('无法读取群聊记录')); }
    finally { if (generation === this.generation) this.loading = false; }
  }

  async loadMembers(): Promise<void> {
    if (!isTauri() || !this.groupId) return;
    const groupId = this.groupId, generation = this.generation;
    try {
      const [members, contacts] = await Promise.all([
        invoke<GroupMember[]>('v1_list_group_members', { groupId }),
        invoke<Contact[]>('v1_list_contacts'),
      ]);
      if (generation !== this.generation) return;
      this.members = members;
      this.contacts = contacts.filter(contact => contact.trust_state === 'verified');
      if (members.length) { this.owner = members.find(member => member.role === 'owner')?.member ?? this.owner; this.isOwner = this.owner === this.self; }
      const leave = this.root.querySelector<HTMLButtonElement>('#group-leave');
      if (leave) leave.textContent = this.isOwner ? t('解散群聊') : t('退出群聊');
      this.paintMembers();
      this.paintSubtitle();
      this.paintSearchMembers();
    } catch { this.status('#group-error', t('无法读取群成员')); }
  }

  notify(text: string): void { this.status('#group-error', text); }

  /// Group files waiting for a local yes/no after the sender's offer was deferred.
  private async loadOffers(): Promise<void> {
    if (!isTauri() || !this.groupId) return;
    const groupId = this.groupId, generation = this.generation;
    try {
      const offers = await invoke<FileOffer[]>('v1_group_file_offer_requests', { groupId });
      if (generation !== this.generation || groupId !== this.groupId) return;
      this.offers = offers;
      this.paintOffers();
    } catch { /* the group view still works without the pending list */ }
  }

  private paintOffers(): void {
    const box = this.root.querySelector<HTMLElement>('#group-offers');
    if (!box) return;
    box.replaceChildren();
    box.hidden = this.offers.length === 0;
    for (const offer of this.offers) {
      const card = document.createElement('div'); card.className = 'offer-card';
      const text = document.createElement('div'); text.className = 'offer-text';
      const title = document.createElement('div'); title.className = 'file-title'; title.textContent = offer.name;
      const meta = document.createElement('small'); meta.className = 'file-meta';
      meta.textContent = `${this.formatSize(offer.total)} · ${new Date(offer.created_at * 1000).toLocaleString()}${t('· 群文件等待确认')}`;
      text.append(title, meta);
      const actions = document.createElement('div'); actions.className = 'file-actions';
      const accept = document.createElement('button'); accept.className = 'outline'; accept.textContent = t('接收');
      accept.dataset.action = 'accept-group-file-offer'; accept.dataset.id = offer.message_id;
      const remember = document.createElement('button'); remember.className = 'outline'; remember.textContent = t('接收并不再询问');
      remember.dataset.action = 'accept-group-file-offer-remember'; remember.dataset.id = offer.message_id;
      const saveAs = document.createElement('button'); saveAs.className = 'outline'; saveAs.textContent = t('另存为…');
      saveAs.dataset.action = 'saveas-group-file-offer'; saveAs.dataset.id = offer.message_id;
      const reject = document.createElement('button'); reject.className = 'linkish'; reject.textContent = t('拒绝');
      reject.dataset.action = 'reject-group-file-offer'; reject.dataset.id = offer.message_id;
      actions.append(accept, remember, saveAs, reject);
      card.append(text, actions);
      box.append(card);
    }
  }

  async resolveOffer(id: string, accept: boolean, options: { dir?: string; remember?: boolean } = {}): Promise<void> {
    if (!isTauri() || !id || !this.groupId) return;
    try {
      if (options.remember) await invoke('v1_remember_file_source', { source: `group:${this.groupId}` });
      await invoke('v1_resolve_file_offer', { id, accept, dir: options.dir ?? null });
      this.offers = this.offers.filter(offer => offer.message_id !== id);
      this.paintOffers();
      this.status('#group-file-status', accept ? t('已接受，等待对方重新发送') : t('已拒绝该群文件'));
    } catch (error) { this.status('#group-error', trError(error, t('无法更新文件确认状态'))); }
  }

  async saveOfferAs(id: string): Promise<void> {
    if (!isTauri() || !id) return;
    try {
      const picked = await open({ directory: true, multiple: false, title: t('选择该群文件的保存位置') });
      if (typeof picked !== 'string') return;
      await this.resolveOffer(id, true, { dir: picked });
    } catch (error) { this.status('#group-error', trError(error, t('无法选择保存位置'))); }
  }

  async pick(kind: 'image' | 'file'): Promise<void> {
    if (!isTauri() || !this.groupId || this.attaching) return;
    this.attaching = true;
    try {
      const selected = await open({
        multiple: kind === 'image', directory: false,
        title: kind === 'image' ? t('选择图片') : t('选择文件'),
        filters: kind === 'image' ? [{ name: t('图片'), extensions: ['png', 'jpg', 'jpeg', 'gif', 'webp', 'bmp'] }] : undefined,
      });
      if (!selected) return;
      const paths = Array.isArray(selected) ? selected : [selected];
      await this.stagePaths(paths);
    } catch (error) { this.status('#group-error', trError(error, t('无法打开文件选择器'))); }
    finally { this.attaching = false; }
  }

  /// Puts picked, dropped or pasted files into the composer staging area so the
  /// user can review them and mix them with text before anything is queued.
  async stagePaths(paths: string[]): Promise<void> {
    if (!isTauri() || !this.groupId || !paths.length) return;
    this.attaching = true;
    this.status('#group-file-status', t('正在准备预览…'));
    try {
      for (const path of paths) {
        const name = path.split(/[\\/]/).pop() ?? path;
        const image = /\.(png|jpe?g|gif|webp|bmp)$/i.test(path);
        let preview: string | null = null;
        if (image) preview = await invoke<string>('v1_local_preview', { path }).catch(() => null);
        if (!this.staged.some((item) => item.path === path)) this.staged.push({ path, name, image, preview });
      }
      this.paintStaging();
      this.status('#group-file-status', this.staged.length
        ? `${t('已添加')}${this.staged.length}${t('个附件：可继续写文字，点「发送」一起发出。')}`
        : '');
    } finally { this.attaching = false; }
  }

  unstage(index: number): void {
    if (index < 0 || index >= this.staged.length) return;
    this.staged.splice(index, 1);
    this.paintStaging();
    this.status('#group-file-status', this.staged.length ? `${t('还剩')}${this.staged.length}${t('个附件')}` : t('已清空附件'));
  }

  private paintStaging(): void {
    const box = this.root.querySelector<HTMLElement>('#group-staging');
    if (!box) return;
    box.hidden = this.staged.length === 0;
    if (!this.staged.length) { box.innerHTML = ''; return; }
    box.innerHTML = this.staged.map((item, index) => item.image
      ? `<div class="staged-item is-image" title="${escapeHtml(item.name)}"><img src="${item.preview ?? ''}" alt=""><button type="button" class="staged-remove" data-action="group-unstage" data-index="${index}" aria-label="${t('移除')}${escapeHtml(item.name)}">×</button></div>`
      : `<div class="staged-item is-file" title="${escapeHtml(item.name)}"><span class="staged-name">${escapeHtml(item.name)}</span><button type="button" class="staged-remove" data-action="group-unstage" data-index="${index}" aria-label="${t('移除')}${escapeHtml(item.name)}">×</button></div>`
    ).join('') + '<span class="staged-hint">发送时随文字一起发出</span>';
  }

  /// One bubble for the selected images, using the composer text as caption.
  async queueImages(paths: string[]): Promise<void> {
    if (!this.groupId || this.attaching || !paths.length) return;
    const groupId = this.groupId, generation = this.generation;
    const input = this.root.querySelector<HTMLTextAreaElement>('#group-input');
    const caption = input?.value ?? '';
    this.attaching = true;
    try {
      const messages = await invoke<GroupMessage[]>('v1_queue_group_images', { groupId, paths, caption });
      if (generation !== this.generation) return;
      for (const message of messages) {
        if (!this.messages.some(item => item.id === message.id)) this.messages.push(message);
      }
      this.messages.sort(compare);
      if (input && caption) input.value = '';
      await this.loadBatches();
      this.paint('bottom');
      this.status('#group-file-status', `${t('已加入待发送：')}${messages.length}${t('张图片（成员在线后自动发送）')}`);
    } catch (error) { this.status('#group-error', trError(error, t('无法加入图片'))); }
    finally { this.attaching = false; }
  }

  private async loadBatches(): Promise<void> {
    if (!isTauri() || !this.groupId) return;
    try {
      const rows = await invoke<{ message_id: string; batch_id: string; caption: string }[]>('v1_image_batches', { kind: 'group', target: this.groupId });
      this.batches.clear(); this.batchCaptions.clear();
      for (const row of rows) { this.batches.set(row.message_id, row.batch_id); this.batchCaptions.set(row.batch_id, row.caption); }
    } catch { /* keep the previous grouping */ }
  }

  async queuePaths(paths: string[]): Promise<void> {
    // A drop or pick that is all images becomes one bubble, same as the image button.
    if (paths.length && paths.every(path => /\.(png|jpe?g|gif|webp|bmp)$/i.test(path))) { await this.queueImages(paths); return; }
    for (const path of paths) await this.queuePath(path);
  }

  async queuePath(path: string): Promise<void> {
    if (!this.groupId || this.attaching) return;
    const groupId = this.groupId, generation = this.generation;
    this.attaching = true;
    this.status('#group-file-status', t('正在计算校验值…'));
    try {
      const message = await invoke<GroupMessage>('v1_queue_group_file', { groupId, path });
      if (generation === this.generation) {
        if (!this.messages.some(item => item.id === message.id)) { this.messages.push(message); this.messages.sort(compare); }
        this.paint('bottom');
        this.status('#group-file-status', `${t('已加入待发送：')}${message.file_name ?? message.body}${t('（对方在线后自动传输）')}`);
        this.dispatchChanged();
      }
    } catch (error) {
      if (generation === this.generation) this.status('#group-file-status', trError(error, t('无法加入群文件')));
    } finally { this.attaching = false; }
  }

  private formatSize(bytes: number): string {
    if (bytes >= 1024 * 1024 * 1024) return `${(bytes / 1024 / 1024 / 1024).toFixed(2)} GB`;
    if (bytes >= 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
    if (bytes >= 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${bytes} B`;
  }

  private fileStatusText(message: GroupMessage): string {
    const total = message.file_size ?? 0;
    const done = message.file_transferred ?? 0;
    if (message.file_status === 'completed') return t('已完成');
    if (message.file_status === 'failed') return message.file_error ?? t('传输失败');
    if (message.file_status === 'transferring') {
      const percent = total > 0 ? Math.min(100, Math.round((done / total) * 100)) : 0;
      return `${t('传输中')}${percent}%`;
    }
    return t('等待传输');
  }

  private isImageMessage(message: GroupMessage): boolean {
    return message.kind === 'file' && /\.(png|jpe?g|gif|webp|bmp)$/i.test(message.file_name ?? '');
  }

  private async loadThumbnail(id: string): Promise<void> {
    if (this.thumbs.has(id) || this.thumbPending.has(id)) return;
    this.thumbPending.add(id);
    try {
      const preview = await invoke<{ name: string; data_url: string } | null>('v1_read_group_attachment', { groupId: this.groupId, messageId: id });
      if (preview) { this.thumbs.set(id, preview.data_url); this.paint('preserve'); }
    } catch { /* 预览失败时保留文件卡片 */ }
    finally { this.thumbPending.delete(id); }
  }

  async send(): Promise<void> {
    const input = this.root.querySelector<HTMLTextAreaElement>('#group-input');
    const button = this.root.querySelector<HTMLButtonElement>('button[data-action="group-send"]');
    if (!this.groupId || this.readOnly || !input || this.sending) return;
    const groupId = this.groupId, generation = this.generation, body = input.value;
    const staged = this.staged.slice();
    if (!this.members.some(member => member.member === this.self && member.active)) {
      this.status('#group-error', t('该群聊已解散，或你已不在群中')); return;
    }
    if (!body.trim() && !staged.length) { this.status('#group-error', t('请输入消息，或先添加图片/文件')); return; }
    const mentions = [...this.mentioned.entries()]
      .filter(([, name]) => body.includes(`@${name}`)).map(([member]) => member);
    this.sending = true; if (button) button.disabled = true; this.status('#group-error', '');
    try {
      if (staged.length) {
        const images = staged.filter((item) => item.image).map((item) => item.path);
        const files = staged.filter((item) => !item.image).map((item) => item.path);
        if (images.length) {
          const sent = await invoke<GroupMessage[]>('v1_queue_group_images', { groupId, paths: images, caption: body });
          for (const item of sent) { if (!this.messages.some(existing => existing.id === item.id)) this.messages.push(item); }
          this.messages.sort(compare);
          await this.loadBatches();
        }
        for (const path of files) await invoke<GroupMessage>('v1_queue_group_file', { groupId, path });
        // Plain files cannot carry a caption yet, so the typed text goes out separately.
        if (!images.length && body.trim()) {
          await invoke<GroupMessage>('v1_queue_group', { groupId, body, replyTo: this.quoted?.id ?? null, mentions });
        }
        this.staged = []; this.paintStaging();
        if (this.drafts.get(groupId) === body) this.drafts.set(groupId, '');
        if (generation === this.generation) {
          if (input.isConnected && input.value === body) input.value = '';
          this.quoted = null; this.mentioned.clear(); this.mentionQuery = null;
          await this.load(false, true);
          this.paint('bottom'); this.paintReplyBar(); this.paintMentionPicker(); this.dispatchChanged();
          this.status('#group-file-status', images.length
            ? `${t('已加入待发送：')}${images.length}${t('张图片')}${body.trim() ? t('与文字') : ''}${t('（成员在线后自动发送）')}`
            : `${t('已加入待发送：')}${files.length}${t('个文件')}${body.trim() ? '，文字已单独发送' : ''}${t('（成员在线后自动发送）')}`);
        }
        return;
      }
      const message = await invoke<GroupMessage>('v1_queue_group', {
        groupId, body, replyTo: this.quoted?.id ?? null, mentions,
      });
      if (this.drafts.get(groupId) === body) this.drafts.set(groupId, '');
      if (generation === this.generation) {
        if (input.isConnected && input.value === body) input.value = '';
        if (!this.messages.some(item => item.id === message.id)) { this.messages.push(message); this.messages.sort(compare); }
        this.quoted = null; this.mentioned.clear(); this.mentionQuery = null;
        this.paint('bottom'); this.paintReplyBar(); this.paintMentionPicker(); this.dispatchChanged();
      }
    } catch (error) {
      if (generation === this.generation) this.status('#group-error', trError(error, t('无法保存待发送消息')));
    } finally { this.sending = false; if (button?.isConnected) button.disabled = false; }
  }

  async hide(): Promise<void> {
    if (!this.groupId) return;
    const groupId = this.groupId, generation = this.generation;
    try {
      await invoke('v1_hide_group', { groupId });
      if (generation === this.generation) { this.clear(); this.onChange(); workspace.notify(t('已隐藏会话；收到新消息时会重新出现。')); }
    } catch { if (generation === this.generation) this.status('#group-error', t('无法隐藏会话')); }
  }

  async leave(): Promise<void> {
    if (!this.groupId) return;
    const groupId = this.groupId, generation = this.generation;
    if (this.isOwner) {
      if (!window.confirm(t('解散后所有成员都会被移出这个群，历史记录会保留为无效群。确定解散？'))) return;
      try {
        await invoke('v1_dissolve_group', { groupId });
        if (generation === this.generation) { this.clear(); this.onChange(); }
      } catch (error) { if (generation === this.generation) this.status('#group-error', trError(error, t('无法解散群聊'))); }
      return;
    }
    try {
      await invoke('v1_leave_group', { groupId });
      if (generation === this.generation) { this.clear(); this.onChange(); }
    } catch (error) { if (generation === this.generation) this.status('#group-error', trError(error, t('无法退出群聊'))); }
  }

  async addMember(): Promise<void> {
    const select = this.root.querySelector<HTMLSelectElement>('#group-member-pick');
    if (!this.groupId || !select || !select.value) return;
    const groupId = this.groupId, generation = this.generation, member = select.value;
    try {
      await invoke('v1_add_group_member', { groupId, member });
      if (generation === this.generation) { select.value = ''; await this.loadMembers(); }
    } catch (error) { if (generation === this.generation) this.status('#group-error', trError(error, t('无法添加成员'))); }
  }

  async removeMember(member: string): Promise<void> {
    if (!this.groupId || !member) return;
    const groupId = this.groupId, generation = this.generation;
    try {
      await invoke('v1_remove_group_member', { groupId, member });
      if (generation === this.generation) await this.loadMembers();
    } catch (error) { if (generation === this.generation) this.status('#group-error', trError(error, t('无法移除成员'))); }
  }

  async transferOwner(member: string): Promise<void> {
    if (!this.groupId || !member || !this.isOwner) return;
    const groupId = this.groupId, generation = this.generation;
    const target = this.members.find(item => item.member === member);
    if (!target) return;
    if (!window.confirm(`${t('确认把群主转让给')}${target.display_name}${t('？转让后你将不再拥有管理权限。')}`)) return;
    try {
      await invoke('v1_transfer_group_owner', { groupId, member });
      if (generation === this.generation) {
        this.status('#group-error', `${t('群主已转让给')}${target.display_name}${t('；通知会在对方在线时送达。')}`);
        await this.loadMembers();
        this.dispatchChanged();
      }
    } catch (error) { if (generation === this.generation) this.status('#group-error', trError(error, t('无法转让群主'))); }
  }

  async dissolve(): Promise<void> {
    if (!this.groupId || !this.isOwner) return;
    const groupId = this.groupId, generation = this.generation;
    if (!window.confirm(t('确认解散该群聊？所有成员的群会话都会结束，本机聊天记录会保留但不能再发送。'))) return;
    try {
      await invoke('v1_dissolve_group', { groupId });
      if (generation === this.generation) { this.clear(); this.onChange(); }
    } catch (error) { if (generation === this.generation) this.status('#group-error', trError(error, t('无法解散群聊'))); }
  }

  toggleMembers(): void { this.membersOpen = !this.membersOpen; this.paintMembers(); }

  toggleNoticeEditor(): void { this.noticeOpen = !this.noticeOpen; this.paintNotice(); }

  private paintNotice(): void {
    const bar = this.root.querySelector<HTMLElement>('#group-notice-bar');
    if (bar) {
      bar.hidden = (!this.notice && !this.noticeOpen);
      const text = bar.querySelector<HTMLElement>('#group-notice-text');
      if (text) text.textContent = this.notice || t('暂无公告');
      const edit = bar.querySelector<HTMLButtonElement>('button[data-action="group-notice-edit"]');
      if (edit) edit.textContent = this.isOwner ? t('编辑') : t('查看');
    }
    const editor = this.root.querySelector<HTMLElement>('#group-notice-editor');
    if (editor) {
      editor.hidden = !this.noticeOpen;
      if (this.noticeOpen) {
        const input = editor.querySelector<HTMLTextAreaElement>('#group-notice-input');
        if (input && document.activeElement !== input) input.value = this.notice;
      }
    }
  }

  /// Reads the latest announcement; called on open and when conversations change.
  async loadNotice(): Promise<void> {
    if (!isTauri() || !this.groupId) return;
    const generation = this.generation, groupId = this.groupId;
    try {
      const notice = await invoke<string | null>('v1_group_notice', { groupId });
      if (generation !== this.generation || groupId !== this.groupId) return;
      this.notice = notice ?? '';
      this.paintNotice();
    } catch { /* keep the previously shown notice */ }
  }

  async saveNotice(value: string): Promise<void> {
    if (!this.groupId || !this.isOwner) return;
    const status = this.root.querySelector<HTMLElement>('#group-notice-status');
    try {
      await invoke('v1_set_group_notice', { groupId: this.groupId, notice: value });
      this.notice = value;
      this.noticeOpen = false;
      this.paintNotice();

    } catch (error) {
      if (status) status.textContent = typeof error === 'string' && error.length <= 200 ? error : t('保存失败，请重试');
    }
  }

  toggleSearch(): void {
    this.searchOpen = !this.searchOpen;
    const box = this.root.querySelector<HTMLElement>('#group-search');
    if (box) box.hidden = !this.searchOpen;
    if (this.searchOpen) {
      const input = this.root.querySelector<HTMLInputElement>('#group-search-query');
      input?.focus();
      this.paintSearchMembers();
      this.paintSearch();
    }
  }

  closeSearch(): void {
    this.searchOpen = false; this.searchResults = []; this.searchStatus = ''; this.searchBusy = false;
    const box = this.root.querySelector<HTMLElement>('#group-search');
    if (box) box.hidden = true;
  }

  searchDraft(field: 'query' | 'member' | 'category', value: string): void {
    if (field === 'query') this.searchQuery = value;
    if (field === 'member') this.searchMember = value;
    if (field === 'category') this.searchCategory = (value === 'text' || value === 'file' || value === 'image' ? value : 'all');
  }

  async runSearch(): Promise<void> {
    if (!isTauri() || !this.groupId || this.searchBusy) return;
    const groupId = this.groupId, generation = this.generation;
    this.searchBusy = true; this.searchStatus = t('正在搜索…'); this.searchResults = []; this.paintSearch();
    try {
      const results = await invoke<GroupMessage[]>('v1_search_group', {
        groupId, query: this.searchQuery, category: this.searchCategory,
        member: this.searchMember || null, limit: 50, offset: 0,
      });
      if (generation !== this.generation) return;
      this.searchResults = results;
      this.searchStatus = results.length ? `${t('找到')}${results.length}${t('条')}` : t('没有匹配的记录');
    } catch (error) {
      if (generation === this.generation) this.searchStatus = trError(error, t('搜索失败'));
    } finally { this.searchBusy = false; if (generation === this.generation) this.paintSearch(); }
  }

  /// Quote an existing message; file quotes stay behind the experimental switch.
  async quote(messageId: string): Promise<void> {
    const message = this.messages.find(item => item.id === messageId);
    if (!message) return;
    if (message.kind === 'file') {
      try {
        const settings = await invoke<{ group_file_quote: boolean }>('v1_experiment_settings');
        if (!settings.group_file_quote) { this.status('#group-error', t('群文件引用实验功能未开启（设置 → 实验功能）')); return; }
      } catch { this.status('#group-error', t('无法读取实验设置')); return; }
    }
    this.quoted = { id: message.id, name: message.sender_name, preview: previewOf(message.body) };
    this.paintReplyBar();
    const input = this.root.querySelector<HTMLTextAreaElement>('#group-input');
    input?.focus();
  }

  cancelQuote(): void { this.quoted = null; this.paintReplyBar(); }

  /// Jump to a message, paging older history until it is in memory.
  async jumpTo(messageId: string): Promise<void> {
    if (!messageId) return;
    const found = () => this.messages.find(item => item.id === messageId);
    await loadUntilFound(() => !!found(), () => this.hasOlder, () => this.messages.length, () => this.load(true));
    const node = this.root.querySelector<HTMLElement>(`#group-messages [data-id="${messageId}"]`);
    if (!node) { this.status('#group-error', t('原消息不在当前记录中，可能尚未同步')); return; }
    node.scrollIntoView({ block: 'center' });
    node.classList.add('jump-target');
    window.setTimeout(() => node.classList.remove('jump-target'), 1600);
  }

  selectMention(member: string): void {
    const target = this.members.find(item => item.member === member && item.active);
    const input = this.root.querySelector<HTMLTextAreaElement>('#group-input');
    if (!target || !input) return;
    const name = target.display_name;
    const value = input.value.replace(/(^|\s)@[^\s@]*$/, `$1@${name} `);
    input.value = value;
    this.drafts.set(this.groupId, value);
    this.mentioned.set(member, name);
    this.mentionQuery = null;
    this.paintMentionPicker();
    input.focus();
  }

  private dispatchChanged(): void { this.root.dispatchEvent(new CustomEvent('conversations-changed')); }

  private status(selector: string, value: string): void {
    const target = this.root.querySelector<HTMLElement>(selector); if (target) target.textContent = value;
  }

  private paintSubtitle(): void {
    const subtitle = this.root.querySelector<HTMLElement>('#group-subtitle');
    if (!subtitle) return;
    const active = this.members.filter(member => member.active).length;
    subtitle.textContent = this.members.length && active === 0
      ? t('群聊已解散 · 仅可查看历史')
      : `${active}${t('位成员 · 点对点加密 · 无中转')}`;
  }

  private paintReplyBar(): void {
    const bar = this.root.querySelector<HTMLElement>('#reply-bar');
    const text = this.root.querySelector<HTMLElement>('#reply-bar-text');
    if (!bar || !text) return;
    bar.hidden = !this.quoted;
    text.textContent = this.quoted ? `${t('引用')}${this.quoted.name}：${this.quoted.preview}` : '';
  }

  private paintMentionPicker(): void {
    const box = this.root.querySelector<HTMLElement>('#mention-picker');
    if (!box) return;
    box.replaceChildren();
    if (this.mentionQuery === null) { box.hidden = true; return; }
    const query = this.mentionQuery.toLowerCase();
    const candidates = this.members.filter(member => member.active && member.member !== this.self
      && (query === '' || member.display_name.toLowerCase().includes(query)));
    if (!candidates.length) { box.hidden = true; return; }
    box.hidden = false;
    for (const member of candidates.slice(0, 8)) {
      const button = document.createElement('button');
      button.className = 'outline';
      button.textContent = member.display_name;
      button.dataset.action = 'group-mention'; button.dataset.member = member.member;
      box.append(button);
    }
  }

  private paintMembers(): void {
    const box = this.root.querySelector<HTMLElement>('#group-members');
    if (!box) return;
    box.hidden = !this.membersOpen;
    if (!this.membersOpen) return;
    box.replaceChildren();
    const heading = document.createElement('p'); heading.className = 'group-members-title';
    heading.textContent = `${t('群成员（')}${this.members.filter(member => member.active).length}）`;
    box.append(heading);
    for (const member of this.members) {
      if (!member.active) continue;
      const row = document.createElement('div'); row.className = 'group-member-row';
      const name = document.createElement('span');
      name.textContent = `${member.display_name}${member.role === 'owner' ? t(' · 群主') : ''}${member.member === this.self ? t(' · 我') : ''}`;
      const joined = document.createElement('small');
      joined.className = 'file-meta';
      joined.textContent = member.joined_at > 0 ? `${formatJoined(member.joined_at)}${t('进群')}` : t('进群时间未知');
      row.append(name, joined);
      if (this.isOwner && member.role !== 'owner') {
        const transfer = document.createElement('button');
        transfer.className = 'outline'; transfer.textContent = t('转让群主');
        transfer.dataset.action = 'group-transfer-owner'; transfer.dataset.member = member.member;
        const remove = document.createElement('button');
        remove.className = 'outline'; remove.textContent = t('移除');
        remove.dataset.action = 'remove-group-member'; remove.dataset.member = member.member;
        row.append(transfer, remove);
      }
      box.append(row);
    }
    this.paintAddMember(box);
    if (this.isOwner) {
      const danger = document.createElement('button');
      danger.className = 'outline danger'; danger.textContent = t('解散群聊');
      danger.dataset.action = 'group-dissolve';
      box.append(danger);
    }
  }

  private paintAddMember(box: HTMLElement): void {
    if (!this.isOwner) return;
    const inGroup = new Set(this.members.filter(member => member.active).map(member => member.member));
    const candidates = this.contacts.filter(contact => !inGroup.has(contact.fingerprint));
    if (!candidates.length) return;
    const row = document.createElement('div'); row.className = 'group-member-row';
    const select = document.createElement('select'); select.id = 'group-member-pick';
    const empty = document.createElement('option'); empty.value = ''; empty.textContent = t('选择已确认联系人'); select.append(empty);
    for (const contact of candidates) {
      const option = document.createElement('option'); option.value = contact.fingerprint;
      option.textContent = `${contact.display_name}（${contact.fingerprint.slice(0, 8)}）`;
      select.append(option);
    }
    const add = document.createElement('button'); add.className = 'outline'; add.textContent = t('邀请进群');
    add.dataset.action = 'add-group-member';
    row.append(select, add);
    box.append(row);
    const hint = document.createElement('small'); hint.className = 'group-members-hint';
    hint.textContent = t('新成员只能看到加入之后的消息；邀请会在对方在线时送达。');
    box.append(hint);
  }

  private paintSearchMembers(): void {
    const select = this.root.querySelector<HTMLSelectElement>('#group-search-member');
    if (!select) return;
    const current = this.searchMember;
    select.replaceChildren();
    const all = document.createElement('option'); all.value = ''; all.textContent = t('全部成员'); select.append(all);
    for (const member of this.members.filter(item => item.active)) {
      const option = document.createElement('option'); option.value = member.member;
      option.textContent = member.display_name;
      select.append(option);
    }
    select.value = this.members.some(member => member.member === current) ? current : '';
    this.searchMember = select.value;
    const category = this.root.querySelector<HTMLSelectElement>('#group-search-category');
    if (category) category.value = this.searchCategory;
    const input = this.root.querySelector<HTMLInputElement>('#group-search-query');
    if (input && document.activeElement !== input) input.value = this.searchQuery;
  }

  private paintSearch(): void {
    const status = this.root.querySelector<HTMLElement>('#group-search-status');
    const list = this.root.querySelector<HTMLUListElement>('#group-search-results');
    if (!status || !list) return;
    status.textContent = this.searchStatus;
    list.replaceChildren();
    for (const message of this.searchResults) {
      const item = document.createElement('li');
      const button = document.createElement('button');
      button.className = 'search-result';
      button.dataset.action = 'group-jump'; button.dataset.id = message.id;
      const meta = document.createElement('small');
      meta.textContent = `${message.sender_name} · ${new Date(message.created_at * 1000).toLocaleString()}`;
      const body = document.createElement('span');
      body.textContent = message.body;
      button.append(meta, body);
      item.append(button);
      list.append(item);
    }
  }

  /// Opens one completed group attachment in the global workspace panel.
  openPreview(message: GroupMessage): void {
    if (!isTauri() || message.kind !== 'file' || message.file_status !== 'completed') return;
    workspace.open({
      kind: 'group-file', groupId: this.groupId, messageId: message.id,
      name: message.file_name ?? message.body,
    });
  }

  /// Resolver entry used by the workspace for live opens and restored tabs.
  async loadWorkspaceFile(source: { messageId: string; name: string }): Promise<LoadedTab> {
    try { return await this.loadPreview(source.messageId, source.name); }
    catch (error) { return { kind: 'error', message: trError(error, t('无法读取该文件')) }; }
  }

  private async loadPreview(messageId: string, name: string): Promise<LoadedTab> {
    const kind = previewKind(name);
    if (kind === 'image') {
      const preview = await invoke<{ name: string; data_url: string } | null>('v1_read_group_attachment', { groupId: this.groupId, messageId });
      if (!preview) return { kind: 'unsupported', reason: t('图片过大或无法读取，未能预览') };
      this.thumbs.set(messageId, preview.data_url);
      return { kind: 'image', src: preview.data_url };
    }
    if (kind === 'pdf') {
      const bytes = await invoke<ArrayBuffer>('v1_group_attachment_bytes', { groupId: this.groupId, messageId });
      return { kind: 'pdf', bytes };
    }
    if (kind === 'other') {
      const plugin = await invoke<{ plugin_id: string; html: string } | null>('v1_plugin_preview', { fileName: name });
      if (!plugin) return { kind: 'unsupported', reason: t('该类型暂不支持内置预览；可用“用默认程序打开”或“打开所在目录”，安装预览插件后也可在此查看。') };
      const data = await invoke<string>('v1_group_attachment_base64', { groupId: this.groupId, messageId });
      return { kind: 'plugin', html: plugin.html, bytes: base64ToBytes(data) };
    }
    const preview = await invoke<TextPreview | null>('v1_read_group_text_attachment', { groupId: this.groupId, messageId });
    if (!preview) return { kind: 'unsupported', reason: t('文件过大、不是文本或无法读取，未能预览') };
    const textKind = preview.kind === 'markdown' ? 'markdown' : preview.kind === 'html' ? 'html' : 'text';
    return { kind: textKind, text: preview.text };
  }

  /// Click behaviour for a completed file card, governed by the workspace setting.
  private activateFile(message: GroupMessage): void {
    if (message.kind !== 'file' || message.file_status !== 'completed') return;
    if (this.fileOpenMode === 'system') { void this.openWithSystem(message); return; }
    this.openPreview(message);
  }

  async openSystemById(messageId: string): Promise<void> {
    try { await invoke('v1_open_group_attachment', { groupId: this.groupId, messageId }); }
    catch (error) {
      const text = trError(error, t('无法打开文件'));
      this.status('#group-file-status', text);
      workspace.notify(text);
    }
  }

  private openWithSystem(message: GroupMessage): Promise<void> {
    return this.openSystemById(message.id);
  }

  async reveal(messageId: string): Promise<void> {
    try { await invoke('v1_reveal_group_attachment', { groupId: this.groupId, messageId }); }
    catch (error) {
      const text = trError(error, t('无法打开所在目录'));
      this.status('#group-error', text);
      workspace.notify(text);
    }
  }

  private canRecall(message: GroupMessage): boolean {
    if (message.sender === this.self || message.recalled_at) return false;
    return Date.now() / 1000 - message.created_at <= 120;
  }

  private textMenu(event: MouseEvent, message: GroupMessage): void {
    const items = [];
    if (this.canRecall(message)) {
      items.push({ label: t('撤回'), danger: true, run: () => { void this.recall(message); } });
    }
    items.push({ label: t('复制文字'), run: () => { void navigator.clipboard?.writeText(message.body).catch(() => {}); } });
    items.push({ label: t('删除'), danger: true, run: () => { void this.deleteLocal(message.id); } });
    showContextMenu(event, items);
  }

  /// Deletes this message from the local record only.
  async deleteLocal(messageId: string): Promise<void> {
    try {
      await invoke('v1_delete_messages', { kind: 'group', id: this.groupId, ids: [messageId] });
      await this.load(false, true);
    } catch (error) { workspace.notify(trError(error, t('删除失败'))); }
  }

  async recall(message: GroupMessage): Promise<void> {
    try {
      const result = await invoke<string>('v1_recall_message', { kind: 'group', id: message.id, groupId: this.groupId });
      if (result === 'recalled') {
        message.recalled_at = Math.floor(Date.now() / 1000);
        message.body = ' ';
        this.paint('auto');
      } else if (result === 'too_late') {
        this.status('#group-error', t('超过 2 分钟，不能再撤回'));
      }
    } catch (error) {
      this.status('#group-error', trError(error, t('撤回失败')));
    }
  }

  /// Right-click on a file bubble: open with the default app, reveal it, or preview it.
  private fileMenu(event: MouseEvent, message: GroupMessage): void {
    const name = message.file_name ?? message.body;
    if (message.file_status !== 'completed') {
      const items = [];
      if (this.canRecall(message)) {
        items.push({ label: t('撤回并取消发送'), danger: true, run: () => { void this.recall(message); } });
      }
      items.push({ label: t('文件尚未接收完成'), run: () => {} });
      showContextMenu(event, items);
      return;
    }
    showContextMenu(event, [
      { label: t('用默认程序打开'), run: () => { void this.openWithSystem(message); } },
      { label: t('打开所在目录'), run: () => {
        void invoke('v1_reveal_group_attachment', { groupId: this.groupId, messageId: message.id })
          .catch((error) => this.status('#group-error', trError(error, t('无法打开所在目录'))));
      } },
      { label: t('预览'), run: () => { this.openPreview(message); } },
      { label: t('复制文件名'), run: () => { void navigator.clipboard?.writeText(name).catch(() => {}); } },
      ...(this.canRecall(message) ? [{ label: t('撤回（已接收的文件保留）'), danger: true, run: () => { void this.recall(message); } }] : []),
    ]);
  }

  openPreviewById(id: string): void {
    const message = this.messages.find((item) => item.id === id);
    if (message) this.openPreview(message);
  }

  private paint(mode: 'bottom' | 'preserve' | 'auto'): void {
    const history = this.root.querySelector<HTMLElement>('#group-history');
    const list = this.root.querySelector<HTMLElement>('#group-messages');
    const older = this.root.querySelector<HTMLButtonElement>('button[data-action="group-older"]');
    if (!history || !list || !older) return;
    const atBottom = history.scrollHeight - history.clientHeight - history.scrollTop < 40;
    const top = history.getBoundingClientRect().top;
    const anchor = [...list.children].find(node => node.getBoundingClientRect().bottom > top) as HTMLElement | undefined;
    const anchorId = anchor?.dataset.id, anchorOffset = anchor ? anchor.getBoundingClientRect().top - top : 0;
    const previousTop = history.scrollTop, previousHeight = history.scrollHeight;
    list.replaceChildren();
    this.renderedBatches.clear();
    for (const message of this.messages) {
      const incoming = message.sender !== this.self;
      const row = document.createElement('div'); row.className = `message-row${incoming ? ' incoming' : ''}`; row.dataset.id = message.id;
      if (incoming && message.mentions.includes(this.self)) row.classList.add('mentioned-row');
      const column = document.createElement('div'); column.className = 'message-column';
      if (incoming) {
        const who = document.createElement('small'); who.className = 'message-sender';
        who.textContent = message.sender_name; column.append(who);
      }
      if (message.recalled_at) {
        const note = document.createElement('div'); note.className = 'message-bubble recalled';
        note.textContent = incoming ? `${message.sender_name}${t('撤回了一条消息')}` : t('你撤回了一条消息');
        column.append(note); row.append(column); list.append(row);
        continue;
      }
      const bubble = document.createElement('div'); bubble.className = 'message-bubble';
      bubble.addEventListener('contextmenu', (event) => this.textMenu(event, message));
      if (message.reply_preview) {
        const quote = document.createElement('button');
        quote.className = 'quote-block';
        quote.dataset.action = 'group-jump';
        if (message.reply_to) quote.dataset.id = message.reply_to;
        quote.textContent = `${message.reply_sender ?? t('原消息')}：${message.reply_preview}`;
        bubble.append(quote);
      }
      const batch = this.batches.get(message.id);
      if (batch) {
        if (this.renderedBatches.has(batch)) continue;
        this.renderedBatches.add(batch);
        const caption = this.batchCaptions.get(batch) ?? '';
        if (caption) {
          const text = document.createElement('div');
          text.className = 'message-text';
          text.append(linkifiedText(caption));
          bubble.append(text);
        }
        const grid = document.createElement('div'); grid.className = 'batch-grid';
        for (const item of this.messages.filter(candidate => this.batches.get(candidate.id) === batch)) {
          const cell = document.createElement('div'); cell.className = 'batch-item';
          cell.addEventListener('contextmenu', (event) => this.fileMenu(event, item));
          if (this.thumbs.has(item.id)) {
            const image = document.createElement('img'); image.className = 'file-thumb'; image.src = this.thumbs.get(item.id) ?? '';
            image.alt = item.file_name ?? t('图片');
            image.addEventListener('click', (event) => {
              event.stopPropagation();
              if (this.imageOpenMode === 'workspace') { this.openPreview(item); return; }
              showImage(image.src, item.file_name ?? t('图片'));
            });
            cell.append(image);
          } else if (item.file_status === 'completed') {
            this.loadThumbnail(item.id);
          }
          const meta = document.createElement('small'); meta.className = 'file-meta';
          meta.textContent = `${this.formatSize(item.file_size ?? 0)} · ${this.fileStatusText(item)}`;
          cell.append(meta);
          grid.append(cell);
        }
        bubble.append(grid);
      } else if (message.kind === 'file') {
        const card = document.createElement('div'); card.className = 'file-card';
        card.addEventListener('contextmenu', (event) => this.fileMenu(event, message));
        card.addEventListener('click', (event) => {
          if ((event.target as HTMLElement).closest('.linkish')) return;
          this.activateFile(message);
        });
        if (this.isImageMessage(message) && this.thumbs.has(message.id)) {
          const image = document.createElement('img'); image.className = 'file-thumb'; image.src = this.thumbs.get(message.id) ?? '';
          image.alt = message.file_name ?? t('图片');
          image.addEventListener('click', (event) => {
            event.stopPropagation();
            if (this.imageOpenMode === 'workspace') { this.openPreview(message); return; }
            showImage(image.src, message.file_name ?? t('图片'));
          });
          card.append(image);
          this.loadThumbnail(message.id);
        } else if (this.isImageMessage(message) && message.file_status === 'completed') {
          this.loadThumbnail(message.id);
        }
        const name = document.createElement('div'); name.className = 'file-name'; name.textContent = message.file_name ?? message.body;
        const meta = document.createElement('small'); meta.className = 'file-meta';
        meta.textContent = `${this.formatSize(message.file_size ?? 0)} · ${this.fileStatusText(message)}`;
        card.append(name, meta);
        if (message.file_status === 'completed') {
          const preview = document.createElement('button');
          preview.className = 'linkish'; preview.textContent = t('预览');
          preview.dataset.action = 'group-preview-file'; preview.dataset.id = message.id;
          const reveal = document.createElement('button');
          reveal.className = 'linkish'; reveal.textContent = t('打开所在目录');
          reveal.dataset.action = 'group-reveal-file'; reveal.dataset.id = message.id;
          card.append(preview, reveal);
        }
        bubble.append(card);
      } else {
        const body = document.createElement('div'); body.className = 'message-text'; body.append(linkifiedText(message.body));
        bubble.append(body);
      }
      if (incoming && message.mentions.includes(this.self)) {
        const badge = document.createElement('small'); badge.className = 'mention-badge'; badge.textContent = '@我';
        bubble.append(badge);
      }
      column.append(bubble);
      const time = document.createElement('small'); time.className = 'message-time';
      time.textContent = formatStamp(message.created_at);
      column.append(time);
      const tools = document.createElement('div'); tools.className = 'message-tools';
      const reply = document.createElement('button');
      reply.className = 'linkish'; reply.textContent = t('引用');
      reply.dataset.action = 'group-reply'; reply.dataset.id = message.id;
      tools.append(reply);
      if (!incoming) {
        const status = document.createElement('small'); status.className = 'message-status';
        status.textContent = message.pending > 0 ? `${t('待送达')}${message.pending}${t('人')}` : t('已送达');
        tools.append(status);
      }
      column.append(tools);
      row.append(column); list.append(row);
    }
    older.hidden = !this.hasOlder;
    if (mode === 'bottom' || (mode === 'auto' && atBottom)) history.scrollTop = history.scrollHeight;
    else {
      const restored = [...list.children].find(node => (node as HTMLElement).dataset.id === anchorId) as HTMLElement | undefined;
      history.scrollTop = restored ? previousTop + restored.getBoundingClientRect().top - top - anchorOffset : previousTop + history.scrollHeight - previousHeight;
    }
  }

  /// Members pull the owner's copy on open; an offline owner keeps the cached image.
  async loadAvatar(): Promise<void> {
    if (!isTauri() || !this.groupId) return;
    const groupId = this.groupId, generation = this.generation;
    try { await invoke('v1_fetch_group_avatar', { groupId }); } catch { /* keep cache */ }
    let bytes: number[] | null = null;
    try { bytes = await invoke<number[] | null>('v1_group_avatar', { groupId }); } catch { return; }
    if (generation !== this.generation) return;
    this.setGroupAvatar(bytes && bytes.length ? new Uint8Array(bytes) : null);
  }

  setGroupAvatar(bytes: Uint8Array | null): void {
    this.avatarBytes = bytes && bytes.length ? bytes : null;
    if (this.avatarUrl) { URL.revokeObjectURL(this.avatarUrl); this.avatarUrl = null; }
    this.avatarUrl = this.avatarBytes ? URL.createObjectURL(new Blob([toArrayBuffer(this.avatarBytes)], { type: 'image/png' })) : null;
    this.paintAvatar();
  }

  private paintAvatar(): void {
    const box = this.root.querySelector<HTMLElement>('#group-avatar');
    if (!box) return;
    box.replaceChildren();
    if (this.avatarUrl) {
      const image = document.createElement('img');
      image.src = this.avatarUrl; image.alt = t('群头像');
      box.append(image);
    } else {
      box.textContent = this.name.slice(0, 1) || t('群');
    }
  }

  render(): void {
    this.paintAvatar();
    const button = this.root.querySelector<HTMLButtonElement>('#group-avatar-button');
    if (button) button.hidden = !this.isOwner;
    const remove = this.root.querySelector<HTMLButtonElement>('#group-avatar-remove');
    if (remove) remove.hidden = !this.isOwner;
    const name = this.root.querySelector<HTMLElement>('#group-name');
    const input = this.root.querySelector<HTMLTextAreaElement>('#group-input');
    if (name) name.textContent = this.name;
    if (input) input.value = this.drafts.get(this.groupId) ?? '';
    this.paintStaging();
    this.paintSubtitle();
    this.paintMembers();
    this.paintReplyBar();
    this.paintMentionPicker();
    this.paintSearchMembers();
    this.paintSearch();
    this.paint('bottom');
  }
}

/// Copies into a fresh ArrayBuffer so Blob accepts it under strict DOM typings.
function toArrayBuffer(bytes: Uint8Array): ArrayBuffer {
  const buffer = new ArrayBuffer(bytes.byteLength);
  new Uint8Array(buffer).set(bytes);
  return buffer;
}

function previewKind(name: string): 'image' | 'pdf' | 'html' | 'markdown' | 'text' | 'other' {
  const extension = name.slice(name.lastIndexOf('.') + 1).toLowerCase();
  if (['png', 'jpg', 'jpeg', 'gif', 'webp', 'bmp'].includes(extension)) return 'image';
  if (extension === 'pdf') return 'pdf';
  if (['html', 'htm'].includes(extension)) return 'html';
  if (['md', 'markdown'].includes(extension)) return 'markdown';
  if (['txt', 'log', 'csv', 'json', 'xml', 'ini', 'toml', 'yaml', 'yml'].includes(extension)) return 'text';
  return 'other';
}

function formatJoined(seconds: number): string {
  const date = new Date(seconds * 1000);
  if (Number.isNaN(date.getTime())) return t('进群时间未知');
  return date.toLocaleDateString();
}

function previewOf(body: string): string {
  const collapsed = body.split(/\s+/).filter(Boolean).join(' ');
  return collapsed.length > 60 ? `${collapsed.slice(0, 60)}…` : collapsed;
}

function escapeHtml(value: string): string {
  return value.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
}
