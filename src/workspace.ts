// Global tabbed right-hand workspace shared by direct, group and self chats.
//
// The panel lives outside #app on purpose: render() rebuilds #app innerHTML on every
// page change, and re-parenting an iframe would reload PDFs and web pages. Mounting
// once on document.body keeps tabs (and their network state) alive across renders.
//
// Tabs are described by serializable sources, so the layout can be written to
// workspace.json and rebuilt on the next launch. Content is always reloaded by id;
// nothing binary is persisted.
import { invoke, isTauri } from '@tauri-apps/api/core';
import { attachImageViewer } from './image_viewer';
import { createPluginFrame } from './plugin_view';
import { t, trError } from './i18n';

export type LoadedTab =
  | { kind: 'image'; src: string }
  | { kind: 'pdf'; bytes: ArrayBuffer }
  | { kind: 'text' | 'markdown' | 'html'; text: string }
  | { kind: 'plugin'; html: string; bytes: Uint8Array }
  | { kind: 'unsupported'; reason: string }
  | { kind: 'error'; message: string };

export type TabActions = { systemOpen?: () => void; reveal?: () => void };

/// Only serializable data; loaders and DOM are rebuilt from this on restore.
export type WorkspaceSource =
  | { kind: 'direct-file'; messageId: string; name: string }
  | { kind: 'group-file'; groupId: string; messageId: string; name: string }
  | { kind: 'web'; url: string };

export type ResolvedSource = { title?: string; loaded: LoadedTab; actions?: TabActions };
export type WorkspaceResolver = (source: WorkspaceSource) => Promise<ResolvedSource>;

type GroupId = 'a' | 'b';

type Tab = {
  key: string;
  source: WorkspaceSource;
  group: GroupId;
  title: string;
  pane: HTMLElement;
  button: HTMLElement;
  actions: TabActions;
  kind: LoadedTab['kind'] | 'loading' | 'web';
  revoke: (() => void) | null;
  destroy: (() => void) | null;
  reload: (() => void) | null;
  url: string;
};

type PersistedTab = { key: string; source: WorkspaceSource };
type PersistedState = {
  version: 1;
  width: number;
  split: boolean;
  focused: GroupId;
  groups: Record<GroupId, { active: string | null; tabs: PersistedTab[] }>;
};

const MAX_TABS = 12;
const DEFAULT_WIDTH = 420;
const MIN_WIDTH = 320;
const MAX_STATE_BYTES = 64 * 1024;
const WEB_LOAD_TIMEOUT = 6000;

class Workspace {
  private groups: Record<GroupId, { tabs: Tab[]; active: string | null }> = {
    a: { tabs: [], active: null },
    b: { tabs: [], active: null },
  };
  private split = false;
  private focused: GroupId = 'a';
  private width = DEFAULT_WIDTH;
  private root: HTMLElement | null = null;
  private toolbarEl: HTMLElement | null = null;
  private hintEl: HTMLElement | null = null;
  private splitButton: HTMLButtonElement | null = null;
  private strips: Record<GroupId, HTMLElement> = {} as Record<GroupId, HTMLElement>;
  private bodies: Record<GroupId, HTMLElement> = {} as Record<GroupId, HTMLElement>;
  private dropIndex: Record<GroupId, number> = { a: 0, b: 0 };
  private dragKey: string | null = null;
  private resolver: WorkspaceResolver | null = null;
  private canSave = false;
  private saveTimer = 0;
  private hint = '';
  private hintTimer = 0;
  private keyBound = false;

  get isOpen(): boolean {
    return this.tabs().length > 0;
  }

  /// Registered once by main.ts; turns a persisted source back into preview content.
  setResolver(resolver: WorkspaceResolver): void {
    this.resolver = resolver;
  }

  open(source: WorkspaceSource): void {
    this.ensure();
    const existing = this.findTab(keyOf(source));
    if (existing) { this.activate(existing); return; }
    const group: GroupId = this.split ? this.focused : 'a';
    this.createTab(source, group, true);
  }

  openUrl(raw: string): void {
    const url = normalizeUrl(raw);
    if (!url) return;
    this.open({ kind: 'web', url });
  }

  closeAll(): void {
    for (const tab of this.tabs()) this.dispose(tab);
    this.groups = { a: { tabs: [], active: null }, b: { tabs: [], active: null } };
    this.split = false;
    this.focused = 'a';
    this.paint();
    this.scheduleSave();
  }

  /// Kept as a hook for render(): the panel is outside #app, so only layout needs a
  /// refresh, never re-parenting.
  refreshLayout(): void { if (this.root) this.applyLayout(); }

  /// Hides the panel while the app is locked; tabs and iframes stay alive.
  suspend(): void {
    if (!this.root) return;
    this.root.style.display = 'none';
    document.body.classList.remove('ws-open');
  }

  resume(): void {
    if (!this.root) return;
    this.root.style.display = '';
    this.applyLayout();
  }

  notify(text: string): void {
    this.ensure();
    this.hint = text;
    if (this.hintEl) this.hintEl.textContent = text;
    window.clearTimeout(this.hintTimer);
    this.hintTimer = window.setTimeout(() => {
      this.hint = '';
      if (this.hintEl) this.hintEl.textContent = '';
    }, 6000);
  }

  /// Reads workspace.json and rebuilds the tabs. Safe to call once at startup.
  async restore(): Promise<void> {
    try {
      if (isTauri()) {
        const raw = await invoke<string | null>('v1_workspace_state');
        if (raw && raw.length <= MAX_STATE_BYTES) this.applyState(parseState(raw));
      }
    } catch { /* a corrupt or missing layout simply starts empty */ }
    this.canSave = true;
  }

  private applyState(state: PersistedState | null): void {
    if (!state) return;
    this.ensure();
    this.width = Math.min(window.innerWidth * 0.7, Math.max(MIN_WIDTH, state.width || DEFAULT_WIDTH));
    this.split = state.split;
    for (const group of ['a', 'b'] as const) {
      for (const entry of state.groups[group].tabs) {
        const source = validSource(entry.source);
        if (!source) continue;
        this.createTab(source, group, false);
      }
    }
    for (const group of ['a', 'b'] as const) {
      const requested = state.groups[group].active;
      const groupState = this.groups[group];
      groupState.active = requested && groupState.tabs.some((tab) => tab.key === requested)
        ? requested
        : groupState.tabs[groupState.tabs.length - 1]?.key ?? null;
    }
    this.focused = state.focused === 'b' && this.groups.b.tabs.length ? 'b' : 'a';
    this.paint();
  }

  private ensure(): void {
    if (this.root) return;
    const root = document.createElement('aside');
    root.id = 'workspace-panel';
    root.className = 'workspace-panel';
    root.hidden = true;
    root.dataset.focus = 'a';
    root.setAttribute('aria-label', t('右侧工作区'));
    root.innerHTML = `
      <div class="ws-resizer" data-ws="resizer" aria-hidden="true"></div>
      <header class="ws-header">
        <div class="ws-tabbar">
          <div class="ws-tabgroups">
            <div class="ws-group" data-group="a"><div class="ws-tabs" data-tabs="a" role="tablist"></div></div>
            <div class="ws-group" data-group="b"><div class="ws-tabs" data-tabs="b" role="tablist"></div></div>
          </div>
          <div class="ws-head-actions">
            <button type="button" class="outline ws-split" data-ws="split">${t('分屏')}</button>
            <button type="button" class="outline ws-close-all" data-ws="close-all">${t('全部关闭')}</button>
          </div>
        </div>
        <div class="ws-toolbar" id="ws-toolbar"></div>
        <p class="ws-hint" id="ws-hint" role="status"></p>
      </header>
      <div class="ws-body">
        <div class="ws-pane-group" data-body="a"></div>
        <div class="ws-pane-group" data-body="b"></div>
      </div>`;
    document.body.append(root);
    this.root = root;
    this.toolbarEl = root.querySelector<HTMLElement>('#ws-toolbar');
    this.hintEl = root.querySelector<HTMLElement>('#ws-hint');
    this.splitButton = root.querySelector<HTMLButtonElement>('[data-ws="split"]');
    for (const group of ['a', 'b'] as const) {
      this.strips[group] = root.querySelector<HTMLElement>(`[data-tabs="${group}"]`)!;
      this.bodies[group] = root.querySelector<HTMLElement>(`[data-body="${group}"]`)!;
      this.bindGroup(this.strips[group], this.bodies[group], group);
    }
    root.querySelector<HTMLElement>('[data-ws="close-all"]')?.addEventListener('click', () => this.closeAll());
    this.splitButton?.addEventListener('click', () => this.toggleSplit());
    root.querySelector<HTMLElement>('[data-ws="resizer"]')?.addEventListener('pointerdown', (event) => this.startResize(event));
    if (!this.keyBound) {
      this.keyBound = true;
      window.addEventListener('keydown', (event) => {
        if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'w') {
          const active = this.focusedTab();
          if (!active) return;
          const target = event.target as HTMLElement | null;
          if (target && target.closest('input, textarea, [contenteditable="true"]')) return;
          event.preventDefault();
          this.closeTab(active.key);
        }
      });
    }
    this.applyLayout();
  }

  private bindGroup(strip: HTMLElement, body: HTMLElement, group: GroupId): void {
    strip.addEventListener('dragover', (event) => {
      if (!this.dragKey) return;
      event.preventDefault();
      this.dropIndex[group] = this.indexAt(strip, event.clientX);
      this.markDrop(group, this.dropIndex[group]);
    });
    strip.addEventListener('dragleave', (event) => {
      if (!strip.contains(event.relatedTarget as Node | null)) this.clearDropMarks();
    });
    strip.addEventListener('drop', (event) => {
      event.preventDefault();
      const key = this.dragKey;
      const index = this.dropIndex[group];
      this.clearDropMarks();
      if (key) this.moveTab(key, group, index);
    });
    body.addEventListener('dragover', (event) => {
      if (!this.dragKey) return;
      event.preventDefault();
      body.classList.add('drop-active');
    });
    body.addEventListener('dragleave', (event) => {
      if (!body.contains(event.relatedTarget as Node | null)) body.classList.remove('drop-active');
    });
    body.addEventListener('drop', (event) => {
      event.preventDefault();
      body.classList.remove('drop-active');
      const key = this.dragKey;
      this.clearDropMarks();
      if (key) this.moveTab(key, group, this.groups[group].tabs.length);
    });
  }

  private indexAt(strip: HTMLElement, clientX: number): number {
    const buttons = [...strip.querySelectorAll<HTMLElement>('.ws-tab')];
    for (let index = 0; index < buttons.length; index += 1) {
      const rect = buttons[index].getBoundingClientRect();
      if (clientX < rect.left + rect.width / 2) return index;
    }
    return buttons.length;
  }

  private markDrop(group: GroupId, index: number): void {
    this.clearDropMarks();
    const buttons = [...this.strips[group].querySelectorAll<HTMLElement>('.ws-tab')];
    const target = buttons[index];
    if (target) target.classList.add('drop-before');
    else this.strips[group].classList.add('drop-end');
  }

  private clearDropMarks(): void {
    for (const group of ['a', 'b'] as const) {
      this.strips[group].classList.remove('drop-end');
      for (const button of this.strips[group].querySelectorAll('.ws-tab')) button.classList.remove('drop-before');
      this.bodies[group].classList.remove('drop-active');
    }
  }

  private moveTab(key: string, group: GroupId, targetIndex: number): void {
    const tab = this.findTab(key);
    if (!tab) return;
    const from = this.groups[tab.group];
    const fromIndex = from.tabs.indexOf(tab);
    let index = Math.max(0, Math.min(targetIndex, this.groups[group].tabs.length));
    from.tabs.splice(fromIndex, 1);
    if (tab.group === group) {
      if (index > fromIndex) index -= 1;
      if (from.active === key) from.active = null;
    } else if (from.active === key) {
      from.active = from.tabs[Math.min(fromIndex, from.tabs.length - 1)]?.key ?? null;
    }
    tab.group = group;
    this.groups[group].tabs.splice(index, 0, tab);
    this.groups[group].active = key;
    this.focused = group;
    this.placeTab(tab);
    this.paint();
    this.scheduleSave();
  }

  private toggleSplit(): void {
    if (this.split) {
      const moved = this.groups.b.tabs;
      this.groups.b.active = null;
      if (moved.length) {
        this.groups.b.tabs = [];
        for (const tab of moved) {
          tab.group = 'a';
          this.groups.a.tabs.push(tab);
          this.groups.a.active = tab.key;
          this.placeTab(tab);
        }
        this.focused = 'a';
      } else {
        this.focused = this.focused === 'b' ? 'a' : this.focused;
      }
      this.split = false;
    } else {
      this.split = true;
      this.focused = this.groups[this.focused].active ? this.focused : 'a';
    }
    this.paint();
    this.scheduleSave();
  }

  private createTab(source: WorkspaceSource, group: GroupId, activate: boolean): Tab | null {
    const key = keyOf(source);
    const existing = this.findTab(key);
    if (existing) {
      if (activate) this.activate(existing);
      return existing;
    }
    if (this.tabs().length >= MAX_TABS) {
      const victim = this.tabs().find((tab) => tab.key !== this.focusedTab()?.key) ?? this.tabs()[0];
      if (victim) this.closeTab(victim.key);
      this.hint = `${t('最多同时打开')}${MAX_TABS}${t('个标签，已关闭最早的标签')}`;
    }
    const pane = document.createElement('div');
    pane.className = 'ws-pane';
    const button = document.createElement('div');
    button.className = 'ws-tab';
    button.setAttribute('role', 'tab');
    button.dataset.wsKey = key;
    button.draggable = true;
    const label = document.createElement('span');
    label.className = 'ws-tab-title';
    const title = titleOf(source);
    label.textContent = title;
    const close = document.createElement('button');
    close.type = 'button';
    close.className = 'ws-tab-close';
    close.setAttribute('aria-label', `${t('关闭')}${title}`);
    close.textContent = '×';
    close.addEventListener('click', (event) => { event.stopPropagation(); this.closeTab(key); });
    button.append(label, close);
    button.addEventListener('click', () => {
      const tab = this.findTab(key);
      if (tab) this.activate(tab);
    });
    button.addEventListener('auxclick', (event) => { if (event.button === 1) { event.preventDefault(); this.closeTab(key); } });
    button.addEventListener('dragstart', (event) => {
      this.dragKey = key;
      button.classList.add('dragging');
      event.dataTransfer?.setData('text/plain', key);
      if (event.dataTransfer) event.dataTransfer.effectAllowed = 'move';
    });
    button.addEventListener('dragend', () => {
      button.classList.remove('dragging');
      this.dragKey = null;
      this.clearDropMarks();
    });
    const tab: Tab = {
      key, source, group, title, pane, button, actions: {}, kind: 'loading',
      revoke: null, destroy: null, reload: null, url: source.kind === 'web' ? source.url : '',
    };
    this.groups[group].tabs.push(tab);
    this.placeTab(tab);
    if (activate) {
      this.groups[group].active = key;
      this.focused = group;
    }
    this.paint();
    this.load(tab);
    if (this.canSave) this.scheduleSave();
    return tab;
  }

  private placeTab(tab: Tab): void {
    if (tab.button.parentElement !== this.strips[tab.group]) this.strips[tab.group].append(tab.button);
    if (tab.pane.parentElement !== this.bodies[tab.group]) this.bodies[tab.group].append(tab.pane);
  }

  private load(tab: Tab): void {
    if (tab.source.kind === 'web') {
      this.paintWeb(tab, tab.source.url);
      return;
    }
    const resolver = this.resolver;
    if (!resolver) {
      this.paintContent(tab, { kind: 'unsupported', reason: t('预览模块尚未就绪，请稍后重试') });
      return;
    }
    void resolver(tab.source)
      .then((resolved) => {
        if (!this.tabs().includes(tab)) return;
        if (resolved.title && resolved.title !== tab.title) {
          tab.title = resolved.title;
          const label = tab.button.querySelector('.ws-tab-title');
          if (label) label.textContent = resolved.title;
        }
        tab.actions = resolved.actions ?? {};
        this.paintContent(tab, resolved.loaded);
      })
      .catch((error: unknown) => {
        if (!this.tabs().includes(tab)) return;
        this.paintContent(tab, { kind: 'error', message: trError(error, t('无法读取该文件')) });
      });
  }

  private activate(tab: Tab): void {
    this.groups[tab.group].active = tab.key;
    this.focused = tab.group;
    this.paint();
    this.scheduleSave();
  }

  private closeTab(key: string): void {
    const tab = this.findTab(key);
    if (!tab) return;
    const group = this.groups[tab.group];
    const index = group.tabs.indexOf(tab);
    this.dispose(tab);
    group.tabs.splice(index, 1);
    tab.button.remove();
    tab.pane.remove();
    if (group.active === key) group.active = group.tabs[Math.min(index, group.tabs.length - 1)]?.key ?? null;
    if (this.focused === tab.group && !group.active) {
      const other: GroupId = tab.group === 'a' ? 'b' : 'a';
      this.focused = this.groups[other].active ? other : 'a';
    }
    this.paint();
    this.scheduleSave();
  }

  private dispose(tab: Tab): void {
    tab.destroy?.();
    tab.destroy = null;
    if (tab.revoke) { tab.revoke(); tab.revoke = null; }
    tab.reload = null;
  }

  private findTab(key: string): Tab | null {
    return this.tabs().find((tab) => tab.key === key) ?? null;
  }

  private tabs(): Tab[] {
    return [...this.groups.a.tabs, ...this.groups.b.tabs];
  }

  private focusedTab(): Tab | null {
    const active = this.groups[this.focused].active;
    if (active) return this.findTab(active);
    const fallback = this.groups[this.focused === 'a' ? 'b' : 'a'].active;
    return fallback ? this.findTab(fallback) : null;
  }

  private paint(): void {
    if (!this.root) return;
    const total = this.tabs().length;
    this.root.hidden = total === 0;
    this.root.classList.toggle('split', this.split && total > 0);
    this.root.dataset.focus = this.focused;
    if (!total) {
      this.root.hidden = true;
      for (const group of ['a', 'b'] as const) {
        this.strips[group].replaceChildren();
        this.bodies[group].replaceChildren();
      }
      this.toolbarEl?.replaceChildren();
      if (this.hintEl) this.hintEl.textContent = '';
      this.hint = '';
      this.applyLayout();
      return;
    }
    if (this.splitButton) this.splitButton.textContent = this.split ? t('取消分屏') : t('分屏');
    for (const group of ['a', 'b'] as const) {
      const visible = group === 'a' || this.split;
      const strip = this.strips[group];
      const body = this.bodies[group];
      strip.parentElement?.toggleAttribute('hidden', !visible);
      body.hidden = !visible;
      this.renderEmpty(group);
      for (const tab of this.groups[group].tabs) {
        const active = tab.key === this.groups[group].active;
        tab.button.classList.toggle('active', active);
        tab.button.setAttribute('aria-selected', active ? 'true' : 'false');
        tab.pane.hidden = !active;
      }
      const focused = this.focused === group;
      this.strips[group].parentElement?.classList.toggle('focused', focused);
    }
    this.paintToolbar(this.focusedTab());
    if (this.hintEl) this.hintEl.textContent = this.hint;
    this.hint = '';
    this.applyLayout();
    const activeTab = this.focusedTab();
    activeTab?.button.scrollIntoView({ block: 'nearest', inline: 'nearest' });
  }

  private renderEmpty(group: GroupId): void {
    const body = this.bodies[group];
    const existing = body.querySelector<HTMLElement>(':scope > .ws-empty');
    const empty = this.groups[group].tabs.length === 0;
    if (!empty) { existing?.remove(); return; }
    if (existing) return;
    const placeholder = document.createElement('p');
    placeholder.className = 'ws-empty';
    placeholder.textContent = group === 'b' ? t('拖一个标签到这里，左右并排查看') : t('这个分屏组还没有标签，可把标签拖进来');
    body.append(placeholder);
  }

  private paintToolbar(tab: Tab | null): void {
    if (!this.toolbarEl) return;
    this.toolbarEl.replaceChildren();
    if (!tab) return;
    const title = document.createElement('strong');
    title.className = 'ws-toolbar-title';
    title.textContent = tab.title;
    this.toolbarEl.append(title);
    const actions = document.createElement('div');
    actions.className = 'ws-toolbar-actions';
    const add = (label: string, run: () => void) => {
      const button = document.createElement('button');
      button.type = 'button';
      button.className = 'outline';
      button.textContent = label;
      button.addEventListener('click', run);
      actions.append(button);
    };
    if (tab.url) {
      add(t('刷新'), () => this.reloadWeb(tab));
      add(t('在系统浏览器打开'), () => this.openExternal(tab.url));
    }
    if (tab.actions.systemOpen) add(t('用默认程序打开'), () => tab.actions.systemOpen?.());
    if (tab.actions.reveal) add(t('打开所在目录'), () => tab.actions.reveal?.());
    this.toolbarEl.append(actions);
  }

  private paintContent(tab: Tab, loaded: LoadedTab): void {
    tab.kind = loaded.kind;
    tab.destroy?.();
    tab.destroy = null;
    if (tab.revoke) { tab.revoke(); tab.revoke = null; }
    tab.pane.replaceChildren();
    tab.pane.classList.remove('is-media', 'is-web');
    if (loaded.kind === 'image') {
      tab.pane.classList.add('is-media');
      const viewer = attachImageViewer(loaded.src, tab.title);
      tab.destroy = viewer.destroy;
      tab.pane.append(viewer.element);
    } else if (loaded.kind === 'pdf') {
      tab.pane.classList.add('is-media');
      const blob = new Blob([loaded.bytes], { type: 'application/pdf' });
      const url = URL.createObjectURL(blob);
      tab.revoke = () => URL.revokeObjectURL(url);
      const frame = document.createElement('iframe');
      frame.className = 'ws-pdf';
      frame.src = url;
      frame.setAttribute('referrerpolicy', 'no-referrer');
      frame.title = tab.title;
      tab.pane.append(frame);
      const tip = document.createElement('p');
      tip.className = 'ws-note';
      tip.textContent = t('PDF 由系统 WebView2 内置阅读器渲染；若显示空白，请使用“用默认程序打开”。');
      tab.pane.append(tip);
    } else if (loaded.kind === 'text') {
      const pre = document.createElement('pre');
      pre.className = 'ws-text';
      pre.textContent = loaded.text;
      tab.pane.append(pre);
    } else if (loaded.kind === 'markdown') {
      const article = document.createElement('article');
      article.className = 'ws-markdown';
      article.innerHTML = markdownToHtml(loaded.text);
      tab.pane.append(article);
    } else if (loaded.kind === 'html') {
      const frame = document.createElement('iframe');
      frame.className = 'ws-html';
      frame.setAttribute('sandbox', '');
      frame.setAttribute('referrerpolicy', 'no-referrer');
      frame.srcdoc = loaded.text;
      tab.pane.append(frame);
      const tip = document.createElement('p');
      tip.className = 'ws-note';
      tip.textContent = t('HTML 在禁用脚本的沙箱中预览；页面样式可能受应用安全策略限制。');
      tab.pane.append(tip);
    } else if (loaded.kind === 'plugin') {
      tab.pane.append(createPluginFrame(loaded.html, loaded.bytes, tab.title));
      const tip = document.createElement('p');
      tip.className = 'ws-note';
      tip.textContent = t('插件在沙箱中运行，只能读取这个文件的字节；第一版为只读预览。');
      tab.pane.append(tip);
    } else if (loaded.kind === 'unsupported') {
      tab.pane.append(note(loaded.reason));
    } else if (loaded.kind === 'error') {
      tab.pane.append(note(loaded.message));
    }
    this.paintToolbar(this.focusedTab());
  }

  private paintWeb(tab: Tab, url: string): void {
    tab.kind = 'web';
    tab.url = url;
    tab.pane.replaceChildren();
    tab.pane.classList.remove('is-media');
    tab.pane.classList.add('is-web');
    const frame = document.createElement('iframe');
    frame.className = 'ws-web';
    frame.setAttribute('sandbox', 'allow-scripts allow-same-origin allow-forms');
    frame.setAttribute('referrerpolicy', 'no-referrer');
    frame.src = url;
    frame.title = tab.title;
    const warning = note(t('页面仍在加载；若长时间空白，说明站点可能禁止嵌入，或当前网络不可达。'));
    warning.hidden = true;
    let loaded = false;
    const timer = window.setTimeout(() => { if (!loaded) warning.hidden = false; }, WEB_LOAD_TIMEOUT);
    frame.addEventListener('load', () => { loaded = true; window.clearTimeout(timer); warning.hidden = true; });
    tab.pane.append(frame, warning);
    tab.destroy = () => window.clearTimeout(timer);
    tab.reload = () => this.reloadWeb(tab);
    this.paintToolbar(this.focusedTab());
  }

  private reloadWeb(tab: Tab): void {
    const iframe = tab.pane.querySelector<HTMLIFrameElement>('iframe');
    if (iframe) iframe.src = tab.url;
  }

  private openExternal(url: string): void {
    if (!url) return;
    void openExternalUrl(url);
  }

  private startResize(event: PointerEvent): void {
    event.preventDefault();
    const startX = event.clientX;
    const startWidth = this.width;
    const move = (moveEvent: PointerEvent) => {
      this.width = Math.min(window.innerWidth * 0.7, Math.max(MIN_WIDTH, startWidth + (startX - moveEvent.clientX)));
      this.applyLayout();
    };
    const stop = () => {
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', stop);
      this.scheduleSave();
    };
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', stop);
  }

  private applyLayout(): void {
    const open = this.tabs().length > 0;
    document.body.classList.toggle('ws-open', open);
    document.body.style.setProperty('--ws-width', `${this.width}px`);
  }

  private scheduleSave(): void {
    if (!this.canSave) return;
    window.clearTimeout(this.saveTimer);
    this.saveTimer = window.setTimeout(() => void this.save(), 300);
  }

  private serialize(): PersistedState {
    const group = (id: GroupId) => ({
      active: this.groups[id].active,
      tabs: this.groups[id].tabs.map((tab) => ({ key: tab.key, source: tab.source })),
    });
    return { version: 1, width: this.width, split: this.split, focused: this.focused, groups: { a: group('a'), b: group('b') } };
  }

  private async save(): Promise<void> {
    if (!isTauri()) return;
    const payload = JSON.stringify(this.serialize());
    if (payload.length > MAX_STATE_BYTES) return;
    try { await invoke('v1_set_workspace_state', { payload }); } catch { /* layout persistence is best effort */ }
  }
}

function note(text: string): HTMLElement {
  const element = document.createElement('p');
  element.className = 'ws-note';
  element.textContent = text;
  return element;
}

function keyOf(source: WorkspaceSource): string {
  if (source.kind === 'direct-file') return `d:${source.messageId}`;
  if (source.kind === 'group-file') return `g:${source.groupId}:${source.messageId}`;
  return `url:${source.url}`;
}

function titleOf(source: WorkspaceSource): string {
  return source.kind === 'web' ? safeHost(source.url) : source.name;
}

function validSource(value: unknown): WorkspaceSource | null {
  if (!value || typeof value !== 'object') return null;
  const source = value as Partial<WorkspaceSource> & Record<string, unknown>;
  if (source.kind === 'direct-file' && typeof source.messageId === 'string' && typeof source.name === 'string') {
    return { kind: 'direct-file', messageId: source.messageId, name: source.name };
  }
  if (source.kind === 'group-file' && typeof source.groupId === 'string'
    && typeof source.messageId === 'string' && typeof source.name === 'string') {
    return { kind: 'group-file', groupId: source.groupId, messageId: source.messageId, name: source.name };
  }
  if (source.kind === 'web' && typeof source.url === 'string') {
    const url = normalizeUrl(source.url);
    return url ? { kind: 'web', url } : null;
  }
  return null;
}

function parseState(raw: string): PersistedState | null {
  try {
    const value = JSON.parse(raw) as {
      version?: unknown;
      width?: unknown;
      split?: unknown;
      focused?: unknown;
      groups?: Record<string, { active?: unknown; tabs?: unknown } | undefined>;
    };
    if (!value || value.version !== 1 || !value.groups) return null;
    const group = (id: GroupId): { active: string | null; tabs: PersistedTab[] } => {
      const entry = value.groups?.[id];
      const tabs: PersistedTab[] = [];
      if (Array.isArray(entry?.tabs)) {
        for (const item of entry.tabs) {
          const source = validSource((item as { source?: unknown } | null)?.source);
          if (source) tabs.push({ key: keyOf(source), source });
        }
      }
      return { active: typeof entry?.active === 'string' ? entry.active : null, tabs };
    };
    const a = group('a');
    const b = group('b');
    if (a.tabs.length + b.tabs.length > MAX_TABS) return null;
    const width = typeof value.width === 'number' && Number.isFinite(value.width) ? value.width : DEFAULT_WIDTH;
    return {
      version: 1,
      width,
      split: value.split === true,
      focused: value.focused === 'b' ? 'b' : 'a',
      groups: { a, b },
    };
  } catch { return null; }
}

function normalizeUrl(raw: string): string {
  const trimmed = raw.trim();
  if (!trimmed) return '';
  const candidate = /^www\./i.test(trimmed) ? `https://${trimmed}` : trimmed;
  try {
    const parsed = new URL(candidate);
    if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') return '';
    return parsed.href;
  } catch { return ''; }
}

function safeHost(url: string): string {
  try { return new URL(url).host || url; } catch { return url; }
}

async function openExternalUrl(url: string): Promise<void> {
  if (!isTauri()) { window.open(url, '_blank', 'noopener'); return; }
  try { await invoke('v1_open_external_url', { url }); }
  catch { window.open(url, '_blank', 'noopener'); }
}

/// Minimal Markdown renderer reused by the workspace; links are rendered as text so
/// no remote content or raw HTML can enter the app origin.
function markdownToHtml(source: string): string {
  const escape = (value: string): string => value
    .replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
  const inline = (text: string): string => escape(text)
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
      if (code) { html.push(`<pre><code>${escape(code.join('\n'))}</code></pre>`); code = null; }
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
  if (code) html.push(`<pre><code>${escape(code.join('\n'))}</code></pre>`);
  return html.join('');
}

export const workspace = new Workspace();
