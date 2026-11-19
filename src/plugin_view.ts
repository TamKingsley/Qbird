/// Shared helpers to run a preview plugin inside a locked-down frame.
/// The plugin gets only the file bytes it is previewing: no origin, no network,
/// no paths and no access to the settings or chat database.
export function base64ToBytes(text: string): Uint8Array {
  const binary = atob(text);
  const bytes = new Uint8Array(binary.length);
  for (let index = 0; index < binary.length; index += 1) bytes[index] = binary.charCodeAt(index);
  return bytes;
}

/// Injects a policy that blocks network and same-origin access before the plugin runs.
export function pluginDocument(html: string): string {
  const policy = "<meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; style-src 'unsafe-inline'; script-src 'unsafe-inline'; img-src data: blob:; media-src data: blob:; font-src data:\">";
  const head = /<head[^>]*>/i.exec(html);
  if (head) {
    const at = head.index + head[0].length;
    return html.slice(0, at) + policy + html.slice(at);
  }
  const htmlTag = /<html[^>]*>/i.exec(html);
  if (htmlTag) {
    const at = htmlTag.index + htmlTag[0].length;
    return html.slice(0, at) + `<head>${policy}</head>` + html.slice(at);
  }
  return `${policy}${html}`;
}

export const PLUGIN_HANDSHAKE = 'qbird:preview';

export function createPluginFrame(html: string, bytes: Uint8Array, name: string): HTMLIFrameElement {
  const frame = document.createElement('iframe');
  frame.className = 'plugin-preview';
  frame.setAttribute('sandbox', 'allow-scripts');
  frame.setAttribute('referrerpolicy', 'no-referrer');
  frame.srcdoc = pluginDocument(html);
  // A copy keeps the caller's buffer intact; the frame only ever sees this preview.
  const copy = bytes.slice();
  frame.addEventListener('load', () => {
    frame.contentWindow?.postMessage({ type: PLUGIN_HANDSHAKE, name, bytes: copy.buffer }, '*');
  });
  return frame;
}
