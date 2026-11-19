import { invoke, isTauri } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';

type DirectMessage = {
  id: string; peer: string; sender: string; body: string; created_at: number; logical: number;
  status: 'pending' | 'delivered' | 'received' | 'failed';
  kind: 'text' | 'file';
  file_name: string | null; file_size: number | null; file_sha256: string | null;
  file_status: 'pending' | 'transferring' | 'completed' | 'failed' | null; file_transferred: number | null; file_error: string | null;
};
type TextPreview = { name: string; kind: string; text: string };
type Panel = { id: string; name: string; kind: string };
type Cursor = Pick<DirectMessage, 'logical' | 'sender' | 'id'>;
const cursorOf = (message: DirectMessage): Cursor => ({ logical: message.logical, sender: message.sender, id: message.id });
const compare = (a: Cursor, b: Cursor): number => a.logical - b.logical || (a.sender < b.sender ? -1 : a.sender > b.sender ? 1 : a.id < b.id ? -1 : a.id > b.id ? 1 : 0);
const POLL_INTERVAL = 1500;

export class DirectChat {
  peer = '';
  private name = '';
  private messages: DirectMessage[] = [];
  private hasOlder = false;
  private loading = false;
  private sending = false;
  private attaching = false;
  private generation = 0;
  private address = '';
  private drafts = new Map<string, string>();
  private addressDrafts = new Map<string, string>();
  private refreshBefore: Cursor | null = null;
  private previews = new Map<string, string>();
  private previewPending = new Set<string>();
  private poll: number | null = null;
  private panel: Panel | null = null;
  private panelWidth = 380;
  private panelBusy = false;

  constructor(private root: HTMLElement, private onChange: () => void) {
    // The resizer is re-created on every render, so a single delegated listener is enough.
    root.addEventListener('pointerdown', (event) => this.startResize(event));
  }

  view(): string {
    return `<section class="self-chat" aria-label="单聊">
      <header class="chat-header"><div><strong id="direct-name"></strong><small>端到端加密 · 指纹已确认</small></div><button class="outline" data-action="hide-direct">不显示此会话</button></header>
      <div class="chat-history" id="direct-history" aria-label="单聊记录"><div class="direct-address"><label for="direct-address">联系人地址（仅连接提示）</label><div class="probe-row"><input id="direct-address" type="text" placeholder="192.168.1.8:42853"><button class="outline" data-action="save-direct-address">保存地址</button></div><small>地址不代表身份，连接时仍核对指纹。请在发现页面启动聊天服务；离线消息与文件须双方同时在线才能补发或续传。</small><p id="direct-address-status" role="status"></p></div><button class="load-older" data-action="direct-older" hidden>加载更早消息</button><div id="direct-messages"></div></div>
      <div class="chat-compose"><label for="direct-input">消息</label><textarea id="direct-input" rows="3" maxlength="32768" placeholder="写点什么，或把文件/图片拖进窗口…"></textarea><div class="compose-footer"><span>Enter 发送 · Shift+Enter 换行</span><div class="compose-actions"><button class="outline" data-action="direct-image">发送图片</button><button class="outline" data-action="direct-file">发送文件</button><button data-action="direct-send">发送</button></div></div><p id="direct-file-status" role="status"></p><p id="direct-error" role="status"></p></div>
      <aside class="file-panel" id="direct-panel" hidden aria-label="文件预览">
        <div class="panel-resizer" data-role="panel-resizer" aria-hidden="true"></div>
        <header class="panel-header"><strong id="panel-title"></strong><div class="panel-actions"><button class="outline" data-action="panel-reveal">打开所在目录</button><button class="outline" data-action="panel-close">关闭</button></div></header>
        <div class="panel-body" id="panel-body"></div>
      </aside>
      <div class="lightbox" id="direct-lightbox" hidden></div>
    </section>`;
  }

  clear(): void { this.peer = ''; this.generation++; this.stopPolling(); this.closePanel(); }

  draft(value: string): void { if (this.peer) this.drafts.set(this.peer, value); }
  addressDraft(value: string): void { if (this.peer) this.addressDrafts.set(this.peer, value); }

  async hide(): Promise<void> {
    if (!this.peer) return;
    const peer = this.peer, generation = this.generation;
    try {
      await invoke('v1_hide_direct', { peer });
      if (generation === this.generation) { this.clear(); this.onChange(); }
    } catch { if (generation === this.generation) this.status('#direct-error', '无法隐藏会话'); }
  }

  async saveAddress(): Promise<void> {
    const input = this.root.querySelector<HTMLInputElement>('#direct-address');
    if (!this.peer || !input) return;
    const peer = this.peer, generation = this.generation, address = input.value.trim();
    try {
      await invoke('v1_set_contact_address', { peer, address });
      if (generation === this.generation) {
        this.address = address; if (input.value.trim() === address) this.addressDrafts.delete(peer);
        this.status('#direct-address-status', '地址已保存；身份仍以指纹验证。');
      }
    } catch (error) {
      if (generation === this.generation) this.status('#direct-address-status', typeof error === 'string' ? error : '无法保存地址');
    }
  }

  async send(): Promise<void> {
    const input = this.root.querySelector<HTMLTextAreaElement>('#direct-input');
    const button = this.root.querySelector<HTMLButtonElement>('button[data-action="direct-send"]');
    if (!this.peer || !input || this.sending) return;
    const peer = this.peer, generation = this.generation, body = input.value;
    if (!body.trim()) { this.status('#direct-error', '请输入消息'); return; }
    this.sending = true; if (button) button.disabled = true; this.status('#direct-error', '');
    try {
      const message = await invoke<DirectMessage>('v1_queue_direct', { peer, body });
      if (this.drafts.get(peer) === body) this.drafts.set(peer, '');
      if (generation === this.generation) {
        if (input.isConnected && input.value === body) input.value = '';
        if (!this.messages.some(item => item.id === message.id)) { this.messages.push(message); this.messages.sort(compare); }
        this.paint('bottom'); this.onChangeConversations();
      }
    } catch (error) {
      if (generation === this.generation) this.status('#direct-error', typeof error === 'string' ? error : '无法保存待发送消息');
    } finally { this.sending = false; if (button?.isConnected) button.disabled = false; }
  }

  /// Pick a local file with the system dialog; the backend keeps the path and reads the bytes.
  async pick(kind: 'image' | 'file'): Promise<void> {
    if (!isTauri() || !this.peer || this.attaching) return;
    this.attaching = true;
    try {
      const selected = await open({
        multiple: kind === 'file',
        directory: false,
        title: kind === 'image' ? '选择图片' : '选择文件',
        filters: kind === 'image' ? [{ name: '图片', extensions: ['png', 'jpg', 'jpeg', 'gif', 'webp', 'bmp'] }] : undefined,
      });
      if (!selected) return;
      await this.queuePaths(Array.isArray(selected) ? selected : [selected]);
    } catch (error) {
      this.status('#direct-error', typeof error === 'string' ? error : '无法打开文件选择器');
    } finally { this.attaching = false; }
  }

  /// Files dropped onto the window are queued in arrival order.
  async queuePaths(paths: string[]): Promise<void> {
    if (!this.peer || this.attaching) return;
    this.attaching = true;
    try {
      for (const path of paths) await this.queuePath(path);
    } finally { this.attaching = false; }
  }

  private async queuePath(path: string): Promise<void> {
    const peer = this.peer, generation = this.generation;
    this.status('#direct-file-status', '正在计算校验值…');
    try {
      const message = await invoke<DirectMessage>('v1_queue_direct_file', { peer, path });
      if (generation !== this.generation) return;
      if (!this.messages.some(item => item.id === message.id)) { this.messages.push(message); this.messages.sort(compare); }
      this.paint('bottom'); this.onChangeConversations();
      this.status('#direct-file-status', `已加入待发送：${message.file_name ?? message.body}（对方在线后自动发送）`);
    } catch (error) {
      if (generation === this.generation) this.status('#direct-file-status', typeof error === 'string' ? error : '无法加入文件');
    }
  }

  /// Show one completed attachment in the right workspace panel. Images reuse the
  /// cached data URL; text-like files are fetched with a bounded backend read.
  async openPreview(message: DirectMessage): Promise<void> {
    if (!isTauri() || message.kind !== 'file' || message.file_status !== 'completed') return;
    const name = message.file_name ?? message.body;
    const kind = previewKind(name);
    this.panel = { id: message.id, name, kind };
    this.panelBusy = true;
    this.paintPanel('正在读取…');
    try {
      if (kind === 'image') {
        const preview = await invoke<{ name: string; data_url: string } | null>('v1_read_attachment', { messageId: message.id });
        if (!preview) { this.paintPanel('图片过大或无法读取，未能预览'); return; }
        this.previews.set(message.id, preview.data_url);
        this.paintPanel('', preview.data_url);
        return;
      }
      if (kind === 'other') { this.paintPanel('该类型暂不支持预览，可使用“打开所在目录”。'); return; }
      const preview = await invoke<TextPreview | null>('v1_read_text_attachment', { messageId: message.id });
      if (!preview) { this.paintPanel('文件过大、不是文本或无法读取，未能预览'); return; }
      this.paintPanel('', preview.text);
    } catch (error) {
      this.paintPanel(typeof error === 'string' ? error : '无法读取附件');
    } finally { this.panelBusy = false; }
  }

  get panelId(): string { return this.panel?.id ?? ''; }

  openPreviewById(id: string): void {
    const message = this.messages.find(item => item.id === id);
    if (message) void this.openPreview(message);
  }

  closePanel(): void { this.panel = null; const panel = this.root.querySelector<HTMLElement>('#direct-panel'); if (panel) panel.hidden = true; }

  async reveal(messageId: string): Promise<void> {
    if (!isTauri()) return;
    try { await invoke('v1_reveal_attachment', { messageId }); }
    catch (error) { this.status('#direct-file-status', typeof error === 'string' ? error : '无法打开文件所在目录'); }
  }

  private paintPanel(message: string, content?: string): void {
    const panel = this.root.querySelector<HTMLElement>('#direct-panel');
    const title = this.root.querySelector<HTMLElement>('#panel-title');
    const body = this.root.querySelector<HTMLElement>('#panel-body');
    if (!panel || !title || !body) return;
    if (!this.panel) { panel.hidden = true; return; }
    panel.hidden = false;
    panel.style.width = `${this.panelWidth}px`;
    title.textContent = this.panel.name;
    body.replaceChildren();
    if (!content) {
      const note = document.createElement('p'); note.className = 'panel-note'; note.textContent = message;
      body.append(note);
      return;
    }
    if (this.panel.kind === 'image') {
      const image = document.createElement('img'); image.src = content; image.alt = this.panel.name;
      body.append(image);
      return;
    }
    if (this.panel.kind === 'markdown') {
      const article = document.createElement('article'); article.className = 'markdown-body';
      article.innerHTML = markdownToHtml(content);
      body.append(article);
      return;
    }
    if (this.panel.kind === 'html') {
      const frame = document.createElement('iframe');
      frame.className = 'html-preview';
      frame.setAttribute('sandbox', '');
      frame.setAttribute('referrerpolicy', 'no-referrer');
      frame.srcdoc = content;
      body.append(frame);
      const note = document.createElement('p'); note.className = 'panel-note';
      note.textContent = 'HTML 在禁用脚本的沙箱中预览；页面样式可能受应用安全策略限制。';
      body.append(note);
      return;
    }
    const pre = document.createElement('pre'); pre.className = 'text-preview'; pre.textContent = content;
    body.append(pre);
  }

  private startResize(event: PointerEvent): void {
    const target = event.target;
    if (!(target instanceof HTMLElement) || target.dataset.role !== 'panel-resizer') return;
    event.preventDefault();
    const startX = event.clientX, startWidth = this.panelWidth;
    const move = (moveEvent: PointerEvent) => {
      this.panelWidth = Math.min(window.innerWidth * 0.7, Math.max(280, startWidth + (startX - moveEvent.clientX)));
      const panel = this.root.querySelector<HTMLElement>('#direct-panel');
      if (panel) panel.style.width = `${this.panelWidth}px`;
    };
    const stop = () => {
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', stop);
    };
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', stop);
  }

  private onChangeConversations(): void { this.root.dispatchEvent(new CustomEvent('conversations-changed')); }

  async open(peer: string, name: string): Promise<void> {
    if (!isTauri()) return;
    this.stopPolling();
    const generation = ++this.generation;
    await invoke('v1_open_direct', { peer });
    if (generation !== this.generation) return;
    this.peer = peer; this.name = name; this.messages = []; this.address = ''; this.hasOlder = false; this.refreshBefore = null;
    this.loading = false; this.previews.clear(); this.previewPending.clear(); this.onChange();
    try {
      const address = await invoke<string | null>('v1_contact_address', { peer });
      if (generation === this.generation) {
        this.address = address ?? '';
        const input = this.root.querySelector<HTMLInputElement>('#direct-address');
        if (input && document.activeElement !== input) input.value = this.addressDrafts.get(peer) ?? this.address;
      }
    } catch { this.status('#direct-address-status', '无法读取联系人地址'); }
    await this.load(false, true);
    this.startPolling();
  }

  private startPolling(): void {
    this.stopPolling();
    this.poll = window.setInterval(() => { if (!document.hidden) void this.load(false); }, POLL_INTERVAL);
  }

  private stopPolling(): void { if (this.poll !== null) { window.clearInterval(this.poll); this.poll = null; } }

  async load(older = false, initial = false): Promise<void> {
    if (!isTauri() || !this.peer || this.loading) return;
    void this.refreshAddress();
    const peer = this.peer, generation = this.generation;
    const first = this.messages[0];
    const before: Cursor | null = older && first ? cursorOf(first) : null;
    if (older && !before) return;
    this.loading = true;
    try {
      const batch = await invoke<DirectMessage[]>('v1_list_direct', { peer, limit: 200, before });
      if (generation !== this.generation || peer !== this.peer) return;
      let historical: DirectMessage[] = [];
      if (!older && !initial && this.refreshBefore && first) {
        historical = await invoke<DirectMessage[]>('v1_list_direct', { peer, limit: 200, before: this.refreshBefore });
        if (generation !== this.generation || peer !== this.peer) return;
      }
      if (!older) {
        const tail = (historical.length ? historical : batch).at(-1);
        this.refreshBefore = first && tail && compare(first, tail) < 0 ? cursorOf(tail) : null;
      }
      const merged = new Map(this.messages.map(message => [message.id, message]));
      for (const message of [...batch, ...historical]) merged.set(message.id, message);
      const next = [...merged.values()].sort(compare);
      const changed = !sameMessages(next, this.messages);
      this.messages = next;
      if (older || initial) this.hasOlder = batch.length === 200;
      if (changed || older || initial) this.paint(initial ? 'bottom' : older ? 'preserve' : 'auto');
    } catch { if (generation === this.generation) this.status('#direct-error', '无法读取单聊记录'); }
    finally { if (generation === this.generation) this.loading = false; }
  }

  private async refreshAddress(): Promise<void> {
    const peer = this.peer, generation = this.generation;
    try {
      const address = await invoke<string | null>('v1_contact_address', { peer });
      if (generation !== this.generation || peer !== this.peer) return;
      this.address = address ?? '';
      const input = this.root.querySelector<HTMLInputElement>('#direct-address');
      if (input && document.activeElement !== input && !this.addressDrafts.has(peer)) input.value = this.address;
    } catch { /* Keep the last displayed address until the next refresh. */ }
  }

  private status(selector: string, value: string): void {
    const target = this.root.querySelector<HTMLElement>(selector); if (target) target.textContent = value;
  }

  /// Image previews are fetched once per message and only after the transfer completed.
  private ensurePreview(message: DirectMessage): void {
    if (message.kind !== 'file' || message.file_status !== 'completed' || !message.file_name) return;
    if (this.previews.has(message.id) || this.previewPending.has(message.id)) return;
    if (!/\.(png|jpe?g|gif|webp|bmp)$/i.test(message.file_name)) return;
    this.previewPending.add(message.id);
    void invoke<{ name: string; data_url: string } | null>('v1_read_attachment', { messageId: message.id }).then(preview => {
      this.previewPending.delete(message.id);
      if (!preview || this.peer !== message.peer) return;
      this.previews.set(message.id, preview.data_url);
      this.paintPreview(message.id);
    }).catch(() => this.previewPending.delete(message.id));
  }

  private paintPreview(id: string): void {
    const url = this.previews.get(id);
    const holder = this.root.querySelector<HTMLElement>(`[data-preview="${id}"]`);
    if (!url || !holder || holder.firstChild) return;
    const image = document.createElement('img');
    image.src = url; image.alt = '聊天图片'; image.className = 'message-image';
    image.addEventListener('click', () => this.openLightbox(url));
    holder.append(image);
  }

  private openLightbox(url: string): void {
    const box = this.root.querySelector<HTMLElement>('#direct-lightbox');
    if (!box) return;
    box.replaceChildren();
    const image = document.createElement('img'); image.src = url; image.alt = '图片预览';
    box.append(image);
    box.hidden = false;
    box.onclick = () => { box.hidden = true; box.replaceChildren(); };
  }

  private paint(mode: 'bottom' | 'preserve' | 'auto'): void {
    const history = this.root.querySelector<HTMLElement>('#direct-history');
    const list = this.root.querySelector<HTMLElement>('#direct-messages');
    const older = this.root.querySelector<HTMLButtonElement>('button[data-action="direct-older"]');
    if (!history || !list || !older) return;
    const atBottom = history.scrollHeight - history.clientHeight - history.scrollTop < 40;
    const top = history.getBoundingClientRect().top;
    const anchor = [...list.children].find(node => node.getBoundingClientRect().bottom > top) as HTMLElement | undefined;
    const anchorId = anchor?.dataset.id, anchorOffset = anchor ? anchor.getBoundingClientRect().top - top : 0;
    const previousTop = history.scrollTop, previousHeight = history.scrollHeight;
    list.replaceChildren();
    for (const message of this.messages) {
      const incoming = message.sender === this.peer;
      const row = document.createElement('div'); row.className = `message-row${incoming ? ' incoming' : ''}`; row.dataset.id = message.id;
      const column = document.createElement('div'); column.className = 'message-column';
      if (message.kind === 'file') {
        const bubble = document.createElement('div'); bubble.className = 'message-bubble file-bubble';
        const name = document.createElement('div'); name.className = 'file-title'; name.textContent = message.file_name ?? message.body;
        bubble.append(name);
        const preview = document.createElement('div'); preview.className = 'file-preview'; preview.dataset.preview = message.id;
        bubble.append(preview);
        const meta = document.createElement('small'); meta.className = 'file-meta'; meta.textContent = fileMeta(message, incoming);
        bubble.append(meta);
        if (message.file_status === 'completed') {
          const actions = document.createElement('div'); actions.className = 'file-actions';
          const preview = document.createElement('button');
          preview.className = 'linkish'; preview.textContent = '预览';
          preview.dataset.action = 'direct-preview'; preview.dataset.id = message.id;
          const reveal = document.createElement('button');
          reveal.className = 'linkish'; reveal.textContent = '打开所在目录';
          reveal.dataset.action = 'direct-reveal'; reveal.dataset.id = message.id;
          actions.append(preview, reveal);
          bubble.append(actions);
        }
        column.append(bubble);
        this.ensurePreview(message);
      } else {
        const body = document.createElement('div'); body.className = 'message-bubble'; body.textContent = message.body;
        column.append(body);
      }
      if (!incoming) {
        const status = document.createElement('small'); status.className = 'message-status';
        status.textContent = message.kind === 'file' ? '' : message.status === 'delivered' ? '已送达' : '待发送';
        if (status.textContent) column.append(status);
      }
      row.append(column); list.append(row);
    }
    older.hidden = !this.hasOlder;
    if (mode === 'bottom' || (mode === 'auto' && atBottom)) history.scrollTop = history.scrollHeight;
    else {
      const restored = [...list.children].find(node => (node as HTMLElement).dataset.id === anchorId) as HTMLElement | undefined;
      history.scrollTop = restored ? previousTop + restored.getBoundingClientRect().top - top - anchorOffset : previousTop + history.scrollHeight - previousHeight;
    }
    for (const id of this.previews.keys()) this.paintPreview(id);
  }

  render(): void {
    const name = this.root.querySelector<HTMLElement>('#direct-name');
    const input = this.root.querySelector<HTMLTextAreaElement>('#direct-input');
    const address = this.root.querySelector<HTMLInputElement>('#direct-address');
    if (name) name.textContent = this.name;
    if (input) input.value = this.drafts.get(this.peer) ?? '';
    if (address) address.value = this.addressDrafts.get(this.peer) ?? this.address;
    this.paint('bottom');
  }
}

function previewKind(name: string): 'image' | 'html' | 'markdown' | 'text' | 'other' {
  const extension = name.slice(name.lastIndexOf('.') + 1).toLowerCase();
  if (['png', 'jpg', 'jpeg', 'gif', 'webp', 'bmp'].includes(extension)) return 'image';
  if (['html', 'htm'].includes(extension)) return 'html';
  if (['md', 'markdown'].includes(extension)) return 'markdown';
  if (['txt', 'log', 'csv', 'json', 'xml', 'ini', 'toml', 'yaml', 'yml'].includes(extension)) return 'text';
  return 'other';
}

function escapeHtml(value: string): string {
  return value.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
}

/// Deliberately small Markdown renderer: only the structures a chat attachment needs.
/// Everything is escaped first, so file content cannot inject markup or scripts.
function markdownToHtml(source: string): string {
  const inline = (text: string): string => escapeHtml(text)
    .replace(/`([^`]+)`/g, '<code>$1</code>')
    .replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>')
    .replace(/(^|[^*])\*([^*]+)\*/g, '$1<em>$2</em>')
    .replace(/\[([^\]]+)\]\(([^)]+)\)/g, '$1（$2）');
  const lines = source.split(/\r?\n/);
  const html: string[] = [];
  let code: string[] | null = null;
  let list: 'ul' | 'ol' | null = null;
  const closeList = () => { if (list) { html.push(`</${list}>`); list = null; } };
  for (const line of lines) {
    if (line.trimStart().startsWith('```')) {
      if (code) { html.push(`<pre><code>${escapeHtml(code.join('\n'))}</code></pre>`); code = null; }
      else code = [];
      continue;
    }
    if (code) { code.push(line); continue; }
    const heading = /^(#{1,6})\s+(.*)$/.exec(line);
    if (heading) { closeList(); html.push(`<h${heading[1].length}>${inline(heading[2])}</h${heading[1].length}>`); continue; }
    if (/^\s*[-*]\s+/.test(line)) {
      if (list !== 'ul') { closeList(); html.push('<ul>'); list = 'ul'; }
      html.push(`<li>${inline(line.replace(/^\s*[-*]\s+/, ''))}</li>`); continue;
    }
    if (/^\s*\d+\.\s+/.test(line)) {
      if (list !== 'ol') { closeList(); html.push('<ol>'); list = 'ol'; }
      html.push(`<li>${inline(line.replace(/^\s*\d+\.\s+/, ''))}</li>`); continue;
    }
    closeList();
    if (!line.trim()) continue;
    if (line.trimStart().startsWith('>')) { html.push(`<blockquote>${inline(line.replace(/^\s*>\s?/, ''))}</blockquote>`); continue; }
    html.push(`<p>${inline(line)}</p>`);
  }
  closeList();
  if (code) html.push(`<pre><code>${escapeHtml(code.join('\n'))}</code></pre>`);
  return html.join('');
}

function formatSize(bytes: number | null): string {
  if (!bytes || bytes < 0) return '未知大小';
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
  return `${(bytes / 1024 / 1024 / 1024).toFixed(2)} GB`;
}

function fileMeta(message: DirectMessage, incoming: boolean): string {
  const size = formatSize(message.file_size);
  const percent = message.file_size ? Math.min(100, Math.round(((message.file_transferred ?? 0) / message.file_size) * 100)) : 0;
  const label = (() => {
    switch (message.file_status) {
      case 'completed': return incoming ? '已接收' : '已发送';
      case 'transferring': return `${incoming ? '接收中' : '发送中'} ${percent}%`;
      case 'failed': return `${incoming ? '接收中断' : '发送失败'}${message.file_error ? `：${message.file_error}` : ' · 重新连接后可续传'}`;
      default: return incoming ? '等待接收' : '等待对方在线';
    }
  })();
  return `${size} · ${label}`;
}

function sameMessages(a: DirectMessage[], b: DirectMessage[]): boolean {
  if (a.length !== b.length) return false;
  return a.every((item, index) => {
    const other = b[index];
    return other && item.id === other.id && item.status === other.status
      && item.file_status === other.file_status && item.file_transferred === other.file_transferred;
  });
}
