<script>
  import CameraView from './components/CameraView.svelte';
  import JogControls from './components/JogControls.svelte';
  import PositionDisplay from './components/PositionDisplay.svelte';
  import ActuatorPanel from './components/ActuatorPanel.svelte';
  import { createEventSocket } from './lib/websocket.js';
  import { position, connected } from './lib/stores.js';

  const eventSocket = createEventSocket();
</script>

<svelte:head>
  <style>
    :global(body) {
      margin: 0;
      font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif;
      background: #1a1a2e;
      color: #eee;
    }
  </style>
</svelte:head>

<div class="app">
  <header>
    <h1>lnp2</h1>
    <span class="status" class:online={$connected}>
      {$connected ? 'Connected' : 'Disconnected'}
    </span>
  </header>

  <main>
    <div class="camera-section">
      <CameraView />
    </div>
    <div class="controls-section">
      <PositionDisplay />
      <JogControls />
      <ActuatorPanel />
    </div>
  </main>
</div>

<style>
  .app {
    max-width: 1400px;
    margin: 0 auto;
    padding: 1rem;
  }

  header {
    display: flex;
    align-items: center;
    gap: 1rem;
    margin-bottom: 1rem;
  }

  header h1 {
    margin: 0;
    font-size: 1.5rem;
    font-weight: 600;
  }

  .status {
    font-size: 0.85rem;
    padding: 0.25rem 0.75rem;
    border-radius: 1rem;
    background: #c0392b;
  }

  .status.online {
    background: #27ae60;
  }

  main {
    display: grid;
    grid-template-columns: 1fr 320px;
    gap: 1rem;
    align-items: start;
  }

  .controls-section {
    display: flex;
    flex-direction: column;
    gap: 1rem;
  }

  @media (max-width: 900px) {
    main {
      grid-template-columns: 1fr;
    }
  }
</style>
