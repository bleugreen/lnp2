<script>
  import { onMount, onDestroy } from 'svelte';
  import { createCameraSocket } from '../lib/websocket.js';
  import { position, jogFeedrate } from '../lib/stores.js';
  import { moveTo, getCameraList, detectPocket, detectFiducial } from '../lib/api.js';

  let canvas;
  let ctx;
  let socket = null;
  let selectedCamera = $state('top');
  let cameras = $state([]);
  let cameraConfigs = $state({});
  let width = $state(1280);
  let height = $state(720);

  // Vision overlay state
  let detection = $state(null);
  let detecting = $state(false);

  onMount(async () => {
    ctx = canvas.getContext('2d');

    try {
      const list = await getCameraList();
      cameras = list.cameras || [];
      cameraConfigs = list.configs || {};
      if (cameras.length > 0 && !cameras.includes(selectedCamera)) {
        selectedCamera = cameras[0];
      }
    } catch {}

    connectCamera();
  });

  onDestroy(() => {
    if (socket) socket.destroy();
  });

  function connectCamera() {
    if (socket) socket.destroy();

    const config = cameraConfigs[selectedCamera];
    if (config) {
      width = config.width || 1280;
      height = config.height || 720;
    }

    socket = createCameraSocket(selectedCamera, (frameData) => {
      const blob = new Blob([frameData], { type: 'image/jpeg' });
      createImageBitmap(blob).then((bitmap) => {
        if (!ctx) return;
        canvas.width = bitmap.width;
        canvas.height = bitmap.height;
        ctx.drawImage(bitmap, 0, 0);
        drawCrosshair(bitmap.width, bitmap.height);
        drawDetection();
      });
    });
  }

  function drawCrosshair(w, h) {
    const cx = w / 2;
    const cy = h / 2;

    ctx.strokeStyle = 'rgba(0, 255, 0, 0.6)';
    ctx.lineWidth = 1;

    // Horizontal line
    ctx.beginPath();
    ctx.moveTo(0, cy);
    ctx.lineTo(w, cy);
    ctx.stroke();

    // Vertical line
    ctx.beginPath();
    ctx.moveTo(cx, 0);
    ctx.lineTo(cx, h);
    ctx.stroke();

    // Center circle
    ctx.beginPath();
    ctx.arc(cx, cy, 20, 0, Math.PI * 2);
    ctx.stroke();
  }

  function drawDetection() {
    if (!detection) return;

    const d = detection;
    const r = d.region_px;

    // Draw bounding region if we have pixel coords
    if (r) {
      ctx.save();
      ctx.translate(r.x, r.y);
      if (r.rotation_deg) ctx.rotate(r.rotation_deg * Math.PI / 180);

      // Filled rect with transparency
      ctx.fillStyle = 'rgba(230, 70, 70, 0.15)';
      ctx.fillRect(-r.width / 2, -r.height / 2, r.width, r.height);

      // Border
      ctx.strokeStyle = '#e94560';
      ctx.lineWidth = 2;
      ctx.strokeRect(-r.width / 2, -r.height / 2, r.width, r.height);

      ctx.restore();

      // Crosshair at detection center
      ctx.strokeStyle = '#e94560';
      ctx.lineWidth = 1;
      ctx.beginPath();
      ctx.moveTo(r.x - 12, r.y);
      ctx.lineTo(r.x + 12, r.y);
      ctx.moveTo(r.x, r.y - 12);
      ctx.lineTo(r.x, r.y + 12);
      ctx.stroke();
    }

    // Info label
    const method = typeof d.method === 'string' ? d.method : Object.keys(d.method)[0];
    const conf = (d.confidence * 100).toFixed(1);
    const ox = d.offset_x_mm.toFixed(3);
    const oy = d.offset_y_mm.toFixed(3);

    const labelX = r ? r.x + r.width / 2 + 8 : 10;
    const labelY = r ? r.y - r.height / 2 : 20;

    ctx.font = '13px monospace';
    // Shadow for readability
    ctx.fillStyle = 'rgba(0, 0, 0, 0.7)';
    ctx.fillRect(labelX - 4, labelY - 14, 200, 38);
    ctx.fillStyle = '#e94560';
    ctx.fillText(`${method}  ${conf}%`, labelX, labelY);
    ctx.fillStyle = '#ccc';
    ctx.fillText(`Δ ${ox}, ${oy} mm`, labelX, labelY + 16);
  }

  async function runDetect(type) {
    detecting = true;
    detection = null;
    try {
      let result;
      if (type === 'pocket') {
        result = await detectPocket(selectedCamera, 8.0, 4.0);
        detection = result.detection;
      } else if (type === 'fiducial') {
        result = await detectFiducial(selectedCamera);
        detection = result.detection;
      }
    } catch (e) {
      console.error('Detection failed:', e);
    } finally {
      detecting = false;
    }
  }

  function handleClick(e) {
    const config = cameraConfigs[selectedCamera];
    if (!config) return;

    const rect = canvas.getBoundingClientRect();
    const scaleX = canvas.width / rect.width;
    const scaleY = canvas.height / rect.height;

    const pixelX = (e.clientX - rect.left) * scaleX;
    const pixelY = (e.clientY - rect.top) * scaleY;

    const cx = canvas.width / 2;
    const cy = canvas.height / 2;

    const offsetX = (pixelX - cx) * config.upp_x;
    const offsetY = (pixelY - cy) * config.upp_y;

    let pos;
    position.subscribe((p) => (pos = p))();

    const targetX = pos.x + offsetX;
    const targetY = pos.y - offsetY; // Y is inverted (screen Y down, machine Y up)

    moveTo(targetX, targetY, undefined, $jogFeedrate).catch(console.error);
  }

  function switchCamera(name) {
    selectedCamera = name;
    detection = null;
    connectCamera();
  }
</script>

<div class="camera-container">
  <div class="camera-toolbar">
    {#if cameras.length > 1}
      <div class="camera-selector">
        {#each cameras as cam}
          <button
            class:active={cam === selectedCamera}
            onclick={() => switchCamera(cam)}
          >
            {cam}
          </button>
        {/each}
      </div>
    {/if}
    <div class="vision-controls">
      <button onclick={() => runDetect('pocket')} disabled={detecting}>
        {detecting ? '...' : 'Detect Pocket'}
      </button>
      <button onclick={() => runDetect('fiducial')} disabled={detecting}>
        {detecting ? '...' : 'Detect Fid'}
      </button>
      {#if detection}
        <button class="clear" onclick={() => detection = null}>Clear</button>
      {/if}
    </div>
  </div>
  <canvas
    bind:this={canvas}
    width={width}
    height={height}
    onclick={handleClick}
    class="camera-canvas"
  ></canvas>
</div>

<style>
  .camera-container {
    background: #000;
    border-radius: 8px;
    overflow: hidden;
  }

  .camera-toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0.5rem;
    background: #16213e;
  }

  .camera-selector {
    display: flex;
    gap: 0.5rem;
  }

  .camera-selector button {
    padding: 0.25rem 0.75rem;
    border: 1px solid #444;
    border-radius: 4px;
    background: #1a1a2e;
    color: #eee;
    cursor: pointer;
    text-transform: capitalize;
  }

  .camera-selector button.active {
    background: #0f3460;
    border-color: #e94560;
  }

  .vision-controls {
    display: flex;
    gap: 0.5rem;
  }

  .vision-controls button {
    padding: 0.25rem 0.75rem;
    border: 1px solid #555;
    border-radius: 4px;
    background: #2a2a4e;
    color: #eee;
    cursor: pointer;
    font-size: 0.8rem;
  }

  .vision-controls button:hover:not(:disabled) {
    background: #3a3a5e;
  }

  .vision-controls button:disabled {
    opacity: 0.5;
    cursor: wait;
  }

  .vision-controls button.clear {
    background: transparent;
    border-color: #e94560;
    color: #e94560;
  }

  .camera-canvas {
    display: block;
    width: 100%;
    height: auto;
    cursor: crosshair;
  }
</style>
