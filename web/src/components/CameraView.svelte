<script>
  import { onMount, onDestroy } from 'svelte';
  import { createCameraSocket } from '../lib/websocket.js';
  import { position, jogFeedrate } from '../lib/stores.js';
  import { moveTo, getCameraList, detectAll, datasetCapture, datasetCount } from '../lib/api.js';

  let canvas;
  let ctx;
  let socket = null;
  let selectedCamera = $state('top');
  let cameras = $state([]);
  let cameraConfigs = $state({});
  let width = $state(1280);
  let height = $state(720);

  // ML overlay state
  let detections = $state([]);
  let mlEnabled = $state(false);
  let mlInterval = null;
  let mlBusy = false;

  // Dataset capture state
  let captureCount = $state(0);
  let captureFlash = $state(false);

  // Class colors for overlay
  const CLASS_COLORS = [
    '#e94560', // red-pink
    '#00d4aa', // teal
    '#ffa726', // orange
    '#42a5f5', // blue
    '#ab47bc', // purple
    '#66bb6a', // green
    '#ef5350', // red
    '#26c6da', // cyan
  ];

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

    datasetCount().then(r => captureCount = r.count).catch(() => {});
  });

  onDestroy(() => {
    if (socket) socket.destroy();
    stopMl();
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
        drawDetections();
      });
    });
  }

  function drawCrosshair(w, h) {
    const cx = w / 2;
    const cy = h / 2;

    ctx.strokeStyle = 'rgba(0, 255, 0, 0.6)';
    ctx.lineWidth = 1;

    ctx.beginPath();
    ctx.moveTo(0, cy);
    ctx.lineTo(w, cy);
    ctx.stroke();

    ctx.beginPath();
    ctx.moveTo(cx, 0);
    ctx.lineTo(cx, h);
    ctx.stroke();

    ctx.beginPath();
    ctx.arc(cx, cy, 20, 0, Math.PI * 2);
    ctx.stroke();
  }

  function drawDetections() {
    if (!detections.length) return;

    for (const d of detections) {
      const color = CLASS_COLORS[d.class_id % CLASS_COLORS.length];
      const x = d.x - d.width / 2;
      const y = d.y - d.height / 2;

      // Filled rect
      ctx.fillStyle = color + '22';
      ctx.fillRect(x, y, d.width, d.height);

      // Border
      ctx.strokeStyle = color;
      ctx.lineWidth = 2;
      ctx.strokeRect(x, y, d.width, d.height);

      // Label background
      const label = `${d.class_name} ${(d.confidence * 100).toFixed(0)}%`;
      ctx.font = 'bold 13px monospace';
      const tw = ctx.measureText(label).width;
      ctx.fillStyle = 'rgba(0, 0, 0, 0.7)';
      ctx.fillRect(x, y - 18, tw + 8, 18);

      // Label text
      ctx.fillStyle = color;
      ctx.fillText(label, x + 4, y - 4);
    }
  }

  function toggleMl() {
    mlEnabled = !mlEnabled;
    if (mlEnabled) {
      startMl();
    } else {
      stopMl();
    }
  }

  function startMl() {
    if (mlInterval) return;
    runMl(); // Run immediately
    mlInterval = setInterval(runMl, 500); // Then every 500ms
  }

  function stopMl() {
    if (mlInterval) {
      clearInterval(mlInterval);
      mlInterval = null;
    }
    detections = [];
  }

  async function runMl() {
    if (mlBusy) return;
    mlBusy = true;
    try {
      const result = await detectAll(selectedCamera);
      detections = result.detections || [];
    } catch {
      // Model not loaded or other error — silently ignore
    } finally {
      mlBusy = false;
    }
  }

  async function capture() {
    try {
      const result = await datasetCapture(selectedCamera);
      captureCount = result.count;
      captureFlash = true;
      setTimeout(() => captureFlash = false, 300);
    } catch (e) {
      console.error('Capture failed:', e);
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
    const targetY = pos.y - offsetY;

    moveTo(targetX, targetY, undefined, $jogFeedrate).catch(console.error);
  }

  function switchCamera(name) {
    selectedCamera = name;
    detections = [];
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
      <button
        class:active={mlEnabled}
        class="ml-toggle"
        onclick={toggleMl}
      >
        {mlEnabled ? 'ML On' : 'ML Off'}
      </button>
      <span class="separator"></span>
      <button class="capture" onclick={capture}>
        Capture
      </button>
      {#if captureCount > 0}
        <span class="capture-count">{captureCount}</span>
      {/if}
    </div>
  </div>
  <canvas
    bind:this={canvas}
    width={width}
    height={height}
    onclick={handleClick}
    class="camera-canvas"
    class:flash={captureFlash}
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
    align-items: center;
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

  .vision-controls button.ml-toggle {
    border-color: #555;
  }

  .vision-controls button.ml-toggle.active {
    background: #1a6b3a;
    border-color: #2ecc71;
    color: #2ecc71;
  }

  .vision-controls .separator {
    width: 1px;
    height: 20px;
    background: #444;
  }

  .vision-controls button.capture {
    background: #1a6b3a;
    border-color: #2ecc71;
  }

  .vision-controls button.capture:hover {
    background: #27ae60;
  }

  .vision-controls .capture-count {
    font-size: 0.75rem;
    color: #888;
    min-width: 1.5rem;
    text-align: center;
  }

  .camera-canvas.flash {
    opacity: 0.5;
  }

  .camera-canvas {
    display: block;
    width: 100%;
    height: auto;
    cursor: crosshair;
  }
</style>
