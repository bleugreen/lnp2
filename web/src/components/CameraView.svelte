<script>
  import { onMount, onDestroy } from 'svelte';
  import { createCameraSocket } from '../lib/websocket.js';
  import { position, jogFeedrate } from '../lib/stores.js';
  import { moveTo, getCameraList } from '../lib/api.js';

  let canvas;
  let ctx;
  let socket = null;
  let selectedCamera = $state('top');
  let cameras = $state([]);
  let cameraConfigs = $state({});
  let width = $state(1280);
  let height = $state(720);

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
    connectCamera();
  }
</script>

<div class="camera-container">
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

  .camera-selector {
    display: flex;
    gap: 0.5rem;
    padding: 0.5rem;
    background: #16213e;
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

  .camera-canvas {
    display: block;
    width: 100%;
    height: auto;
    cursor: crosshair;
  }
</style>
