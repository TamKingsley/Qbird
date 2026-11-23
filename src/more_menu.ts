export interface MoreMenuEntry {
  labelKey: string;
  page: string;
  settingsTab?: string;
}

// Visible More menu entries. Identity-level user backup/restore stay hidden
// until those flows are implemented, so they must not appear here.
export const MORE_MENU_ENTRIES: readonly MoreMenuEntry[] = [
  { labelKey: 'menu.settings', page: 'settings' },
  { labelKey: 'menu.chatBackup', page: 'settings', settingsTab: 'privacy' },
  { labelKey: 'menu.chatRestore', page: 'settings', settingsTab: 'privacy' },
  { labelKey: 'menu.about', page: 'about' },
];

export function moreMenuHtml(t: (key: string) => string): string {
  return MORE_MENU_ENTRIES.map((entry) => {
    const tab = entry.settingsTab ? ` data-settings-tab="${entry.settingsTab}"` : '';
    return `<button data-page="${entry.page}"${tab} role="menuitem">${t(entry.labelKey)}</button>`;
  }).join('');
}
