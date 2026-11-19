import { invoke, isTauri } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { showImage } from './image_viewer';
import { base64ToBytes, createPluginFrame } from './plugin_view';
import { showContextMenu } from './context_menu';

type DirectMessage = {
  id: string; peer: string; sender: string; body: string; created_at: number; logical: number;
  status: 'pending' | 'delivered' | 'received' | 'failed';
  kind: 'text' | 'file';
  file_name: string | null; file_size: number | null; file_sha256: string | null;
  file_status: 'pending' | 'transferring' | 'completed' | 'failed' | null; file_transferred: number | null; file_error: string | null;
};
type TextPreview = { name: string; kind: string; text: string };
type Panel = { id: string; name: string; kind: string };
type FileOffer = { message_id: string; peer: string; name: string; total: number; created_at: number; status: string };
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
  private offers: FileOffer[] = [];
  private batches = new Map<string, string>();
  private batchCaptions = new Map<string, string>();
  private renderedBatches = new Set<string>();
  private panelBytes: Uint8Array | null = null;
  /// 'lightbox' keeps the in-app viewer; 'workspace' opens the right panel instead.
  imageOpenMode: 'lightbox' | 'workspace' = 'lightbox';

  constructor(private root: HTMLElement, private onChange: () => void) {
    // The resizer is re-created on every render, so a single delegated listener is enough.
    root.addEventListener('pointerdown', (event) => this.startResize(event));
  }

  view(): string {
    return `<section class="self-chat" aria-label="单聊">
      <header class="chat-header"><div><strong id="direct-name"></strong><small>端到端加密 · 指纹已确认</small></div><div class="group-actions"><button class="outline" data-action="export-text" data-kind="direct">导出文本</button><button class="outline" data-action="hide-direct">不显示此会话</button></div></header>
      <div class="chat-history" id="direct-history" aria-label="单聊记录"><div class="direct-address"><label for="direct-address">联系人地址（仅连接提示）</label><div class="probe-row"><input id="direct-address" type="text" placeholder="192.168.1.8:42853"><button class="outline" data-action="save-direct-address">保存地址</button></div><small>地址不代表身份，连接时仍核对指纹。请在发现页面启动聊天服务；离线消息与文件须双方同时在线才能补发或续传。</small><p id="direct-address-status" role="status"></p></div><button class="load-older" data-action="direct-older" hidden>加载更早消息</button><div id="direct-messages"></div><div id="direct-offers" class="direct-offers" hidden></div></div>
      <div class="chat-compose"><label for="direct-input">消息</label><textarea id="direct-input" rows="3" maxlength="32768" placeholder="写点什么，或把文件/图片拖进窗口…"></textarea><div class="compose-footer"><span>Enter 发送 · Shift+Enter 换行</span><div class="compose-actions"><button class="outline" data-action="direct-image">发送图片</button><button class="outline" data-action="direct-file">发送文件</button><button class="outline" data-action="direct-screenshot">截图</button><button data-action="direct-send">发送</button></div></div><p id="direct-file-status" role="status"></p><p id="direct-error" role="status"></p></div>
      <aside class="file-panel" id="direct-panel" hidden aria-label="文件预览">
        <div class="panel-resizer" data-role="panel-resizer" aria-hidden="true"></div>
        <header class="panel-header"><strong id="panel-title"></strong><div class="panel-actions"><button class="outline" data-action="panel-reveal">打开所在目录</button><button class="outline" data-action="panel-close">关闭</button></div></header>
        <div class="panel-body" id="panel-body"></div>
      </aside>
    </section>`;
  }

  clear(): void { this.peer = ''; this.generation++; this.offers = []; this.stopPolling(); this.closePanel(); }

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
        multiple: true,
        directory: false,
        title: kind === 'image' ? '选择图片' : '选择文件',
        filters: kind === 'image' ? [{ name: '图片', extensions: ['png', 'jpg', 'jpeg', 'gif', 'webp', 'bmp'] }] : undefined,
      });
      if (!selected) return;
      const paths = Array.isArray(selected) ? selected : [selected];
      if (kind === 'image') await this.queueImages(paths);
      else await this.queuePaths(paths);
    } catch (error) {
      this.status('#direct-error', typeof error === 'string' ? error : '无法打开文件选择器');
    } finally { this.attaching = false; }
  }

  /// One bubble for the selected images, using the composer text as its caption.
  async queueImages(paths: string[]): Promise<void> {
    if (!this.peer || this.attaching) return;
    if (!paths.length) return;
    const peer = this.peer, generation = this.generation;
    const input = this.root.querySelector<HTMLTextAreaElement>('#direct-input');
    const caption = input?.value ?? '';
    this.attaching = true;
    this.status('#direct-file-status', '正在计算校验值…');
    try {
      const messages = await invoke<DirectMessage[]>('v1_queue_direct_images', { peer, paths, caption });
      if (generation !== this.generation) return;
      for (const message of messages) {
        if (!this.messages.some(item => item.id === message.id)) this.messages.push(message);
      }
      this.messages.sort(compare);
      if (input && caption) input.value = '';
      await this.loadBatches();
      this.paint('bottom'); this.onChangeConversations();
      this.status('#direct-file-status', `已加入待发送：${messages.length} 张图片（对方在线后自动发送）`);
    } catch (error) {
      if (generation === this.generation) this.status('#direct-file-status', typeof error === 'string' ? error : '无法加入图片');
    } finally { this.attaching = false; }
  }

  private async loadBatches(): Promise<void> {
    if (!isTauri() || !this.peer) return;
    try {
      const rows = await invoke<{ message_id: string; batch_id: string; caption: string }[]>('v1_image_batches', { kind: 'direct', target: this.peer });
      this.batches.clear(); this.batchCaptions.clear();
      for (const row of rows) { this.batches.set(row.message_id, row.batch_id); this.batchCaptions.set(row.batch_id, row.caption); }
    } catch { /* keep the previous grouping */ }
  }

  /// Files dropped onto the window are queued in arrival order.
  async queuePaths(paths: string[]): Promise<void> {
    // A drop or pick that is all images becomes one bubble, same as the image button.
    if (paths.length && paths.every(path => /\.(png|jpe?g|gif|webp|bmp)$/i.test(path))) { await this.queueImages(paths); return; }
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
      if (kind === 'other' && await this.openWithPlugin(message, name)) { return; }
      if (kind === 'other') { this.paintPanel('该类型暂不支持预览，也未安装对应插件；可使用“打开所在目录”。'); return; }
      const preview = await invoke<TextPreview | null>('v1_read_text_attachment', { messageId: message.id });
      if (!preview) { this.paintPanel('文件过大、不是文本或无法读取，未能预览'); return; }
      this.paintPanel('', preview.text);
    } catch (error) {
      this.paintPanel(typeof error === 'string' ? error : '无法读取附件');
    } finally { this.panelBusy = false; }
  }

  /// Hands the file bytes to an installed read-only preview plugin, if one claims it.
  private async openWithPlugin(message: DirectMessage, name: string): Promise<boolean> {
    try {
      const plugin = await invoke<{ plugin_id: string; html: string } | null>('v1_plugin_preview', { fileName: name });
      if (!plugin) return false;
      const data = await invoke<string>('v1_attachment_base64', { messageId: message.id });
      this.panel = { id: message.id, name, kind: 'plugin' };
      this.panelBytes = base64ToBytes(data);
      this.paintPanel('', plugin.html);
      return true;
    } catch (error) {
      this.paintPanel(typeof error === 'string' ? error : '插件预览失败');
      return true;
    }
  }

  /// Right-click on a file bubble: open with the default app, reveal it, or preview it.
  private fileMenu(event: MouseEvent, message: DirectMessage): void {
    const name = message.file_name ?? message.body;
    if (message.file_status !== 'completed') {
      showContextMenu(event, [{ label: '文件尚未接收完成', run: () => {} }]);
      return;
    }
    showContextMenu(event, [
      { label: '用默认程序打开', run: () => { void invoke('v1_open_attachment', { messageId: message.id })
        .catch((error) => this.status('#direct-file-status', typeof error === 'string' ? error : '无法打开文件')); } },
      { label: '打开所在目录', run: () => { void this.reveal(message.id); } },
      { label: '预览', run: () => { void this.openPreview(message); } },
      { label: '复制文件名', run: () => { void navigator.clipboard?.writeText(name).catch(() => {}); } },
    ]);
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
    if (this.panel.kind === 'plugin') {
      body.append(createPluginFrame(content, this.panelBytes ?? new Uint8Array(), this.panel.name));
      const note = document.createElement('p'); note.className = 'panel-note';
      note.textContent = '插件在沙箱中运行，只能读取这个文件的字节；第一版为只读预览。';
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
    this.loading = false; this.previews.clear(); this.previewPending.clear(); this.offers = []; this.onChange();
    try {
      const address = await invoke<string | null>('v1_contact_address', { peer });
      if (generation === this.generation) {
        this.address = address ?? '';
        const input = this.root.querySelector<HTMLInputElement>('#direct-address');
        if (input && document.activeElement !== input) input.value = this.addressDrafts.get(peer) ?? this.address;
      }
    } catch { this.status('#direct-address-status', '无法读取联系人地址'); }
    await this.load(false, true);
    void this.loadOffers();
    this.startPolling();
  }

  private startPolling(): void {
    this.stopPolling();
    this.poll = window.setInterval(() => { if (!document.hidden) { void this.load(false); void this.loadOffers(); } }, POLL_INTERVAL);
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
      await this.loadBatches();
      if (older || initial) this.hasOlder = batch.length === 200;
      if (changed || older || initial) this.paint(initial ? 'bottom' : older ? 'preserve' : 'auto');
    } catch { if (generation === this.generation) this.status('#direct-error', '无法读取单聊记录'); }
    finally { if (generation === this.generation) this.loading = false; }
  }

  /// Jump to a message, paging older history until it is in memory.
  async jumpTo(messageId: string): Promise<void> {
    if (!messageId) return;
    const found = () => this.messages.find(item => item.id === messageId);
    for (let page = 0; page < 20 && !found() && this.hasOlder; page += 1) {
      const before = this.messages.length;
      await this.load(true);
      if (!isTauri() || this.messages.length === before) break;
    }
    const node = this.root.querySelector<HTMLElement>(`#direct-messages [data-id="${messageId}"]`);
    if (!node) { this.status('#direct-error', '该消息暂不在本机记录中'); return; }
    node.scrollIntoView({ block: 'center' });
    node.classList.add('jump-target');
    window.setTimeout(() => node.classList.remove('jump-target'), 1600);
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
    image.addEventListener('click', () => {
      const message = this.messages.find(item => item.id === id);
      if (this.imageOpenMode === 'workspace' && message) { void this.openPreview(message); return; }
      showImage(url, '聊天图片');
    });
    holder.append(image);
  }

  /// Pending file offers that the peer must wait on while per-file confirmation is on.
  private async loadOffers(): Promise<void> {
    if (!isTauri() || !this.peer) return;
    const peer = this.peer, generation = this.generation;
    try {
      const offers = await invoke<FileOffer[]>('v1_file_offer_requests', { peer });
      if (generation !== this.generation || peer !== this.peer) return;
      const changed = offers.length !== this.offers.length
        || offers.some((offer, index) => offer.message_id !== this.offers[index].message_id);
      this.offers = offers;
      if (changed) this.paintOffers();
    } catch { /* polling; the next tick retries */ }
  }

  private paintOffers(): void {
    const box = this.root.querySelector<HTMLElement>('#direct-offers');
    if (!box) return;
    box.replaceChildren();
    box.hidden = this.offers.length === 0;
    for (const offer of this.offers) {
      const card = document.createElement('div'); card.className = 'offer-card';
      const text = document.createElement('div'); text.className = 'offer-text';
      const title = document.createElement('div'); title.className = 'file-title'; title.textContent = offer.name;
      const meta = document.createElement('small'); meta.className = 'file-meta';
      meta.textContent = `${formatSize(offer.total)} · ${new Date(offer.created_at * 1000).toLocaleString()} · 等待确认`;
      text.append(title, meta);
      const actions = document.createElement('div'); actions.className = 'file-actions';
      const accept = document.createElement('button');
      accept.className = 'outline'; accept.textContent = '接收';
      accept.dataset.action = 'accept-file-offer'; accept.dataset.id = offer.message_id;
      const reject = document.createElement('button');
      reject.className = 'linkish'; reject.textContent = '拒绝';
      reject.dataset.action = 'reject-file-offer'; reject.dataset.id = offer.message_id;
      actions.append(accept, reject);
      card.append(text, actions);
      box.append(card);
    }
  }

  /// Accepting only marks the local decision; the sender retries and then proceeds.
  async resolveOffer(id: string, accept: boolean): Promise<void> {
    if (!isTauri() || !id) return;
    try {
      await invoke('v1_resolve_file_offer', { id, accept });
      this.offers = this.offers.filter(offer => offer.message_id !== id);
      this.paintOffers();
      this.status('#direct-file-status', accept ? '已接受，等待对方重新发送' : '已拒绝该文件');
    } catch { this.status('#direct-error', '无法更新文件确认状态'); }
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
    this.renderedBatches.clear();
    for (const message of this.messages) {
      const incoming = message.sender === this.peer;
      const row = document.createElement('div'); row.className = `message-row${incoming ? ' incoming' : ''}`; row.dataset.id = message.id;
      const column = document.createElement('div'); column.className = 'message-column';
      const batch = this.batches.get(message.id);
      if (batch) {
        // Only the first child paints the shared bubble; the rest are skipped.
        if (this.renderedBatches.has(batch)) continue;
        this.renderedBatches.add(batch);
        const bubble = document.createElement('div'); bubble.className = 'message-bubble image-batch';
        const caption = this.batchCaptions.get(batch) ?? '';
        if (caption) { const text = document.createElement('div'); text.className = 'message-text'; text.textContent = caption; bubble.append(text); }
        const grid = document.createElement('div'); grid.className = 'batch-grid';
        for (const item of this.messages.filter(candidate => this.batches.get(candidate.id) === batch)) {
          const cell = document.createElement('div'); cell.className = 'batch-item';
          cell.addEventListener('contextmenu', (event) => this.fileMenu(event, item));
          const preview = document.createElement('div'); preview.className = 'file-preview'; preview.dataset.preview = item.id;
          const meta = document.createElement('small'); meta.className = 'file-meta';
          meta.textContent = item.file_status === 'completed' ? formatSize(item.file_size ?? 0) : fileStatus(item);
          cell.append(preview, meta);
          grid.append(cell);
          this.ensurePreview(item);
        }
        bubble.append(grid);
        column.append(bubble);
      } else if (message.kind === 'file') {
        const bubble = document.createElement('div'); bubble.className = 'message-bubble file-bubble';
        const name = document.createElement('div'); name.className = 'file-title'; name.textContent = message.file_name ?? message.body;
        bubble.append(name);
        const preview = document.createElement('div'); preview.className = 'file-preview'; preview.dataset.preview = message.id;
        bubble.append(preview);
        const meta = document.createElement('small'); meta.className = 'file-meta'; meta.textContent = fileMeta(message, incoming);
        bubble.append(meta);
        bubble.addEventListener('contextmenu', (event) => this.fileMenu(event, message));
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

function fileStatus(message: DirectMessage): string {
  const status: string = message.file_status ?? 'pending';
  if (status === 'transferring') return `传输中 ${formatSize(message.file_transferred ?? 0)} / ${formatSize(message.file_size ?? 0)}`;
  if (status === 'completed') return '已完成';
  if (status === 'failed') return message.file_error ?? '失败';
  if (status === 'paused') return message.file_error ?? '已暂停';
  return '等待对方';
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
