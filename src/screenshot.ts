// Screen capture with a drag-to-select overlay. The capture itself happens in Rust;
// this module only crops the chosen rectangle on a canvas and saves it as a temp PNG.
import { invoke } from '@tauri-apps/api/core';

type Capture = { data_url: string; width: number; height: number };

/// Returns the temp path of the cropped PNG, or null when the user cancels.
export async function captureRegion(): Promise<string | null> {
  const capture = await invoke<Capture>('v1_capture_screen');
  const png = await selectRegion(capture);
  if (!png) return null;
  return invoke<string>('v1_save_temp_image', { bytes: Array.from(png), extension: 'png' });
}

function selectRegion(capture: Capture): Promise<Uint8Array | null> {
  return new Promise((resolve) => {
    const overlay = document.createElement('div');
    overlay.className = 'capture-overlay';
    overlay.innerHTML = '<img class="capture-shot" alt="屏幕截图"><div class="capture-box" hidden></div><p class="capture-hint">拖动选择区域 · Esc 取消</p>';
    document.body.append(overlay);
    const image = overlay.querySelector<HTMLImageElement>('.capture-shot');
    const box = overlay.querySelector<HTMLElement>('.capture-box');
    if (!image || !box) { overlay.remove(); resolve(null); return; }
    image.src = capture.data_url;
    let startX = 0, startY = 0, dragging = false;
    let current = { x: 0, y: 0, w: 0, h: 0 };
    const cleanup = () => {
      overlay.remove();
      window.removeEventListener('keydown', onKey);
    };
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') { cleanup(); resolve(null); }
    };
    window.addEventListener('keydown', onKey);
    overlay.addEventListener('pointerdown', (event) => {
      dragging = true;
      startX = event.clientX;
      startY = event.clientY;
      box.hidden = false;
      overlay.setPointerCapture(event.pointerId);
    });
    overlay.addEventListener('pointermove', (event) => {
      if (!dragging) return;
      const x = Math.min(startX, event.clientX);
      const y = Math.min(startY, event.clientY);
      const w = Math.abs(event.clientX - startX);
      const h = Math.abs(event.clientY - startY);
      current = { x, y, w, h };
      box.style.left = `${x}px`;
      box.style.top = `${y}px`;
      box.style.width = `${w}px`;
      box.style.height = `${h}px`;
    });
    overlay.addEventListener('pointerup', async () => {
      dragging = false;
      if (current.w < 8 || current.h < 8) { box.hidden = true; return; }
      const scaleX = capture.width / window.innerWidth;
      const scaleY = capture.height / window.innerHeight;
      const canvas = document.createElement('canvas');
      canvas.width = Math.max(1, Math.round(current.w * scaleX));
      canvas.height = Math.max(1, Math.round(current.h * scaleY));
      const context = canvas.getContext('2d');
      if (!context) { cleanup(); resolve(null); return; }
      context.drawImage(image, current.x * scaleX, current.y * scaleY, current.w * scaleX, current.h * scaleY, 0, 0, canvas.width, canvas.height);
      const blob = await new Promise<Blob | null>((done) => canvas.toBlob(done, 'image/png'));
      cleanup();
      if (!blob) { resolve(null); return; }
      resolve(new Uint8Array(await blob.arrayBuffer()));
    });
  });
}
