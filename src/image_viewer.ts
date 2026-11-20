// Zoomable, rotatable and draggable image viewer shared by the lightbox overlay and
// the right workspace. It only ever receives a data/blob URL already in memory.
export type ViewerHandle = {
  element: HTMLElement;
  destroy: () => void;
};

/// Builds a viewer that fills its container; used by the workspace image tab.
export function attachImageViewer(src: string, alt = '图片'): ViewerHandle {
  return buildViewer(src, alt, { overlay: false });
}

/// Kept for existing call sites: opens the full-screen lightbox.
export function showImage(src: string, alt = '图片'): void {
  const viewer = buildViewer(src, alt, { overlay: true });
  document.body.append(viewer.element);
  viewer.element.querySelector<HTMLButtonElement>('[data-viewer="close"]')?.focus();
  viewer.element.addEventListener('viewer-closed', () => viewer.destroy(), { once: true });
}

function buildViewer(src: string, alt: string, options: { overlay: boolean }): ViewerHandle {
  const root = document.createElement('div');
  root.className = options.overlay ? 'image-viewer' : 'ws-image-viewer';
  const toolbar = document.createElement('div');
  toolbar.className = 'viewer-toolbar';
  toolbar.innerHTML = `
    <button type="button" data-viewer="zoom-out" aria-label="缩小">−</button>
    <span class="viewer-scale">100%</span>
    <button type="button" data-viewer="zoom-in" aria-label="放大">+</button>
    <button type="button" data-viewer="rotate" aria-label="旋转 90 度">旋转</button>
    <button type="button" data-viewer="fit">适应</button>
    <button type="button" data-viewer="actual">100%</button>
    <button type="button" data-viewer="reset">复位</button>
    ${options.overlay ? '<button type="button" data-viewer="close">关闭</button>' : ''}`;
  const stage = document.createElement('div');
  stage.className = 'viewer-stage';
  const image = document.createElement('img');
  image.className = 'viewer-image';
  image.alt = alt;
  image.draggable = false;
  stage.append(image);
  root.append(toolbar, stage);

  let scale = 1;
  let fitScale = 1;
  let rotation = 0;
  let offsetX = 0;
  let offsetY = 0;
  let dragging = false;
  let lastX = 0;
  let lastY = 0;

  const scaleLabel = toolbar.querySelector<HTMLElement>('.viewer-scale');
  const apply = (): void => {
    image.style.transform = `translate(${offsetX}px, ${offsetY}px) scale(${scale}) rotate(${rotation}deg)`;
    if (scaleLabel) scaleLabel.textContent = `${Math.round(scale * 100)}%`;
  };

  const clampScale = (value: number): number => Math.min(8, Math.max(0.1, value));
  const zoomTo = (value: number): void => { scale = clampScale(value); apply(); };

  const computeFit = (): number => {
    const natural = image.naturalWidth && image.naturalHeight
      ? { width: image.naturalWidth, height: image.naturalHeight }
      : { width: stage.clientWidth || 1, height: stage.clientHeight || 1 };
    const quarter = rotation % 180 !== 0;
    const width = quarter ? natural.height : natural.width;
    const height = quarter ? natural.width : natural.height;
    const available = { width: Math.max(1, stage.clientWidth - 24), height: Math.max(1, stage.clientHeight - 24) };
    return Math.min(available.width / width, available.height / height, 1);
  };

  const fit = (): void => {
    fitScale = Math.max(0.05, computeFit());
    scale = fitScale;
    offsetX = 0;
    offsetY = 0;
    rotation = 0;
    apply();
  };

  const reset = (): void => fit();

  // Wheel zoom keeps the pointer roughly anchored by shifting the pan offset.
  const onWheel = (event: WheelEvent): void => {
    event.preventDefault();
    const factor = event.deltaY < 0 ? 1.12 : 0.89;
    const next = clampScale(scale * factor);
    if (rotation % 360 === 0) {
      const rect = stage.getBoundingClientRect();
      const px = event.clientX - rect.left - rect.width / 2 - offsetX;
      const py = event.clientY - rect.top - rect.height / 2 - offsetY;
      const ratio = next / scale;
      offsetX -= px * (ratio - 1);
      offsetY -= py * (ratio - 1);
    }
    scale = next;
    apply();
  };
  stage.addEventListener('wheel', onWheel, { passive: false });

  stage.addEventListener('pointerdown', (event) => {
    if (event.button !== 0) return;
    dragging = true;
    lastX = event.clientX;
    lastY = event.clientY;
    stage.setPointerCapture(event.pointerId);
    stage.classList.add('dragging');
  });
  stage.addEventListener('pointermove', (event) => {
    if (!dragging) return;
    offsetX += event.clientX - lastX;
    offsetY += event.clientY - lastY;
    lastX = event.clientX;
    lastY = event.clientY;
    apply();
  });
  const stopDrag = (): void => { dragging = false; stage.classList.remove('dragging'); };
  stage.addEventListener('pointerup', stopDrag);
  stage.addEventListener('pointercancel', stopDrag);
  stage.addEventListener('dblclick', () => {
    if (scale > fitScale * 1.01) { scale = fitScale; offsetX = 0; offsetY = 0; }
    else { scale = 1; offsetX = 0; offsetY = 0; }
    apply();
  });

  toolbar.addEventListener('click', (event) => {
    const target = event.target as HTMLElement;
    const action = target.closest<HTMLElement>('[data-viewer]')?.dataset.viewer;
    if (action === 'zoom-in') zoomTo(scale * 1.25);
    else if (action === 'zoom-out') zoomTo(scale * 0.8);
    else if (action === 'rotate') { rotation = (rotation + 90) % 360; offsetX = 0; offsetY = 0; apply(); }
    else if (action === 'fit') fit();
    else if (action === 'actual') { scale = 1; offsetX = 0; offsetY = 0; apply(); }
    else if (action === 'reset') reset();
    else if (action === 'close') root.dispatchEvent(new Event('viewer-closed'));
  });

  if (options.overlay) {
    root.addEventListener('click', (event) => { if (event.target === root) root.dispatchEvent(new Event('viewer-closed')); });
  }

  image.addEventListener('load', () => { fit(); });
  image.src = src;
  if (image.complete && image.naturalWidth) fit();

  const onKey = (event: KeyboardEvent): void => {
    if (event.key !== 'Escape') return;
    if (options.overlay) { root.dispatchEvent(new Event('viewer-closed')); return; }
    if (!root.isConnected || root.offsetParent === null) return;
    reset();
  };
  document.addEventListener('keydown', onKey);

  const onResize = (): void => { if (root.isConnected && root.offsetParent !== null) fit(); };
  window.addEventListener('resize', onResize);

  return {
    element: root,
    destroy: () => {
      document.removeEventListener('keydown', onKey);
      window.removeEventListener('resize', onResize);
      stage.removeEventListener('wheel', onWheel);
      root.remove();
    },
  };
}
