import './style.css';
import { invoke, isTauri } from '@tauri-apps/api/core';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import { version as appVersion } from '../package.json';
import { DirectChat } from './direct_chat';
import { GroupChat } from './group_chat';

type Page = 'messages' | 'contacts' | 'discover' | 'settings' | 'about';
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
let manualAddress = '';
let probeNotice = isTauri() ? '探测服务未启动' : '仅桌面应用支持身份探测';
let probeMessage = '';
let remoteFingerprint = '';
let observedAddress = '';
let probeBusy = false;
let selectedFingerprint = '';
type Contact = { fingerprint: string; display_name: string; trust_state: string };
type Conversation = { id: string; last_body: string; last_logical: number; kind: string };
type Group = { id: string; name: string; owner: string; member_count: number; hidden: boolean; is_owner: boolean; last_body: string; last_logical: number };
let groups: Group[] = [];
type LocalMessage = { id: string; body: string; logical: number; created_at: number };
let selfOpen = false;
let selfMessages: LocalMessage[] = [];
let selfHasOlder = false;
let selfBusy = false;
let selfDraft = '';
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
    for (const conversation of conversations) {
      if (conversation.kind === 'group') {
        const group = groups.find(item => item.id === conversation.id);
        if (!group) continue;
        const button = document.createElement('button');
        button.className = `conversation-row${groupChat.groupId === conversation.id ? ' selected' : ''}`;
        button.dataset.action = 'open-group'; button.dataset.group = group.id; button.dataset.name = group.name;
        const name = document.createElement('strong'); name.textContent = group.name;
        const preview = document.createElement('span'); preview.textContent = conversation.last_body || `${group.member_count} 位成员`;
        button.append(name, preview); list.append(button);
        continue;
      }
      const contact = contacts.find(item => item.fingerprint === conversation.id && item.trust_state === 'verified');
      if (conversation.id !== 'self' && !contact) continue;
      const button = document.createElement('button');
      button.className = `conversation-row${conversation.id === 'self' ? (selfOpen ? ' selected' : '') : (direct.peer === conversation.id ? ' selected' : '')}`;
      button.dataset.action = conversation.id === 'self' ? 'open-self' : 'open-direct';
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
    const row = document.createElement('div'); row.className = 'message-row';
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
    <header class="chat-header"><div><strong>自己</strong><small>仅保存在本机</small></div><button class="outline" data-action="hide-self">不显示此会话</button></header>
    <div class="chat-history" id="self-history" aria-label="自聊记录"><button class="load-older" data-action="self-older" hidden>加载更早消息</button><div id="self-messages"></div></div>
    <div class="chat-compose"><label for="self-input">消息</label><textarea id="self-input" rows="3" maxlength="32768" placeholder="写点什么…"></textarea><div class="compose-footer"><span>Enter 发送 · Shift+Enter 换行</span><button data-action="self-send">发送</button></div><p id="self-error" role="status"></p></div>
  </section>`;
}

const pages: Record<Page, { title: string; eyebrow: string; heading: string; copy: string }> = {
  messages: { title: '消息', eyebrow: 'MESSAGES', heading: '还没有会话', copy: '从联系人打开自聊或已确认的单聊。' },
  contacts: { title: '联系人', eyebrow: 'CONTACTS', heading: '联系人', copy: '联系人管理功能正在开发中。' },
  discover: { title: '发现', eyebrow: 'DISCOVERY', heading: '局域网发现', copy: '仅浏览同网段 IPv4 mDNS 服务；发现结果未经身份验证，不代表可信联系人。' },
  settings: { title: '设置', eyebrow: 'PREFERENCES', heading: '设置', copy: '网络、隐私、备份和应用锁功能将在后续阶段开放。' },
  about: { title: '设置', eyebrow: 'ABOUT', heading: 'Qbird 青鸟', copy: '本地优先的局域网聊天工具。' }
};

function discoveryView(): string {
  return `<section class="discovery-panel card">
    <span class="eyebrow">DISCOVERY / 发现</span><h2>局域网身份探测</h2>
    <p>探测仅证明对方持有密钥，不会自动建立信任。双方须分别通过独立可靠渠道核对完整指纹。</p>
    <div class="probe-box"><label for="probe-port">本机监听端口</label><div class="probe-row"><input id="probe-port" type="number" min="1" max="65535" aria-label="本机监听端口"><button class="outline" data-action="probe-start">启动聊天和探测</button><button class="outline" data-action="probe-stop">停止</button></div>
    <p id="probe-status" aria-live="polite"></p><small>端口仅本次生效；启动后提供探测、聊天及待发消息重试。停止后待发消息保留在本机。</small></div>
    <div class="probe-box"><label for="probe-address">手动地址</label><div class="probe-row"><input id="probe-address" type="text" placeholder="192.168.1.8:42853" aria-label="手动地址"><button class="outline" data-action="probe-manual">探测指纹</button></div><p id="probe-message" aria-live="polite"></p>
    <p>本机指纹：<br><code id="local-fingerprint" class="fingerprint"></code></p><p>对方指纹（未确认）：<br><span id="remote-target"></span><br><code id="remote-fingerprint" class="fingerprint"></code></p><button class="outline" data-action="select-outbound">选择此指纹核对</button></div>
    <p id="discovery-status" aria-live="polite"></p><ul id="discovery-results" class="discovery-results"></ul>
    <h3>收到的探测请求（未确认）</h3><ul id="probe-requests" class="discovery-results"></ul>
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

async function refreshContacts(): Promise<void> {
  const status = app.querySelector<HTMLElement>('#contacts-status');
  const list = app.querySelector<HTMLUListElement>('#contacts-list');
  if (!status || !list) return;
  if (!isTauri()) { status.textContent = '仅桌面应用支持联系人'; return; }
  try {
    const [contacts, groupList] = await Promise.all([invoke<Contact[]>('v1_list_contacts'), invoke<Group[]>('v1_list_groups')]);
    groups = groupList;
    if (!list.isConnected) return;
    list.replaceChildren();
    status.textContent = contacts.length ? `其他联系人：${contacts.length}` : '暂无其他联系人，可前往发现页面核对指纹。';
    paintGroups(contacts);
    for (const contact of contacts) {
      const item = document.createElement('li');
      item.textContent = `${contact.display_name} · ${contact.trust_state === 'verified' ? '已确认' : '待确认'} · ${contact.fingerprint}`;
      if (contact.trust_state === 'verified') {
        const button = document.createElement('button'); button.className = 'outline'; button.textContent = '打开单聊';
        button.dataset.action = 'open-direct'; button.dataset.peer = contact.fingerprint; button.dataset.name = contact.display_name;
        item.append(button);
      }
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
  const current = pages[page];
  app.innerHTML = `
    <div class="shell">
      <aside class="rail" aria-label="主导航">
        <div class="profile" aria-label="我的资料">青</div>
        <nav class="rail-nav" aria-label="主要功能">
          <button data-page="messages" class="${page === 'messages' ? 'active' : ''}" aria-label="消息" title="消息">${icon('messages')}</button>
          <button data-page="contacts" class="${page === 'contacts' ? 'active' : ''}" aria-label="联系人" title="联系人">${icon('contacts')}</button>
          <button data-page="discover" class="${page === 'discover' ? 'active' : ''}" aria-label="发现" title="发现">${icon('discover')}</button>
        </nav>
        <span class="rail-brand">Qbird</span>
        <button class="rail-bottom" disabled aria-label="临时离开锁定（待实现）" title="临时离开锁定（待实现）">${icon('lock')}</button>
        <button class="rail-bottom" data-action="menu" aria-label="更多" aria-expanded="${menuOpen}" title="更多">${icon('menu')}</button>
      </aside>
      ${menuOpen ? '<div class="menu" role="menu"><button data-page="settings" role="menuitem">设置</button><button data-page="about" role="menuitem">关于</button><span>备份与还原 · 待实现</span></div>' : ''}
      <aside class="sidebar">
        <div class="team"><span class="team-mark">${icon('spark')}</span><div><strong>Qbird 青鸟</strong><small>LOCAL NETWORK MESSENGER</small></div></div>
        <div class="side-heading"><div><span class="eyebrow">${current.eyebrow}</span><h1>${current.title}</h1></div></div>
        ${page === 'settings' || page === 'about' ? `<div class="settings-links"><button data-page="settings" class="${page === 'settings' ? 'selected' : ''}">常规与网络</button><button data-page="about" class="${page === 'about' ? 'selected' : ''}">关于</button></div>` : page === 'messages' ? '<div id="conversation-list" class="conversation-list">正在读取…</div>' : '<div class="side-empty">暂无内容</div>'}
        <div class="side-foot"><span class="dot"></span>开发中 · 本地桌面版</div>
      </aside>
      <main class="workspace">
        ${page === 'messages' && groupChat.groupId ? groupChat.view() : page === 'messages' && direct.peer ? direct.view() : page === 'messages' && selfOpen ? selfChatView() : page === 'discover' ? discoveryView() : page === 'contacts' ? `<section class="discovery-panel card"><h2>联系人</h2><button class="self-contact" data-action="open-self" ${isTauri() ? '' : 'disabled'}><strong>自己</strong><span>打开本地自聊</span></button><p>已确认的联系人保存在本机；双方须各自核对指纹后才能聊天。</p><p id="contacts-status" aria-live="polite"></p><ul id="contacts-list" class="discovery-results"></ul><div class="probe-box"><h3>我的群聊</h3><ul id="groups-list" class="discovery-results"></ul><label for="group-name">新建群聊名称</label><input id="group-name" type="text" maxlength="128" placeholder="例如：项目组"><div id="group-members-pick" class="group-pick"></div><button class="outline" data-action="create-group">创建群聊</button><p id="group-create-status" aria-live="polite"></p><small>只能邀请已确认的联系人；新成员只能看到加入之后的消息。</small></div></section>` : page === 'about' ? `<section class="card about"><span class="eyebrow">ABOUT / 关于</span><h2>Qbird 青鸟</h2><p>面向局域网的本地优先聊天与文件交流工具。</p><dl><div><dt>当前阶段</dt><dd>Windows 开发版 ${appVersion}</dd></div><div><dt>许可证</dt><dd>Apache License 2.0</dd></div><div><dt>平台规划</dt><dd>Windows 首发 · 后续 Linux / macOS</dd></div></dl><p class="notice">开发版未经安全审计，本地数据库未加密。</p></section>` : `<section class="empty"><div class="empty-mark">${icon(page === 'settings' ? 'settings' : 'spark')}</div><h2>${current.heading}</h2><p>${current.copy}</p>${page === 'settings' ? `<p>本机身份指纹（SHA-256）：<br><code class="fingerprint">${fingerprint}</code></p><button data-page="about" class="outline">关于 Qbird</button>` : ''}</section>`}
      </main>
    </div>`;
  if (page === 'discover') paintDiscovery();
  if (page === 'messages') {
    void refreshConversations();
    if (direct.peer) direct.render();
    if (selfOpen) {
      const input = app.querySelector<HTMLTextAreaElement>('#self-input'); if (input) input.value = selfDraft;
      paintSelfMessages('bottom');
    }
  }
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
  if (target.id === 'probe-address') { manualAddress = target.value; remoteFingerprint = ''; observedAddress = ''; paintDiscovery(); }
  if (target.id === 'group-search-query') { groupChat.searchDraft('query', target.value); return; }
  if (target.id === 'group-search-member') { groupChat.searchDraft('member', target.value); return; }
  if (target.id === 'group-search-category') { groupChat.searchDraft('category', target.value); return; }
});
app.addEventListener('keydown', (event) => {
  if (event.target instanceof HTMLTextAreaElement && event.target.id === 'direct-input' && event.key === 'Enter' && !event.shiftKey && !event.ctrlKey && !event.altKey && !event.metaKey && !event.isComposing) {
    event.preventDefault(); void direct.send(); return;
  }
  if (event.target instanceof HTMLTextAreaElement && event.target.id === 'group-input' && event.key === 'Enter' && !event.shiftKey && !event.ctrlKey && !event.altKey && !event.metaKey && !event.isComposing) {
    event.preventDefault(); void groupChat.send(); return;
  }
  if (event.target instanceof HTMLTextAreaElement && event.target.id === 'self-input' && event.key === 'Enter' && !event.shiftKey && !event.ctrlKey && !event.altKey && !event.metaKey && !event.isComposing) {
    event.preventDefault(); void sendSelf();
  }
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
  if (action?.dataset.action === 'save-direct-address') { void direct.saveAddress(); return; }
  if (action?.dataset.action === 'group-send') { void groupChat.send(); return; }
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
  if (action?.dataset.action === 'group-reply' && action.dataset.id) { groupChat.quote(action.dataset.id); return; }
  if (action?.dataset.action === 'group-cancel-reply') { groupChat.cancelQuote(); return; }
  if (action?.dataset.action === 'group-jump' && action.dataset.id) { void groupChat.jumpTo(action.dataset.id); return; }
  if (action?.dataset.action === 'group-mention' && action.dataset.member) { groupChat.selectMention(action.dataset.member); return; }
  if (action?.dataset.action === 'group-transfer-owner' && action.dataset.member) { void groupChat.transferOwner(action.dataset.member); return; }
  if (action?.dataset.action === 'group-dissolve') { void groupChat.dissolve(); return; }
  const nav = target.closest<HTMLButtonElement>('button[data-page]');
  if (nav?.dataset.page && nav.dataset.page in pages) {
    page = nav.dataset.page as Page;
    menuOpen = false;
    render();
    if (page === 'discover') { void refreshDiscovery(); void refreshProbeStatus(); }
    if (page === 'contacts') void refreshContacts();
  }
});
render();
app.addEventListener('conversations-changed', () => { void refreshConversations(); });
window.setInterval(() => {
  void refreshDiscovery(); void refreshProbeStatus();
  if (page === 'messages') { void refreshConversations(); if (direct.peer) void direct.load(); if (groupChat.groupId) void groupChat.load(); }
}, 4000);
if (isTauri()) {
  // Dragging files onto the window provides real local paths, so no extra permission is needed.
  void getCurrentWebview().onDragDropEvent((event) => {
    if (event.payload.type !== 'drop' || !direct.peer) return;
    const paths = event.payload.paths.filter(path => path.length <= 4096);
    if (paths.length) void direct.queuePaths(paths);
  }).catch(() => undefined);
  invoke<string>('identity_fingerprint').then((value) => {
    fingerprint = /^[0-9a-f]{64}$/.test(value) ? value : '无法读取';
    if (page === 'settings') render(); else if (page === 'discover') paintDiscovery();
  }).catch(() => {
    fingerprint = '无法读取';
    if (page === 'settings') render(); else if (page === 'discover') paintDiscovery();
  });
}
