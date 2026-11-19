// Zoomable, rotatable and draggable image viewer shared by direct and group chats.
// It is a local UI overlay only: the source is a data URL or blob URL already in memory.
let viewer: HTMLDivElement | null = null;
let image: HTMLImageElement | null = null;
let scaleLabel: HTMLElement | null = null;
let scale = 1;
let rotation = 0;
let offsetX = 0;
let offsetY = 0;
let dragging = false;
let lastX = 0;
let lastY = 0;

export function showImage(src: string, alt = '图片'): void {
  ensureViewer();
  if (!viewer || !image) return;
  image.src = src;
  image.alt = alt;
  scale = 1; rotation = 0; offsetX = 0; offsetY = 0;
  apply();
  viewer.hidden = false;
}

function closeViewer(): void {
  if (viewer) viewer.hidden = true;
}

function ensureViewer(): void {
  if (viewer) return;
  viewer = document.createElement('div');
  viewer.className = 'image-viewer';
  viewer.hidden = true;
  viewer.innerHTML = `<div class="viewer-toolbar">
      <button type="button" data-viewer="zoom-out" aria-label="缩小">−</button>
      <span class="viewer-scale">100%</span>
      <button type="button" data-viewer="zoom-in" aria-label="放大">+</button>
      <button type="button" data-viewer="rotate">旋转</button>
      <button type="button" data-viewer="reset">复位</button>
      <button type="button" data-viewer="close">关闭</button>
    </div>
    <img class="viewer-image" alt="">`;
  document.body.append(viewer);
  image = viewer.querySelector<HTMLImageElement>('.viewer-image');
  scaleLabel = viewer.querySelector<HTMLElement>('.viewer-scale');
  viewer.addEventListener('click', (event) => {
    const target = event.target as HTMLElement;
    const action = target.dataset.viewer;
    if (action === 'close' || target === viewer) closeViewer();
    else if (action === 'zoom-in') zoom(1.25);
    else if (action === 'zoom-out') zoom(0.8);
    else if (action === 'rotate') { rotation = (rotation + 90) % 360; apply(); }
    else if (action === 'reset') { scale = 1; rotation = 0; offsetX = 0; offsetY = 0; apply(); }
  });
  viewer.addEventListener('wheel', (event) => {
    event.preventDefault();
    zoom(event.deltaY < 0 ? 1.1 : 0.9);
  }, { passive: false });
  viewer.addEventListener('pointerdown', (event) => {
    if ((event.target as HTMLElement).closest('.viewer-toolbar')) return;
    dragging = true;
    lastX = event.clientX;
    lastY = event.clientY;
    viewer?.setPointerCapture(event.pointerId);
  });
  viewer.addEventListener('pointermove', (event) => {
    if (!dragging) return;
    offsetX += event.clientX - lastX;
    offsetY += event.clientY - lastY;
    lastX = event.clientX;
    lastY = event.clientY;
    apply();
  });
  viewer.addEventListener('pointerup', () => { dragging = false; });
  viewer.addEventListener('pointercancel', () => { dragging = false; });
  window.addEventListener('keydown', (event) => {
    if (event.key === 'Escape' && viewer && !viewer.hidden) closeViewer();
  });
}

function zoom(factor: number): void {
  scale = Math.min(8, Math.max(0.2, scale * factor));
  apply();
}

function apply(): void {
  if (!image) return;
  image.style.transform = `translate(${offsetX}px, ${offsetY}px) scale(${scale}) rotate(${rotation}deg)`;
  if (scaleLabel) scaleLabel.textContent = `${Math.round(scale * 100)}%`;
}
