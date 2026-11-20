// Interactive avatar cropper.
//
// Picking a file used to centre-crop immediately, so the user could not see the
// result before it was written to disk. This module shows the picked image in a
// circular stage, lets the user pan and zoom, and only returns bytes when the
// user confirms. The visible circle is rendered as a square PNG, which is what
// the Rust side validates (square, 64-1024 px, at most 512 KiB).

/// Nominal stage edge in CSS px; the live size is read after mount so the
/// cropper still fits small windows.
const STAGE = 288;
const MIN_ZOOM = 1;
const MAX_ZOOM = 4;
const MAX_OUTPUT_BYTES = 500 * 1024;
const OUTPUT_EDGES = [512, 384, 256] as const;
/// Smaller ladder for group avatars, which travel over the LAN under 96 KiB.
const TIGHT_EDGES = [256, 192, 128] as const;

export type CropOptions = {
  title?: string;
  /** Extra line under the title, e.g. changing copy for the group avatar. */
  hint?: string;
  /** Byte cap for the PNG payload (group avatars allow less than the profile). */
  maxBytes?: number;
};

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}

function copyBuffer(bytes: Uint8Array): ArrayBuffer {
  const buffer = new ArrayBuffer(bytes.byteLength);
  new Uint8Array(buffer).set(bytes);
  return buffer;
}

/// The picker only offers PNG/JPEG, so the signature is enough for the blob type.
function imageType(bytes: Uint8Array): string {
  const isPng = bytes.length > 8 && bytes[0] === 0x89 && bytes[1] === 0x50 && bytes[2] === 0x4e && bytes[3] === 0x47;
  return isPng ? 'image/png' : 'image/jpeg';
}

function loadImage(url: string): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const image = new Image();
    image.onload = () => resolve(image);
    image.onerror = () => reject(new Error('无法读取所选图片'));
    image.src = url;
  });
}

async function paintSquare(
  image: HTMLImageElement, sx: number, sy: number, side: number, edge: number,
): Promise<Uint8Array> {
  const canvas = document.createElement('canvas');
  canvas.width = edge;
  canvas.height = edge;
  const context = canvas.getContext('2d');
  if (!context) throw new Error('无法创建画布');
  context.imageSmoothingEnabled = true;
  context.imageSmoothingQuality = 'high';
  context.drawImage(image, sx, sy, side, side, 0, 0, edge, edge);
  const blob = await new Promise<Blob | null>((resolve) => canvas.toBlob(resolve, 'image/png'));
  if (!blob) throw new Error('无法生成头像');
  return new Uint8Array(await blob.arrayBuffer());
}

/// Renders the current viewport, stepping down in size until it fits the cap.
async function renderOutput(
  image: HTMLImageElement, stage: number, baseScale: number, zoom: number,
  offsetX: number, offsetY: number, maxBytes: number,
): Promise<Uint8Array> {
  const scale = baseScale * zoom;
  const side = clamp(stage / scale, 1, Math.min(image.naturalWidth, image.naturalHeight));
  const sx = clamp(image.naturalWidth / 2 - offsetX / scale - side / 2, 0, image.naturalWidth - side);
  const sy = clamp(image.naturalHeight / 2 - offsetY / scale - side / 2, 0, image.naturalHeight - side);
  const edges = maxBytes < MAX_OUTPUT_BYTES ? TIGHT_EDGES : OUTPUT_EDGES;
  let last: Uint8Array | null = null;
  for (const edge of edges) {
    last = await paintSquare(image, sx, sy, side, edge);
    if (last.length <= maxBytes) return last;
  }
  if (!last) throw new Error('无法生成头像');
  return last;
}

export async function cropAvatarInteractively(
  source: Uint8Array, options: CropOptions = {},
): Promise<Uint8Array | null> {
  const url = URL.createObjectURL(new Blob([copyBuffer(source)], { type: imageType(source) }));
  let image: HTMLImageElement;
  try {
    image = await loadImage(url);
  } catch (error) {
    URL.revokeObjectURL(url);
    throw error;
  }
  if (!image.naturalWidth || !image.naturalHeight) {
    URL.revokeObjectURL(url);
    throw new Error('无法读取所选图片');
  }

  return new Promise<Uint8Array | null>((resolve) => {
    const backdrop = document.createElement('div');
    backdrop.className = 'crop-backdrop';
    backdrop.innerHTML = `
      <div class="crop-dialog" role="dialog" aria-modal="true" aria-labelledby="crop-dialog-title">
        <span class="eyebrow">AVATAR / 头像</span>
        <h2 id="crop-dialog-title">${options.title ?? '调整头像'}</h2>
        <p class="crop-hint">${options.hint ?? '拖动图片调整位置，滚轮或下方滑块缩放；圆形区域会保存为正方形图片。'}</p>
        <div class="crop-stage" id="crop-stage">
          <img class="crop-image" id="crop-image" alt="待裁剪的头像" draggable="false">
        </div>
        <div class="crop-zoom-row">
          <span>缩小</span>
          <input type="range" id="crop-zoom" min="${MIN_ZOOM}" max="${MAX_ZOOM}" step="0.01" value="1" aria-label="缩放">
          <span>放大</span>
        </div>
        <div class="crop-actions">
          <button type="button" class="linkish" data-crop="cancel">取消</button>
          <button type="button" class="outline" data-crop="reset">重置</button>
          <button type="button" class="ob-primary" data-crop="apply" id="crop-apply">使用此头像</button>
        </div>
      </div>`;
    document.body.append(backdrop);

    const stage = backdrop.querySelector<HTMLElement>('#crop-stage');
    const view = backdrop.querySelector<HTMLImageElement>('#crop-image');
    const slider = backdrop.querySelector<HTMLInputElement>('#crop-zoom');
    if (!stage || !view || !slider) { backdrop.remove(); URL.revokeObjectURL(url); resolve(null); return; }
    view.src = url;

    const stageSize = stage.clientWidth || STAGE;
    const baseScale = Math.max(stageSize / image.naturalWidth, stageSize / image.naturalHeight);
    let zoom = 1;
    let offsetX = 0;
    let offsetY = 0;
    let settled = false;

    const apply = (): void => {
      const width = image.naturalWidth * baseScale * zoom;
      const height = image.naturalHeight * baseScale * zoom;
      const maxX = Math.max(0, (width - stageSize) / 2);
      const maxY = Math.max(0, (height - stageSize) / 2);
      offsetX = clamp(offsetX, -maxX, maxX);
      offsetY = clamp(offsetY, -maxY, maxY);
      view.style.width = `${width}px`;
      view.style.height = `${height}px`;
      view.style.transform = `translate(calc(-50% + ${offsetX}px), calc(-50% + ${offsetY}px))`;
    };
    apply();

    const setZoom = (value: number): void => {
      zoom = clamp(Number.isFinite(value) ? value : 1, MIN_ZOOM, MAX_ZOOM);
      slider.value = String(zoom);
      apply();
    };

    const finish = (result: Uint8Array | null): void => {
      if (settled) return;
      settled = true;
      document.removeEventListener('keydown', onKey, true);
      backdrop.remove();
      URL.revokeObjectURL(url);
      resolve(result);
    };

    const onKey = (event: KeyboardEvent): void => {
      if (event.key === 'Escape') { event.preventDefault(); finish(null); }
    };

    let dragging = false;
    let startX = 0;
    let startY = 0;
    let originX = 0;
    let originY = 0;

    stage.addEventListener('pointerdown', (event) => {
      if (event.button !== 0) return;
      dragging = true;
      startX = event.clientX;
      startY = event.clientY;
      originX = offsetX;
      originY = offsetY;
      stage.setPointerCapture(event.pointerId);
      stage.classList.add('is-dragging');
    });
    stage.addEventListener('pointermove', (event) => {
      if (!dragging) return;
      offsetX = originX + (event.clientX - startX);
      offsetY = originY + (event.clientY - startY);
      apply();
    });
    const endDrag = (event: PointerEvent): void => {
      if (!dragging) return;
      dragging = false;
      stage.classList.remove('is-dragging');
      if (stage.hasPointerCapture(event.pointerId)) stage.releasePointerCapture(event.pointerId);
    };
    stage.addEventListener('pointerup', endDrag);
    stage.addEventListener('pointercancel', endDrag);

    stage.addEventListener('wheel', (event) => {
      event.preventDefault();
      setZoom(zoom - event.deltaY * 0.0015);
    }, { passive: false });

    slider.addEventListener('input', () => setZoom(Number(slider.value)));

    backdrop.addEventListener('click', (event) => {
      const target = event.target;
      if (target === backdrop) { finish(null); return; }
      const action = (target as HTMLElement).closest<HTMLElement>('[data-crop]')?.dataset.crop;
      if (action === 'cancel') { finish(null); return; }
      if (action === 'reset') { zoom = 1; offsetX = 0; offsetY = 0; slider.value = '1'; apply(); return; }
      if (action === 'apply') {
        const button = backdrop.querySelector<HTMLButtonElement>('#crop-apply');
        if (button) button.disabled = true;
        void renderOutput(image, stageSize, baseScale, zoom, offsetX, offsetY, options.maxBytes ?? MAX_OUTPUT_BYTES)
          .then((bytes) => finish(bytes))
          .catch(() => {
            if (button) button.disabled = false;
            const hint = backdrop.querySelector<HTMLElement>('.crop-hint');
            if (hint) hint.textContent = '无法生成头像，请换一张图片重试。';
          });
      }
    });

    document.addEventListener('keydown', onKey, true);
    backdrop.querySelector<HTMLButtonElement>('#crop-apply')?.focus();
  });
}
