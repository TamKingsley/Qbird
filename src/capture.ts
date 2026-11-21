// PixPin-style simplified region capture.
// The Rust side hides the main window, grabs the screen and opens this frameless
// fullscreen window. A region can be drawn, moved and resized before it is cropped
// and handed back to the chat as a staged attachment (never sent directly).
import { invoke } from '@tauri-apps/api/core';
import './capture.css';

type Capture = { data_url: string; width: number; height: number };
type Rect = { x: number; y: number; w: number; h: number };
type Mode = 'idle' | 'draw' | 'move' | 'resize';

const root = document.querySelector<HTMLDivElement>('#capture-root');
if (!root) throw new Error('capture root missing');


const image = document.createElement('img');
image.className = 'shot';
image.alt = 'Screen capture';
root.append(image);

const selection = document.createElement('div');
selection.className = 'selection';
selection.innerHTML = '<span class="size"></span>' +
  ['nw', 'n', 'ne', 'e', 'se', 's', 'sw', 'w']
    .map((dir) => `<div class="handle" data-dir="${dir}"></div>`).join('');
root.append(selection);

const toolbar = document.createElement('div');
toolbar.className = 'toolbar';
toolbar.innerHTML = '<span class="hint">Drag to select · Drag edges to resize · Enter to confirm · Esc to cancel</span>' +
  '<button type="button" data-act="cancel">Cancel</button>' +
  '<button type="button" class="primary" data-act="confirm">Confirm and add to composer</button>';
root.append(toolbar);

const loading = document.createElement('div');
loading.className = 'loading';
loading.textContent = 'Preparing screenshot…';
root.append(loading);

const sizeLabel = selection.querySelector<HTMLElement>('.size');

let shot: Capture | null = null;
let rect: Rect = { x: 0, y: 0, w: 0, h: 0 };
let mode: Mode = 'idle';
let dragStart = { x: 0, y: 0 };
let baseRect: Rect = { ...rect };
let handleDir = '';

const clamp = (value: number, min: number, max: number): number => Math.min(Math.max(value, min), max);

function paint(): void {
  if (rect.w < 1 || rect.h < 1) { selection.classList.remove('visible'); return; }
  selection.classList.add('visible');
  selection.style.left = `${rect.x}px`;
  selection.style.top = `${rect.y}px`;
  selection.style.width = `${rect.w}px`;
  selection.style.height = `${rect.h}px`;
  if (sizeLabel && shot) {
    const sx = shot.width / window.innerWidth, sy = shot.height / window.innerHeight;
    sizeLabel.textContent = `${Math.round(rect.w * sx)} × ${Math.round(rect.h * sy)}`;
  }
}

function hit(event: PointerEvent): { type: Mode; dir: string } {
  const target = event.target as HTMLElement;
  const dir = target.dataset?.dir;
  if (dir) return { type: 'resize', dir };
  if (target.closest('.selection')) return { type: 'move', dir: '' };
  return { type: 'draw', dir: '' };
}

function onPointerDown(event: PointerEvent): void {
  if ((event.target as HTMLElement).closest('.toolbar')) return;
  if (event.button !== 0) return;
  const { type, dir } = hit(event);
  mode = type;
  handleDir = dir;
  dragStart = { x: event.clientX, y: event.clientY };
  baseRect = { ...rect };
  if (type === 'draw') {
    rect = { x: event.clientX, y: event.clientY, w: 0, h: 0 };
    paint();
  }
  (event.target as Element).setPointerCapture?.(event.pointerId);
}

function onPointerMove(event: PointerEvent): void {
  if (mode === 'idle') return;
  const dx = event.clientX - dragStart.x, dy = event.clientY - dragStart.y;
  if (mode === 'draw') {
    rect = {
      x: clamp(Math.min(dragStart.x, dragStart.x + dx), 0, window.innerWidth),
      y: clamp(Math.min(dragStart.y, dragStart.y + dy), 0, window.innerHeight),
      w: Math.abs(dx), h: Math.abs(dy),
    };
  } else if (mode === 'move') {
    rect = {
      ...baseRect,
      x: clamp(baseRect.x + dx, 0, window.innerWidth - baseRect.w),
      y: clamp(baseRect.y + dy, 0, window.innerHeight - baseRect.h),
    };
  } else if (mode === 'resize') {
    let left = baseRect.x, top = baseRect.y, right = baseRect.x + baseRect.w, bottom = baseRect.y + baseRect.h;
    if (handleDir.includes('w')) left = clamp(baseRect.x + dx, 0, right - 4);
    if (handleDir.includes('e')) right = clamp(right + dx, left + 4, window.innerWidth);
    if (handleDir.includes('n')) top = clamp(baseRect.y + dy, 0, bottom - 4);
    if (handleDir.includes('s')) bottom = clamp(bottom + dy, top + 4, window.innerHeight);
    rect = { x: left, y: top, w: right - left, h: bottom - top };
  }
  paint();
}

function onPointerUp(): void {
  if (mode === 'draw' && (rect.w < 6 || rect.h < 6)) {
    rect = { x: 0, y: 0, w: 0, h: 0 };
    paint();
  }
  mode = 'idle';
  handleDir = '';
}

let finishing = false;

async function finish(confirm: boolean): Promise<void> {
  if (finishing) return;
  finishing = true;
  if (!shot) { void invoke('v1_end_capture', { path: null }).catch(() => undefined); return; }
  if (!confirm || rect.w < 1 || rect.h < 1) {
    await invoke('v1_end_capture', { path: null }).catch(() => undefined);
    return;
  }
  try {
    const scaleX = shot.width / window.innerWidth, scaleY = shot.height / window.innerHeight;
    const canvas = document.createElement('canvas');
    canvas.width = Math.max(1, Math.round(rect.w * scaleX));
    canvas.height = Math.max(1, Math.round(rect.h * scaleY));
    const context = canvas.getContext('2d');
    if (!context) throw new Error('Could not crop the screenshot');
    context.drawImage(image, rect.x * scaleX, rect.y * scaleY, rect.w * scaleX, rect.h * scaleY,
      0, 0, canvas.width, canvas.height);
    const blob = await new Promise<Blob | null>((done) => canvas.toBlob(done, 'image/png'));
    if (!blob) throw new Error('Could not crop the screenshot');
    const bytes = Array.from(new Uint8Array(await blob.arrayBuffer()));
    const path = await invoke<string>('v1_save_temp_image', { bytes, extension: 'png' });
    await invoke('v1_end_capture', { path });
  } catch {
    await invoke('v1_end_capture', { path: null }).catch(() => undefined);
  }
}

document.addEventListener('pointerdown', onPointerDown);
document.addEventListener('pointermove', onPointerMove);
document.addEventListener('pointerup', onPointerUp);
document.addEventListener('dblclick', (event) => {
  if (mode === 'idle' && (event.target as HTMLElement).closest('.selection')) void finish(true);
});
toolbar.addEventListener('click', (event) => {
  const act = (event.target as HTMLElement).closest<HTMLElement>('[data-act]')?.dataset.act;
  if (act === 'confirm') void finish(true);
  else if (act === 'cancel') void finish(false);
});
window.addEventListener('keydown', (event) => {
  if (event.key === 'Escape') { event.preventDefault(); void finish(false); }
  else if (event.key === 'Enter') { event.preventDefault(); void finish(true); }
});

async function start(): Promise<void> {
  try {
    shot = await invoke<Capture>('v1_capture_current');
  } catch {
    loading.textContent = 'Screenshot expired. Click the capture button again.';
    window.setTimeout(() => { void invoke('v1_end_capture', { path: null }).catch(() => undefined); }, 1200);
    return;
  }
  image.src = shot.data_url;
  await image.decode().catch(() => undefined);
  loading.remove();
}

void start();
