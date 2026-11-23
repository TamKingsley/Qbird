import { invoke, isTauri } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { composerTools } from './ui_icons';
import { showImage } from './image_viewer';
import { base64ToBytes } from './plugin_view';
import { workspace, type LoadedTab } from './workspace';
import { linkifiedText } from './linkify';
import { recallItems, showContextMenu } from './context_menu';
import { t, trError } from './i18n';
import { formatStamp } from './chat_prefs';
import { loadUntilFound } from './search_navigation';
import { groupedMessage, messageAvatar } from './message_avatar';

type DirectMessage = {
  id: string; peer: string; sender: string; body: string; created_at: number; logical: number;
  status: 'pending' | 'delivered' | 'received' | 'failed';
  kind: 'text' | 'file';
  file_name: string | null; file_size: number | null; file_sha256: string | null;
  file_status: 'pending' | 'transferring' | 'completed' | 'failed' | null; file_transferred: number | null; file_error: string | null;
  recalled_at: number | null;
};
type TextPreview = { name: string; kind: string; text: string };
type FileOffer = { message_id: string; peer: string; name: string; total: number; created_at: number; status: string; group_id?: string | null };
type Staged = { path: string; name: string; image: boolean; preview: string | null };
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
  private offers: FileOffer[] = [];
  private staged: Staged[] = [];
  private batches = new Map<string, string>();
  private batchCaptions = new Map<string, string>();
  private renderedBatches = new Set<string>();
  /// 'lightbox' keeps the in-app viewer; 'workspace' opens the right panel instead.
  imageOpenMode: 'lightbox' | 'workspace' = 'lightbox';
  /// What clicking a file bubble does; the workspace previews it or the system app opens it.
  fileOpenMode: 'workspace' | 'system' = 'workspace';

  constructor(private root: HTMLElement, private onChange: () => void) {}

  view(): string {
    return `<section class="self-chat" aria-label="${t('单聊')}">
      <header class="chat-header"><div><strong id="direct-name"></strong><span id="direct-presence" class="presence-dot" hidden></span><span id="direct-presence-text" class="presence-text"></span><small>${t('端到端加密 · 已添加联系人')}</small></div><div class="group-actions"><button class="outline chat-more" data-action="chat-menu" data-kind="direct" aria-label="${t('会话菜单')}" title="${t('会话菜单')}">⋯</button></div></header>
      <div class="chat-history" id="direct-history" aria-label="${t('单聊记录')}"><div class="direct-address"><label for="direct-address">${t('联系人地址（仅连接提示）')}</label><div class="probe-row"><input id="direct-address" type="text" placeholder="192.168.1.8:42853"><button class="outline" data-action="save-direct-address">${t('保存地址')}</button></div><small>${t('地址不代表身份，连接时仍核对指纹。请在发现页面启动聊天服务；离线消息与文件须双方同时在线才能补发或续传。')}</small><p id="direct-address-status" role="status"></p></div><button class="load-older" data-action="direct-older" hidden>${t('加载更早消息')}</button><div id="direct-messages"></div><div id="direct-offers" class="direct-offers" hidden></div></div>
      <div class="chat-compose"><label for="direct-input">${t('消息')}</label><div class="compose-staging" id="direct-staging" aria-label="${t('待发送附件')}" hidden></div><textarea id="direct-input" rows="3" maxlength="32768" placeholder="${t('写点什么，或把文件/图片拖进窗口…')}"></textarea>${composerTools('direct', t('Enter 发送 · Shift+Enter 换行'))}<p id="direct-file-status" role="status"></p><p id="direct-error" role="status"></p></div>
    </section>`;
  }

  clear(): void { this.peer = ''; this.generation++; this.offers = []; this.staged = []; this.paintStaging(); this.stopPolling(); }

  draft(value: string): void { if (this.peer) this.drafts.set(this.peer, value); }
  addressDraft(value: string): void { if (this.peer) this.addressDrafts.set(this.peer, value); }

  async hide(): Promise<void> {
    if (!this.peer) return;
    const peer = this.peer, generation = this.generation;
    try {
      await invoke('v1_hide_direct', { peer });
      if (generation === this.generation) { this.clear(); this.onChange(); workspace.notify(t('已隐藏会话；收到新消息时会重新出现。')); }
    } catch { if (generation === this.generation) this.status('#direct-error', t('无法隐藏会话')); }
  }

  async saveAddress(): Promise<void> {
    const input = this.root.querySelector<HTMLInputElement>('#direct-address');
    if (!this.peer || !input) return;
    const peer = this.peer, generation = this.generation, address = input.value.trim();
    try {
      await invoke('v1_set_contact_address', { peer, address });
      if (generation === this.generation) {
        this.address = address; if (input.value.trim() === address) this.addressDrafts.delete(peer);
        this.status('#direct-address-status', t('地址已保存，正在尝试发送待发送消息…'));
      }
      let sent = 0; let failure = '';
      try { sent = await invoke<number>('v1_flush_direct', { peer }); }
      catch (error) { failure = trError(error, t('发送未确认')); }
      if (generation === this.generation) {
        this.status('#direct-address-status', failure
          ? `${failure}${t('。地址已保存，可稍后重试。')}`
          : sent > 0 ? `${t('地址已保存，已发送')}${sent}${t('条待发送消息。')}` : t('地址已保存；当前没有待发送消息。'));
        await this.load(false);
      }
    } catch (error) {
      if (generation === this.generation) this.status('#direct-address-status', trError(error, t('无法保存地址')));
    }
  }

  async send(): Promise<void> {
    const input = this.root.querySelector<HTMLTextAreaElement>('#direct-input');
    const button = this.root.querySelector<HTMLButtonElement>('button[data-action="direct-send"]');
    if (!this.peer || !input || this.sending) return;
    const peer = this.peer, generation = this.generation, body = input.value;
    const staged = this.staged.slice();
    if (!body.trim() && !staged.length) { this.status('#direct-error', t('请输入消息，或先添加图片/文件')); return; }
    this.sending = true; if (button) button.disabled = true; this.status('#direct-error', '');
    try {
      if (staged.length) {
        const images = staged.filter((item) => item.image).map((item) => item.path);
        const files = staged.filter((item) => !item.image).map((item) => item.path);
        if (images.length) await invoke<DirectMessage[]>('v1_queue_direct_images', { peer, paths: images, caption: body });
        for (const path of files) await invoke<DirectMessage>('v1_queue_direct_file', { peer, path });
        // A plain file cannot carry a caption yet, so the typed text goes out as its own message.
        if (!images.length && body.trim()) await invoke<DirectMessage>('v1_queue_direct', { peer, body });
        this.staged = [];
        this.paintStaging();
        if (this.drafts.get(peer) === body) this.drafts.set(peer, '');
        if (generation === this.generation) {
          if (input.isConnected && input.value === body) input.value = '';
          await this.load(false, true);
          this.onChangeConversations();
          this.status('#direct-file-status', images.length
            ? `${t('已加入待发送：')}${images.length}${t('张图片')}${body.trim() ? t('与文字') : ''}${t('（对方在线后自动发送）')}`
            : `${t('已加入待发送：')}${files.length}${t('个文件')}${body.trim() ? '，文字已单独发送' : ''}${t('（对方在线后自动发送）')}`);
        }
        return;
      }
      const message = await invoke<DirectMessage>('v1_queue_direct', { peer, body });
      if (this.drafts.get(peer) === body) this.drafts.set(peer, '');
      if (generation === this.generation) {
        if (input.isConnected && input.value === body) input.value = '';
        if (!this.messages.some(item => item.id === message.id)) { this.messages.push(message); this.messages.sort(compare); }
        this.paint('bottom'); this.onChangeConversations();
      }
    } catch (error) {
      if (generation === this.generation) this.status('#direct-error', trError(error, t('无法保存待发送消息')));
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
        title: kind === 'image' ? t('选择图片') : t('选择文件'),
        filters: kind === 'image' ? [{ name: t('图片'), extensions: ['png', 'jpg', 'jpeg', 'gif', 'webp', 'bmp'] }] : undefined,
      });
      if (!selected) return;
      const paths = Array.isArray(selected) ? selected : [selected];
      await this.stagePaths(paths);
    } catch (error) {
      this.status('#direct-error', trError(error, t('无法打开文件选择器')));
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
    this.status('#direct-file-status', t('正在计算校验值…'));
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
      this.status('#direct-file-status', `${t('已加入待发送：')}${messages.length}${t('张图片（对方在线后自动发送）')}`);
    } catch (error) {
      if (generation === this.generation) this.status('#direct-file-status', trError(error, t('无法加入图片')));
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
    this.status('#direct-file-status', t('正在计算校验值…'));
    try {
      const message = await invoke<DirectMessage>('v1_queue_direct_file', { peer, path });
      if (generation !== this.generation) return;
      if (!this.messages.some(item => item.id === message.id)) { this.messages.push(message); this.messages.sort(compare); }
      this.paint('bottom'); this.onChangeConversations();
      this.status('#direct-file-status', `${t('已加入待发送：')}${message.file_name ?? message.body}${t('（对方在线后自动发送）')}`);
    } catch (error) {
      if (generation === this.generation) this.status('#direct-file-status', trError(error, t('无法加入文件')));
    }
  }

  /// Puts picked, dropped or pasted files into the composer staging area so the user
  /// can review thumbnails and mix them with text before anything is queued.
  async stagePaths(paths: string[]): Promise<void> {
    if (!isTauri() || !this.peer || !paths.length) return;
    this.attaching = true;
    this.status('#direct-file-status', t('正在准备预览…'));
    try {
      for (const path of paths) {
        const name = path.split(/[\\/]/).pop() ?? path;
        const image = /\.(png|jpe?g|gif|webp|bmp)$/i.test(path);
        let preview: string | null = null;
        if (image) preview = await invoke<string>('v1_local_preview', { path }).catch(() => null);
        if (!this.staged.some((item) => item.path === path)) this.staged.push({ path, name, image, preview });
      }
      this.paintStaging();
      this.status('#direct-file-status', this.staged.length
        ? `${t('已添加')}${this.staged.length}${t('个附件：可继续写文字，点「发送」一起发出。')}`
        : '');
    } finally { this.attaching = false; }
  }

  unstage(index: number): void {
    if (index < 0 || index >= this.staged.length) return;
    this.staged.splice(index, 1);
    this.paintStaging();
    this.status('#direct-file-status', this.staged.length ? `${t('还剩')}${this.staged.length}${t('个附件')}` : t('已清空附件'));
  }

  private paintStaging(): void {
    const box = this.root.querySelector<HTMLElement>('#direct-staging');
    if (!box) return;
    box.hidden = this.staged.length === 0;
    if (!this.staged.length) { box.innerHTML = ''; return; }
    box.innerHTML = this.staged.map((item, index) => item.image
      ? `<div class="staged-item is-image" title="${escapeHtml(item.name)}"><img src="${item.preview ?? ''}" alt=""><button type="button" class="staged-remove" data-action="direct-unstage" data-index="${index}" aria-label="${t('移除')}${escapeHtml(item.name)}">×</button></div>`
      : `<div class="staged-item is-file" title="${escapeHtml(item.name)}"><span class="staged-name">${escapeHtml(item.name)}</span><button type="button" class="staged-remove" data-action="direct-unstage" data-index="${index}" aria-label="${t('移除')}${escapeHtml(item.name)}">×</button></div>`
    ).join('') + `${t('发送时随文字一起发出')}`;
  }

  /// Opens one completed attachment in the global workspace panel. The workspace
  /// owns the tab and reloads content from the message id, so the layout survives
  /// page changes and restarts.
  openPreview(message: DirectMessage): void {
    if (!isTauri() || message.kind !== 'file' || message.file_status !== 'completed') return;
    workspace.open({ kind: 'direct-file', messageId: message.id, name: message.file_name ?? message.body });
  }

  /// Resolver entry used by the workspace for live opens and restored tabs.
  async loadWorkspaceFile(source: { messageId: string; name: string }): Promise<LoadedTab> {
    try { return await this.loadPreview(source.messageId, source.name); }
    catch (error) { return { kind: 'error', message: trError(error, t('无法读取该文件')) }; }
  }

  private async loadPreview(messageId: string, name: string): Promise<LoadedTab> {
    const kind = previewKind(name);
    if (kind === 'image') {
      const preview = await invoke<{ name: string; data_url: string } | null>('v1_read_attachment', { messageId });
      if (!preview) return { kind: 'unsupported', reason: t('图片过大或无法读取，未能预览') };
      this.previews.set(messageId, preview.data_url);
      return { kind: 'image', src: preview.data_url };
    }
    if (kind === 'pdf') {
      const bytes = await invoke<ArrayBuffer>('v1_attachment_bytes', { messageId });
      return { kind: 'pdf', bytes };
    }
    if (kind === 'other') {
      const plugin = await invoke<{ plugin_id: string; html: string } | null>('v1_plugin_preview', { fileName: name });
      if (!plugin) return { kind: 'unsupported', reason: t('该类型暂不支持内置预览；可用“用默认程序打开”或“打开所在目录”，安装预览插件后也可在此查看。') };
      const data = await invoke<string>('v1_attachment_base64', { messageId });
      return { kind: 'plugin', html: plugin.html, bytes: base64ToBytes(data) };
    }
    const preview = await invoke<TextPreview | null>('v1_read_text_attachment', { messageId });
    if (!preview) return { kind: 'unsupported', reason: t('文件过大、不是文本或无法读取，未能预览') };
    const textKind = preview.kind === 'markdown' ? 'markdown' : preview.kind === 'html' ? 'html' : 'text';
    return { kind: textKind, text: preview.text };
  }

  /// Click behaviour for a completed file bubble, governed by the workspace setting.
  private activateFile(message: DirectMessage): void {
    if (message.kind !== 'file' || message.file_status !== 'completed') return;
    if (this.fileOpenMode === 'system') { void this.openWithSystem(message); return; }
    this.openPreview(message);
  }

  async openSystemById(messageId: string): Promise<void> {
    try { await invoke('v1_open_attachment', { messageId }); }
    catch (error) {
      const text = trError(error, t('无法打开文件'));
      this.status('#direct-file-status', text);
      workspace.notify(text);
    }
  }

  private openWithSystem(message: DirectMessage): Promise<void> {
    return this.openSystemById(message.id);
  }

  /// Text bubble menu: withdraw inside the two-minute window, copy otherwise.
  private textMenu(event: MouseEvent, message: DirectMessage): void {
    const items = recallItems(message.sender !== this.peer, this.canRecall(message), () => { void this.recall(message); });
    items.push({ label: t('复制文字'), run: () => { void navigator.clipboard?.writeText(message.body).catch(() => {}); } });
    items.push({ label: t('删除'), danger: true, run: () => { void this.deleteLocal(message.id); } });
    showContextMenu(event, items);
  }

  /// Deletes this message from the local record only.
  async deleteLocal(messageId: string): Promise<void> {
    if (!isTauri()) return;
    try {
      await invoke('v1_delete_messages', { kind: 'direct', id: this.peer, ids: [messageId] });
      this.load(true);
    } catch (error) { workspace.notify(trError(error, t('删除失败'))); }
  }

  private canRecall(message: DirectMessage): boolean {
    if (message.sender === this.peer || message.recalled_at) return false;
    return Date.now() / 1000 - message.created_at <= 120;
  }

  async recall(message: DirectMessage): Promise<void> {
    if (!isTauri()) return;
    try {
      const result = await invoke<string>('v1_recall_message', { kind: 'direct', id: message.id, groupId: null });
      if (result === 'recalled') {
        message.recalled_at = Math.floor(Date.now() / 1000);
        message.body = ' ';
        this.paint('auto');
        this.onChangeConversations();
      } else if (result === 'too_late') {
        this.status('#direct-file-status', t('超过 2 分钟，不能再撤回'));
      } else if (result === 'missing') {
        this.status('#direct-file-status', t('这条消息已经不存在'));
      }
    } catch (error) {
      this.status('#direct-file-status', trError(error, t('撤回失败')));
    }
  }

  /// Right-click on a file bubble: open with the default app, reveal it, or preview it.
  private fileMenu(event: MouseEvent, message: DirectMessage): void {
    const name = message.file_name ?? message.body;
    if (message.file_status !== 'completed') {
      const items = [];
      if (message.sender !== this.peer && this.canRecall(message)) {
        items.push({ label: t('撤回并取消发送'), danger: true, run: () => { void this.recall(message); } });
      }
      items.push({ label: t('文件尚未接收完成'), run: () => {} });
      showContextMenu(event, items);
      return;
    }
    showContextMenu(event, [
      { label: t('用默认程序打开'), run: () => { void this.openWithSystem(message); } },
      { label: t('打开所在目录'), run: () => { void this.reveal(message.id); } },
      { label: t('预览'), run: () => { this.openPreview(message); } },
      { label: t('复制文件名'), run: () => { void navigator.clipboard?.writeText(name).catch(() => {}); } },
      ...(this.canRecall(message) ? [{ label: t('撤回（已接收的文件保留）'), danger: true, run: () => { void this.recall(message); } }] : []),
    ]);
  }

  async reveal(messageId: string): Promise<void> {
    if (!isTauri()) return;
    try { await invoke('v1_reveal_attachment', { messageId }); }
    catch (error) {
      const text = trError(error, t('无法打开文件所在目录'));
      this.status('#direct-file-status', text);
      workspace.notify(text);
    }
  }

  openPreviewById(id: string): void {
    const message = this.messages.find((item) => item.id === id);
    if (message) this.openPreview(message);
  }

  private onChangeConversations(): void { this.root.dispatchEvent(new CustomEvent('conversations-changed')); }

  async open(peer: string, name: string): Promise<void> {
    if (!isTauri()) return;
    this.stopPolling();
    const generation = ++this.generation;
    await invoke('v1_open_direct', { peer });
    if (generation !== this.generation) return;
    this.peer = peer; this.name = name; this.messages = []; this.address = ''; this.hasOlder = false; this.refreshBefore = null;
    this.loading = false; this.previews.clear(); this.previewPending.clear(); this.offers = []; this.staged = []; this.paintStaging(); this.onChange();
    try {
      const address = await invoke<string | null>('v1_contact_address', { peer });
      if (generation === this.generation) {
        this.address = address ?? '';
        const input = this.root.querySelector<HTMLInputElement>('#direct-address');
        if (input && document.activeElement !== input) input.value = this.addressDrafts.get(peer) ?? this.address;
      }
    } catch { this.status('#direct-address-status', t('无法读取联系人地址')); }
    await this.load(false, true);
    if (!this.address) this.status('#direct-address-status', t('尚未记录对方地址：消息会一直显示“待发送”，请填入对方 IP:端口并保存地址。'));
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
    } catch { if (generation === this.generation) this.status('#direct-error', t('无法读取单聊记录')); }
    finally { if (generation === this.generation) this.loading = false; }
  }

  /// Jump to a message, paging older history until it is in memory.
  async jumpTo(messageId: string): Promise<void> {
    if (!messageId) return;
    const found = () => this.messages.find(item => item.id === messageId);
    await loadUntilFound(() => !!found(), () => this.hasOlder, () => this.messages.length, () => this.load(true));
    const node = this.root.querySelector<HTMLElement>(`#direct-messages [data-id="${messageId}"]`);
    if (!node) { this.status('#direct-error', t('该消息暂不在本机记录中')); return; }
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
    image.src = url; image.alt = t('聊天图片'); image.className = 'message-image';
    image.addEventListener('click', (event) => {
      event.stopPropagation();
      const message = this.messages.find(item => item.id === id);
      if (this.imageOpenMode === 'workspace' && message) { this.openPreview(message); return; }
      showImage(url, t('聊天图片'));
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
      meta.textContent = `${formatSize(offer.total)} · ${new Date(offer.created_at * 1000).toLocaleString()}${t('· 等待确认')}`;
      text.append(title, meta);
      const actions = document.createElement('div'); actions.className = 'file-actions';
      const accept = document.createElement('button');
      accept.className = 'outline'; accept.textContent = t('接收');
      accept.dataset.action = 'accept-file-offer'; accept.dataset.id = offer.message_id;
      const remember = document.createElement('button');
      remember.className = 'outline'; remember.textContent = t('接收并不再询问');
      remember.dataset.action = 'accept-file-offer-remember'; remember.dataset.id = offer.message_id;
      const saveAs = document.createElement('button');
      saveAs.className = 'outline'; saveAs.textContent = t('另存为…');
      saveAs.dataset.action = 'saveas-file-offer'; saveAs.dataset.id = offer.message_id;
      const reject = document.createElement('button');
      reject.className = 'linkish'; reject.textContent = t('拒绝');
      reject.dataset.action = 'reject-file-offer'; reject.dataset.id = offer.message_id;
      actions.append(accept, remember, saveAs, reject);
      card.append(text, actions);
      box.append(card);
    }
  }

  /// Accepting only marks the local decision; the sender retries and then proceeds.
  /// `remember` also stops future prompts from this contact; `dir` is a one-shot
  /// destination for this single file.
  async resolveOffer(id: string, accept: boolean, options: { dir?: string; remember?: boolean } = {}): Promise<void> {
    if (!isTauri() || !id) return;
    try {
      if (options.remember && this.peer) await invoke('v1_remember_file_source', { source: this.peer });
      await invoke('v1_resolve_file_offer', { id, accept, dir: options.dir ?? null });
      this.offers = this.offers.filter(offer => offer.message_id !== id);
      this.paintOffers();
      const saved = options.dir ? t('，将保存到你选择的文件夹') : '';
      this.status('#direct-file-status', accept ? `${t('已接受')}${saved}${t('，等待对方重新发送')}` : t('已拒绝该文件'));
    } catch (error) { this.status('#direct-error', trError(error, t('无法更新文件确认状态'))); }
  }

  /// Ask for a folder and accept this one file into it.
  async saveOfferAs(id: string): Promise<void> {
    if (!isTauri() || !id) return;
    try {
      const picked = await open({ directory: true, multiple: false, title: t('选择该文件的保存位置') });
      if (typeof picked !== 'string') return;
      await this.resolveOffer(id, true, { dir: picked });
    } catch (error) { this.status('#direct-error', trError(error, t('无法选择保存位置'))); }
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
    let previous: DirectMessage | null = null;
    for (const message of this.messages) {
      const incoming = message.sender === this.peer;
      const row = document.createElement('div'); row.className = `message-row${incoming ? ' incoming' : ''}`; row.dataset.id = message.id;
      if (groupedMessage(previous, message)) row.classList.add('grouped');
      const column = document.createElement('div'); column.className = 'message-column';
      const avatar = messageAvatar(incoming ? message.sender : 'self', incoming ? this.name : t('我'));
      if (message.recalled_at) {
        const note = document.createElement('div');
        note.className = 'message-bubble recalled';
        note.textContent = incoming ? `${this.name || t('对方')}${t('撤回了一条消息')}` : t('你撤回了一条消息');
        column.append(note);
        row.append(avatar, column);
        list.append(row);
        previous = message;
        continue;
      }
      const batch = this.batches.get(message.id);
      if (batch) {
        // Only the first child paints the shared bubble; the rest are skipped.
        if (this.renderedBatches.has(batch)) continue;
        this.renderedBatches.add(batch);
        const bubble = document.createElement('div'); bubble.className = 'message-bubble image-batch';
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
        bubble.addEventListener('click', (event) => {
          if ((event.target as HTMLElement).closest('.linkish')) return;
          this.activateFile(message);
        });
        if (message.file_status === 'completed') {
          const actions = document.createElement('div'); actions.className = 'file-actions';
          const preview = document.createElement('button');
          preview.className = 'linkish'; preview.textContent = t('预览');
          preview.dataset.action = 'direct-preview'; preview.dataset.id = message.id;
          const reveal = document.createElement('button');
          reveal.className = 'linkish'; reveal.textContent = t('打开所在目录');
          reveal.dataset.action = 'direct-reveal'; reveal.dataset.id = message.id;
          actions.append(preview, reveal);
          bubble.append(actions);
        }
        column.append(bubble);
        this.ensurePreview(message);
      } else {
        const body = document.createElement('div'); body.className = 'message-bubble'; body.append(linkifiedText(message.body));
        body.addEventListener('contextmenu', (event) => this.textMenu(event, message));
        column.append(body);
      }
      const time = document.createElement('small'); time.className = 'message-time';
      time.textContent = formatStamp(message.created_at);
      column.prepend(time);
      if (!incoming) {
        const status = document.createElement('small'); status.className = 'message-status';
        status.textContent = message.kind === 'file' ? '' : message.status === 'delivered' ? t('已送达') : t('待发送');
        if (status.textContent) column.append(status);
      }
      row.append(avatar, column); list.append(row);
      previous = message;
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
    this.paintStaging();
    this.paint('bottom');
  }
}

function fileStatus(message: DirectMessage): string {
  const status: string = message.file_status ?? 'pending';
  if (status === 'transferring') return `${t('传输中')}${formatSize(message.file_transferred ?? 0)} / ${formatSize(message.file_size ?? 0)}`;
  if (status === 'completed') return t('已完成');
  if (status === 'failed') return message.file_error ?? t('失败');
  if (status === 'paused') return message.file_error ?? t('已暂停');
  return t('等待对方');
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
  if (!bytes || bytes < 0) return t('未知大小');
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
      case 'completed': return incoming ? t('已接收') : t('已发送');
      case 'transferring': return `${incoming ? t('接收中') : t('发送中')} ${percent}%`;
      case 'failed': return `${incoming ? t('接收中断') : t('发送失败')}${message.file_error ? `：${message.file_error}` : t(' · 重新连接后可续传')}`;
      default: return incoming ? t('等待接收') : t('等待对方在线');
    }
  })();
  return `${size} · ${label}`;
}

function sameMessages(a: DirectMessage[], b: DirectMessage[]): boolean {
  if (a.length !== b.length) return false;
  return a.every((item, index) => {
    const other = b[index];
    return other && item.id === other.id && item.status === other.status && item.recalled_at === other.recalled_at
      && item.file_status === other.file_status && item.file_transferred === other.file_transferred;
  });
}
