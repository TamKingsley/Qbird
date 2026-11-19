import { invoke, isTauri } from '@tauri-apps/api/core';

type GroupMessage = { id: string; group_id: string; sender: string; sender_name: string; body: string; created_at: number; logical: number; pending: number };
type GroupMember = { group_id: string; member: string; display_name: string; role: string; joined_at: number; active: boolean };
type Contact = { fingerprint: string; display_name: string; trust_state: string };
type Cursor = Pick<GroupMessage, 'logical' | 'sender' | 'id'>;
const cursorOf = (message: GroupMessage): Cursor => ({ logical: message.logical, sender: message.sender, id: message.id });
const compare = (a: Cursor, b: Cursor): number => a.logical - b.logical || (a.sender < b.sender ? -1 : a.sender > b.sender ? 1 : a.id < b.id ? -1 : a.id > b.id ? 1 : 0);

/// Group chat: outbound messages are queued per member by the backend, so the UI only
/// shows the pending count and never claims delivery on its own.
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

  constructor(private root: HTMLElement, private onChange: () => void) {}

  view(): string {
    return `<section class="self-chat" aria-label="群聊">
      <header class="chat-header"><div><strong id="group-name"></strong><small id="group-subtitle">群聊</small></div><div class="group-actions"><button class="outline" data-action="toggle-group-members">成员</button><button class="outline" data-action="hide-group">不显示此会话</button><button class="outline" data-action="leave-group">退出群聊</button></div></header>
      <div class="chat-history" id="group-history" aria-label="群聊记录"><div class="group-members" id="group-members" hidden></div><button class="load-older" data-action="group-older" hidden>加载更早消息</button><div id="group-messages"></div></div>
      <div class="chat-compose"><label for="group-input">消息</label><textarea id="group-input" rows="3" maxlength="32768" placeholder="发消息给群里的人…"></textarea><div class="compose-footer"><span>Enter 发送 · Shift+Enter 换行</span><button data-action="group-send">发送</button></div><p id="group-error" role="status"></p></div>
    </section>`;
  }

  clear(): void { this.groupId = ''; this.generation++; this.membersOpen = false; }

  draft(value: string): void { if (this.groupId) this.drafts.set(this.groupId, value); }

  async open(groupId: string, name: string): Promise<void> {
    if (!isTauri()) return;
    const generation = ++this.generation;
    await invoke('v1_open_group', { groupId });
    if (generation !== this.generation) return;
    this.groupId = groupId; this.name = name; this.messages = []; this.hasOlder = false;
    this.refreshBefore = null; this.loading = false; this.membersOpen = false;
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
      const changed = next.length !== this.messages.length || next.some((item, i) => item.id !== this.messages[i]?.id || item.pending !== this.messages[i]?.pending);
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
    } catch { this.status('#group-error', '无法读取群成员'); }
  }

  async send(): Promise<void> {
    const input = this.root.querySelector<HTMLTextAreaElement>('#group-input');
    const button = this.root.querySelector<HTMLButtonElement>('button[data-action="group-send"]');
    if (!this.groupId || !input || this.sending) return;
    const groupId = this.groupId, generation = this.generation, body = input.value;
    if (!body.trim()) { this.status('#group-error', '请输入消息'); return; }
    this.sending = true; if (button) button.disabled = true; this.status('#group-error', '');
    try {
      const message = await invoke<GroupMessage>('v1_queue_group', { groupId, body });
      if (this.drafts.get(groupId) === body) this.drafts.set(groupId, '');
      if (generation === this.generation) {
        if (input.isConnected && input.value === body) input.value = '';
        if (!this.messages.some(item => item.id === message.id)) { this.messages.push(message); this.messages.sort(compare); }
        this.paint('bottom'); this.dispatchChanged();
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

  toggleMembers(): void { this.membersOpen = !this.membersOpen; this.paintMembers(); }

  private dispatchChanged(): void { this.root.dispatchEvent(new CustomEvent('conversations-changed')); }

  private status(selector: string, value: string): void {
    const target = this.root.querySelector<HTMLElement>(selector); if (target) target.textContent = value;
  }

  private paintSubtitle(): void {
    const subtitle = this.root.querySelector<HTMLElement>('#group-subtitle');
    if (subtitle) subtitle.textContent = `${this.members.filter(member => member.active).length} 位成员 · 点对点加密 · 无中转`;
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
        const remove = document.createElement('button');
        remove.className = 'outline'; remove.textContent = '移除';
        remove.dataset.action = 'remove-group-member'; remove.dataset.member = member.member;
        row.append(remove);
      }
      box.append(row);
    }
    this.paintAddMember(box);
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
      const column = document.createElement('div'); column.className = 'message-column';
      if (incoming) {
        const who = document.createElement('small'); who.className = 'message-sender';
        who.textContent = message.sender_name; column.append(who);
      }
      const body = document.createElement('div'); body.className = 'message-bubble'; body.textContent = message.body;
      column.append(body);
      if (!incoming) {
        const status = document.createElement('small'); status.className = 'message-status';
        status.textContent = message.pending > 0 ? `待送达 ${message.pending} 人` : '已送达';
        column.append(status);
      }
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
    this.paint('bottom');
  }
}
