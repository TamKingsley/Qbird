// Close-button prompt: the native window stays open until the user picks
// "minimize to tray" or "quit". The choice can be remembered, and the settings
// page can change the saved behavior later.
import { invoke } from '@tauri-apps/api/core';
import { t } from './i18n';

export type CloseChoice = 'tray' | 'quit';

let prompt: HTMLDivElement | null = null;

export function dismissClosePrompt(): void {
  prompt?.remove();
  prompt = null;
}

export function showClosePrompt(onDone?: (choice: CloseChoice | null) => void): void {
  if (prompt) return;
  prompt = document.createElement('div');
  prompt.className = 'close-dialog-backdrop';
  prompt.innerHTML = `
    <div class="close-dialog" role="dialog" aria-modal="true" aria-labelledby="close-dialog-title">
      <span class="eyebrow">${t('close.eyebrow')}</span>
      <h2 id="close-dialog-title">${t('close.title')}</h2>
      <p>${t('close.body')}</p>
      <label class="close-dialog-remember"><input id="close-dialog-remember" type="checkbox" checked> ${t('close.remember')}</label>
      <div class="close-dialog-actions">
        <button type="button" class="outline" data-close="tray">${t('close.tray')}</button>
        <button type="button" class="outline danger" data-close="quit">${t('close.quit')}</button>
        <button type="button" class="linkish" data-close="cancel">${t('close.cancel')}</button>
      </div>
    </div>`;
  document.body.append(prompt);

  const finish = (choice: CloseChoice | null): void => {
    const remember = prompt?.querySelector<HTMLInputElement>('#close-dialog-remember')?.checked ?? true;
    dismissClosePrompt();
    if (choice) {
      // A failed IPC keeps the window open, because the close was only prevented.
      void invoke('v1_resolve_window_close', { action: choice, remember }).catch(() => undefined);
    }
    onDone?.(choice);
  };

  prompt.addEventListener('click', (event) => {
    const choice = (event.target as HTMLElement).closest<HTMLElement>('[data-close]')?.dataset.close;
    if (choice === 'tray' || choice === 'quit') finish(choice);
    else if (choice === 'cancel') finish(null);
  });
  prompt.addEventListener('keydown', (event) => {
    if (event.key === 'Escape') finish(null);
  });
  prompt.querySelector<HTMLButtonElement>('[data-close="tray"]')?.focus();
}
