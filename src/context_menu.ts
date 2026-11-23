/// One small right-click menu shared by chats. Only the actions passed in can run;
/// attachment paths are always resolved again on the backend from the message id.
import { t } from './i18n';

export type ContextItem = { label: string; run: () => void; danger?: boolean; disabled?: boolean };

export function recallItems(own: boolean, within: boolean, run: () => void): ContextItem[] {
  const noop = () => {};
  if (!own) return [{ label: t('只能撤回自己发送的消息'), disabled: true, run: noop }];
  return [
    { label: t('撤回'), danger: true, disabled: !within, run },
    { label: within ? t('仅 2 分钟内的消息可撤回') : t('超过 2 分钟，不能再撤回'), disabled: true, run: noop },
  ];
}

let menu: HTMLElement | null = null;

function close(): void {
  if (menu) { menu.remove(); menu = null; }
}

export function showContextMenu(event: MouseEvent, items: ContextItem[]): void {
  event.preventDefault();
  event.stopPropagation();
  close();
  if (!items.length) return;
  const box = document.createElement('div');
  box.className = 'context-menu';
  box.setAttribute('role', 'menu');
  for (const item of items) {
    const button = document.createElement('button');
    button.type = 'button';
    button.setAttribute('role', 'menuitem');
    button.disabled = !!item.disabled;
    if (item.danger) button.className = 'danger';
    button.textContent = item.label;
    button.addEventListener('click', (clickEvent) => {
      clickEvent.stopPropagation();
      close();
      item.run();
    });
    box.append(button);
  }
  document.body.append(box);
  const width = 200, height = items.length * 32 + 12;
  box.style.left = `${Math.min(event.clientX, Math.max(8, window.innerWidth - width - 8))}px`;
  box.style.top = `${Math.min(event.clientY, Math.max(8, window.innerHeight - height - 8))}px`;
  menu = box;
}

document.addEventListener('pointerdown', (event) => {
  if (menu && !menu.contains(event.target as Node)) close();
}, true);

document.addEventListener('keydown', (event) => {
  if (event.key === 'Escape') close();
});

window.addEventListener('blur', close);
