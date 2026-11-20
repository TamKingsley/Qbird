/// Shared line icons for the composer toolbar. One visual language, meaning supplied by
/// the button's aria-label/title, so the toolbar never turns into a row of text buttons.
const PATHS: Record<string, string> = {
  smile: '<circle cx="12" cy="12" r="8.4"/><path d="M8.6 14c.8 1.3 2 2 3.4 2s2.6-.7 3.4-2"/><path d="M9.2 9.6h.02M14.8 9.6h.02" stroke-width="2.4"/>',
  image: '<rect x="3.6" y="5" width="16.8" height="14" rx="2.4"/><circle cx="9" cy="10" r="1.5"/><path d="m4.6 17 4.6-4.6 3.3 3.3 2.5-2.4 4.4 4.2"/>',
  folder: '<path d="M3.6 7.2a2 2 0 0 1 2-2h3.6l1.8 2.2h7.4a2 2 0 0 1 2 2v8.4a2 2 0 0 1-2 2H5.6a2 2 0 0 1-2-2z"/>',
  crop: '<path d="M7 3.6v13.8h13.4M3.6 7h13.8v13.4"/>',
  mic: '<rect x="9.2" y="3.6" width="5.6" height="10.4" rx="2.8"/><path d="M5.8 11.4a6.2 6.2 0 0 0 12.4 0M12 17.6v2.8"/>',
  sticker: '<path d="M12 3.8a8.2 8.2 0 1 0 8.2 8.2H12z"/><path d="M13.6 3.9a7 7 0 0 1 6.5 6.4h-6.5z"/>',
};

export function toolIcon(name: string): string {
  return `<svg class="tool-icon" viewBox="0 0 24 24" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">${PATHS[name] ?? ''}</svg>`;
}

/// The composer toolbar shared by direct, group and self chat.
export function composerTools(sendAction: string, hint: string, media = true): string {
  return `<div class="compose-toolbar">
    <div class="tool-left">
      <button type="button" class="tool-btn" data-action="toggle-emoji" aria-label="表情" title="表情" aria-expanded="false">${toolIcon('smile')}</button>
      ${media ? `<button type="button" class="tool-btn" data-action="${sendAction}-image" aria-label="图片" title="图片">${toolIcon('image')}</button>
      <button type="button" class="tool-btn" data-action="${sendAction}-file" aria-label="文件" title="文件">${toolIcon('folder')}</button>
      <button type="button" class="tool-btn" data-action="${sendAction}-screenshot" aria-label="截图" title="截图">${toolIcon('crop')}</button>` : ''}
      <button type="button" class="tool-btn" aria-label="语音" title="语音（后续支持）" disabled>${toolIcon('mic')}</button>
    </div>
    <div class="tool-right">
      <span class="compose-hint">${hint}</span>
      <button type="button" class="send-btn" data-action="${sendAction}-send">发送</button>
    </div>
  </div>`;
}
