// Close-button prompt: the native window stays open until the user picks
// "minimize to tray" or "quit". The choice can be remembered, and the settings
// page can change the saved behavior later.
import { invoke } from '@tauri-apps/api/core';

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
      <span class="eyebrow">CLOSE / 关闭窗口</span>
      <h2 id="close-dialog-title">最小化到托盘，还是退出 Qbird？</h2>
      <p>最小化到托盘后 Qbird 仍在后台运行，可以继续接收消息；随时点击系统托盘图标重新打开。</p>
      <label class="close-dialog-remember"><input id="close-dialog-remember" type="checkbox" checked> 记住我的选择，下次不再询问（可在设置中修改）</label>
      <div class="close-dialog-actions">
        <button type="button" class="outline" data-close="tray">最小化到托盘</button>
        <button type="button" class="outline danger" data-close="quit">退出 Qbird</button>
        <button type="button" class="linkish" data-close="cancel">取消</button>
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
