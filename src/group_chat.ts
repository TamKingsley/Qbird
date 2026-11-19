import { invoke, isTauri } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { showImage } from './image_viewer';

type GroupMessage = {
  id: string; group_id: string; sender: string; sender_name: string; body: string;
  created_at: number; logical: number; pending: number;
  reply_to: string | null; reply_sender: string | null; reply_preview: string | null;
  mentions: string[];
  kind: string; file_name: string | null; file_size: number | null; file_sha256: string | null;
  file_status: string | null; file_transferred: number | null; file_error: string | null;
};
type GroupMember = { group_id: string; member: string; display_name: string; role: string; joined_at: number; active: boolean };
type Contact = { fingerprint: string; display_name: string; trust_state: string };
type Cursor = Pick<GroupMessage, 'logical' | 'sender' | 'id'>;
const cursorOf = (message: GroupMessage): Cursor => ({ logical: message.logical, sender: message.sender, id: message.id });
const compare = (a: Cursor, b: Cursor): number => a.logical - b.logical || (a.sender < b.sender ? -1 : a.sender > b.sender ? 1 : a.id < b.id ? -1 : a.id > b.id ? 1 : 0);
type SearchCategory = 'all' | 'text' | 'file' | 'image';
type Quoted = { id: string; name: string; preview: string };

/// Group chat: outbound messages are queued per member by the backend, so the UI only
/// shows the pending count and never claims delivery on its own. Replies keep a short
/// local quote, and @ mentions are resolved against the active member list.
export class GroupChat {
  groupId = '';
  private name = '';
  private owner = '';
  private isOwner = false;
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

  constructor(private root: HTMLElement, private onChange: () => void) {}

  view(): string {
    return `<section class="self-chat" aria-label="群聊">
      <header class="chat-header"><div><strong id="group-name"></strong><small id="group-subtitle">群聊</small></div><div class="group-actions"><button class="outline" data-action="export-text" data-kind="group">导出文本</button><button class="outline" data-action="toggle-group-search">搜索</button><button class="outline" data-action="toggle-group-members">成员</button><button class="outline" data-action="hide-group">不显示此会话</button><button class="outline" data-action="leave-group">退出群聊</button></div></header>
      <div class="group-search" id="group-search" hidden>
        <div class="search-row">
          <input id="group-search-query" type="search" maxlength="200" placeholder="搜索聊天记录" aria-label="搜索聊天记录">
          <select id="group-search-category" aria-label="搜索类别"><option value="all">全部</option><option value="text">文字</option><option value="file">文件</option><option value="image">图片</option></select>
          <select id="group-search-member" aria-label="按成员筛选"><option value="">全部成员</option></select>
          <button class="outline" data-action="group-search-run">搜索</button>
          <button class="outline" data-action="group-search-close">关闭</button>
        </div>
        <p id="group-search-status" role="status"></p>
        <ul id="group-search-results" class="search-results"></ul>
      </div>
      <div class="chat-history" id="group-history" aria-label="群聊记录"><div class="group-members" id="group-members" hidden></div><button class="load-older" data-action="group-older" hidden>加载更早消息</button><div id="group-messages"></div></div>
      <div class="chat-compose">
        <div class="reply-bar" id="reply-bar" hidden><span id="reply-bar-text"></span><button class="outline" data-action="group-cancel-reply">取消引用</button></div>
        <label for="group-input">消息</label>
        <textarea id="group-input" rows="3" maxlength="32768" placeholder="发消息给群里的人…输入 @ 可选择成员"></textarea>
        <div class="mention-picker" id="mention-picker" hidden></div>
        <div class="compose-footer"><span>Enter 发送 · Shift+Enter 换行</span><div class="compose-actions"><button class="outline" data-action="group-image">发送图片</button><button class="outline" data-action="group-file">发送文件</button><button class="outline" data-action="group-screenshot">截图</button><button data-action="group-send">发送</button></div></div>
        <p id="group-file-status" role="status"></p>
        <p id="group-error" role="status"></p>
      </div>
    </section>`;
  }

  clear(): void {
    this.groupId = ''; this.generation++; this.membersOpen = false; this.quoted = null;
    this.thumbs.clear(); this.thumbPending.clear();
    this.mentionQuery = null; this.mentioned.clear(); this.closeSearch();
  }

  draft(value: string): void {
    if (!this.groupId) return;
    this.drafts.set(this.groupId, value);
    const match = /(?:^|\s)@([^\s@]{0,32})$/.exec(value);
    this.mentionQuery = match ? match[1] : null;
    this.paintMentionPicker();
  }

  async open(groupId: string, name: string): Promise<void> {
    if (!isTauri()) return;
    const generation = ++this.generation;
    await invoke('v1_open_group', { groupId });
    if (generation !== this.generation) return;
    this.groupId = groupId; this.name = name; this.messages = []; this.hasOlder = false;
    this.refreshBefore = null; this.loading = false; this.membersOpen = false;
    this.quoted = null; this.mentionQuery = null; this.mentioned.clear(); this.closeSearch();
    this.onChange();
    if (!this.self) { try { this.self = await invoke<string>('identity_fingerprint'); } catch { this.self = ''; } }
    await Promise.all([this.load(false, true), this.loadMembers()]);
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
      if (older || initial) this.hasOlder = batch.length === 200;
      if (changed || older || initial) this.paint(initial ? 'bottom' : older ? 'preserve' : 'auto');
    } catch { if (generation === this.generation) this.status('#group-error', '无法读取群聊记录'); }
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
      this.paintMembers();
      this.paintSubtitle();
      this.paintSearchMembers();
    } catch { this.status('#group-error', '无法读取群成员'); }
  }

  notify(text: string): void { this.status('#group-error', text); }

  async pick(kind: 'image' | 'file'): Promise<void> {
    if (!isTauri() || !this.groupId || this.attaching) return;
    this.attaching = true;
    try {
      const selected = await open({
        multiple: false, directory: false,
        title: kind === 'image' ? '选择图片' : '选择文件',
        filters: kind === 'image' ? [{ name: '图片', extensions: ['png', 'jpg', 'jpeg', 'gif', 'webp', 'bmp'] }] : undefined,
      });
      if (!selected || typeof selected !== 'string') return;
      await this.queuePaths([selected]);
    } catch (error) { this.status('#group-error', typeof error === 'string' ? error : '无法打开文件选择器'); }
    finally { this.attaching = false; }
  }

  async queuePaths(paths: string[]): Promise<void> {
    for (const path of paths) await this.queuePath(path);
  }

  async queuePath(path: string): Promise<void> {
    if (!this.groupId || this.attaching) return;
    const groupId = this.groupId, generation = this.generation;
    this.attaching = true;
    this.status('#group-file-status', '正在计算校验值…');
    try {
      const message = await invoke<GroupMessage>('v1_queue_group_file', { groupId, path });
      if (generation === this.generation) {
        if (!this.messages.some(item => item.id === message.id)) { this.messages.push(message); this.messages.sort(compare); }
        this.paint('bottom');
        this.status('#group-file-status', `已加入待发送：${message.file_name ?? message.body}（对方在线后自动传输）`);
        this.dispatchChanged();
      }
    } catch (error) {
      if (generation === this.generation) this.status('#group-file-status', typeof error === 'string' ? error : '无法加入群文件');
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
    if (message.file_status === 'completed') return '已完成';
    if (message.file_status === 'failed') return message.file_error ?? '传输失败';
    if (message.file_status === 'transferring') {
      const percent = total > 0 ? Math.min(100, Math.round((done / total) * 100)) : 0;
      return `传输中 ${percent}%`;
    }
    return '等待传输';
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
    if (!this.groupId || !input || this.sending) return;
    const groupId = this.groupId, generation = this.generation, body = input.value;
    if (!this.members.some(member => member.member === this.self && member.active)) {
      this.status('#group-error', '该群聊已解散，或你已不在群中'); return;
    }
    if (!body.trim()) { this.status('#group-error', '请输入消息'); return; }
    const mentions = [...this.mentioned.entries()]
      .filter(([, name]) => body.includes(`@${name}`)).map(([member]) => member);
    this.sending = true; if (button) button.disabled = true; this.status('#group-error', '');
    try {
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
      if (generation === this.generation) this.status('#group-error', typeof error === 'string' ? error : '无法保存待发送消息');
    } finally { this.sending = false; if (button?.isConnected) button.disabled = false; }
  }

  async hide(): Promise<void> {
    if (!this.groupId) return;
    const groupId = this.groupId, generation = this.generation;
    try {
      await invoke('v1_hide_group', { groupId });
      if (generation === this.generation) { this.clear(); this.onChange(); }
    } catch { if (generation === this.generation) this.status('#group-error', '无法隐藏会话'); }
  }

  async leave(): Promise<void> {
    if (!this.groupId) return;
    const groupId = this.groupId, generation = this.generation;
    try {
      await invoke('v1_leave_group', { groupId });
      if (generation === this.generation) { this.clear(); this.onChange(); }
    } catch (error) { if (generation === this.generation) this.status('#group-error', typeof error === 'string' ? error : '无法退出群聊'); }
  }

  async addMember(): Promise<void> {
    const select = this.root.querySelector<HTMLSelectElement>('#group-member-pick');
    if (!this.groupId || !select || !select.value) return;
    const groupId = this.groupId, generation = this.generation, member = select.value;
    try {
      await invoke('v1_add_group_member', { groupId, member });
      if (generation === this.generation) { select.value = ''; await this.loadMembers(); }
    } catch (error) { if (generation === this.generation) this.status('#group-error', typeof error === 'string' ? error : '无法添加成员'); }
  }

  async removeMember(member: string): Promise<void> {
    if (!this.groupId || !member) return;
    const groupId = this.groupId, generation = this.generation;
    try {
      await invoke('v1_remove_group_member', { groupId, member });
      if (generation === this.generation) await this.loadMembers();
    } catch (error) { if (generation === this.generation) this.status('#group-error', typeof error === 'string' ? error : '无法移除成员'); }
  }

  async transferOwner(member: string): Promise<void> {
    if (!this.groupId || !member || !this.isOwner) return;
    const groupId = this.groupId, generation = this.generation;
    const target = this.members.find(item => item.member === member);
    if (!target) return;
    if (!window.confirm(`确认把群主转让给 ${target.display_name}？转让后你将不再拥有管理权限。`)) return;
    try {
      await invoke('v1_transfer_group_owner', { groupId, member });
      if (generation === this.generation) {
        this.status('#group-error', `群主已转让给 ${target.display_name}；通知会在对方在线时送达。`);
        await this.loadMembers();
        this.dispatchChanged();
      }
    } catch (error) { if (generation === this.generation) this.status('#group-error', typeof error === 'string' ? error : '无法转让群主'); }
  }

  async dissolve(): Promise<void> {
    if (!this.groupId || !this.isOwner) return;
    const groupId = this.groupId, generation = this.generation;
    if (!window.confirm('确认解散该群聊？所有成员的群会话都会结束，本机聊天记录会保留但不能再发送。')) return;
    try {
      await invoke('v1_dissolve_group', { groupId });
      if (generation === this.generation) { this.clear(); this.onChange(); }
    } catch (error) { if (generation === this.generation) this.status('#group-error', typeof error === 'string' ? error : '无法解散群聊'); }
  }

  toggleMembers(): void { this.membersOpen = !this.membersOpen; this.paintMembers(); }

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
    this.searchBusy = true; this.searchStatus = '正在搜索…'; this.searchResults = []; this.paintSearch();
    try {
      const results = await invoke<GroupMessage[]>('v1_search_group', {
        groupId, query: this.searchQuery, category: this.searchCategory,
        member: this.searchMember || null, limit: 50, offset: 0,
      });
      if (generation !== this.generation) return;
      this.searchResults = results;
      this.searchStatus = results.length ? `找到 ${results.length} 条` : '没有匹配的记录';
    } catch (error) {
      if (generation === this.generation) this.searchStatus = typeof error === 'string' ? error : '搜索失败';
    } finally { this.searchBusy = false; if (generation === this.generation) this.paintSearch(); }
  }

  /// Quote an existing message; file quotes stay behind the experimental switch.
  async quote(messageId: string): Promise<void> {
    const message = this.messages.find(item => item.id === messageId);
    if (!message) return;
    if (message.kind === 'file') {
      try {
        const settings = await invoke<{ group_file_quote: boolean }>('v1_experiment_settings');
        if (!settings.group_file_quote) { this.status('#group-error', '群文件引用实验功能未开启（设置 → 实验功能）'); return; }
      } catch { this.status('#group-error', '无法读取实验设置'); return; }
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
    for (let page = 0; page < 20 && !found() && this.hasOlder; page += 1) {
      const before = this.messages.length;
      await this.load(true);
      if (!isTauri() || this.messages.length === before) break;
    }
    const node = this.root.querySelector<HTMLElement>(`#group-messages [data-id="${messageId}"]`);
    if (!node) { this.status('#group-error', '原消息不在当前记录中，可能尚未同步'); return; }
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
      ? '群聊已解散 · 仅可查看历史'
      : `${active} 位成员 · 点对点加密 · 无中转`;
  }

  private paintReplyBar(): void {
    const bar = this.root.querySelector<HTMLElement>('#reply-bar');
    const text = this.root.querySelector<HTMLElement>('#reply-bar-text');
    if (!bar || !text) return;
    bar.hidden = !this.quoted;
    text.textContent = this.quoted ? `引用 ${this.quoted.name}：${this.quoted.preview}` : '';
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
    heading.textContent = `群成员（${this.members.filter(member => member.active).length}）`;
    box.append(heading);
    for (const member of this.members) {
      if (!member.active) continue;
      const row = document.createElement('div'); row.className = 'group-member-row';
      const name = document.createElement('span');
      name.textContent = `${member.display_name}${member.role === 'owner' ? ' · 群主' : ''}${member.member === this.self ? ' · 我' : ''}`;
      row.append(name);
      if (this.isOwner && member.role !== 'owner') {
        const transfer = document.createElement('button');
        transfer.className = 'outline'; transfer.textContent = '转让群主';
        transfer.dataset.action = 'group-transfer-owner'; transfer.dataset.member = member.member;
        const remove = document.createElement('button');
        remove.className = 'outline'; remove.textContent = '移除';
        remove.dataset.action = 'remove-group-member'; remove.dataset.member = member.member;
        row.append(transfer, remove);
      }
      box.append(row);
    }
    this.paintAddMember(box);
    if (this.isOwner) {
      const danger = document.createElement('button');
      danger.className = 'outline danger'; danger.textContent = '解散群聊';
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
    const empty = document.createElement('option'); empty.value = ''; empty.textContent = '选择已确认联系人'; select.append(empty);
    for (const contact of candidates) {
      const option = document.createElement('option'); option.value = contact.fingerprint;
      option.textContent = `${contact.display_name}（${contact.fingerprint.slice(0, 8)}）`;
      select.append(option);
    }
    const add = document.createElement('button'); add.className = 'outline'; add.textContent = '邀请进群';
    add.dataset.action = 'add-group-member';
    row.append(select, add);
    box.append(row);
    const hint = document.createElement('small'); hint.className = 'group-members-hint';
    hint.textContent = '新成员只能看到加入之后的消息；邀请会在对方在线时送达。';
    box.append(hint);
  }

  private paintSearchMembers(): void {
    const select = this.root.querySelector<HTMLSelectElement>('#group-search-member');
    if (!select) return;
    const current = this.searchMember;
    select.replaceChildren();
    const all = document.createElement('option'); all.value = ''; all.textContent = '全部成员'; select.append(all);
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
    for (const message of this.messages) {
      const incoming = message.sender !== this.self;
      const row = document.createElement('div'); row.className = `message-row${incoming ? ' incoming' : ''}`; row.dataset.id = message.id;
      if (incoming && message.mentions.includes(this.self)) row.classList.add('mentioned-row');
      const column = document.createElement('div'); column.className = 'message-column';
      if (incoming) {
        const who = document.createElement('small'); who.className = 'message-sender';
        who.textContent = message.sender_name; column.append(who);
      }
      const bubble = document.createElement('div'); bubble.className = 'message-bubble';
      if (message.reply_preview) {
        const quote = document.createElement('button');
        quote.className = 'quote-block';
        quote.dataset.action = 'group-jump';
        if (message.reply_to) quote.dataset.id = message.reply_to;
        quote.textContent = `${message.reply_sender ?? '原消息'}：${message.reply_preview}`;
        bubble.append(quote);
      }
      if (message.kind === 'file') {
        const card = document.createElement('div'); card.className = 'file-card';
        if (this.isImageMessage(message) && this.thumbs.has(message.id)) {
          const image = document.createElement('img'); image.className = 'file-thumb'; image.src = this.thumbs.get(message.id) ?? '';
          image.alt = message.file_name ?? '图片';
          image.addEventListener('click', () => showImage(image.src, message.file_name ?? '图片'));
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
          const reveal = document.createElement('button');
          reveal.className = 'linkish'; reveal.textContent = '打开所在目录';
          reveal.dataset.action = 'group-reveal-file'; reveal.dataset.id = message.id;
          card.append(reveal);
        }
        bubble.append(card);
      } else {
        const body = document.createElement('div'); body.className = 'message-text'; body.textContent = message.body;
        bubble.append(body);
      }
      if (incoming && message.mentions.includes(this.self)) {
        const badge = document.createElement('small'); badge.className = 'mention-badge'; badge.textContent = '@我';
        bubble.append(badge);
      }
      column.append(bubble);
      const tools = document.createElement('div'); tools.className = 'message-tools';
      const reply = document.createElement('button');
      reply.className = 'linkish'; reply.textContent = '引用';
      reply.dataset.action = 'group-reply'; reply.dataset.id = message.id;
      tools.append(reply);
      if (!incoming) {
        const status = document.createElement('small'); status.className = 'message-status';
        status.textContent = message.pending > 0 ? `待送达 ${message.pending} 人` : '已送达';
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

  render(): void {
    const name = this.root.querySelector<HTMLElement>('#group-name');
    const input = this.root.querySelector<HTMLTextAreaElement>('#group-input');
    if (name) name.textContent = this.name;
    if (input) input.value = this.drafts.get(this.groupId) ?? '';
    this.paintSubtitle();
    this.paintMembers();
    this.paintReplyBar();
    this.paintMentionPicker();
    this.paintSearchMembers();
    this.paintSearch();
    this.paint('bottom');
  }
}

function previewOf(body: string): string {
  const collapsed = body.split(/\s+/).filter(Boolean).join(' ');
  return collapsed.length > 60 ? `${collapsed.slice(0, 60)}…` : collapsed;
}
