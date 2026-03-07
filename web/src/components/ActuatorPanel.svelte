<script>
  import { setVacuum, blow, setLed, ledOff } from '../lib/api.js';

  let vacuumN1 = $state(false);
  let vacuumN2 = $state(false);
  let ledOn = $state(false);
  let ledColor = $state('#ffffff');

  async function toggleVacuum(nozzle) {
    try {
      if (nozzle === 'n1') {
        vacuumN1 = !vacuumN1;
        await setVacuum('n1', vacuumN1);
      } else {
        vacuumN2 = !vacuumN2;
        await setVacuum('n2', vacuumN2);
      }
    } catch (e) {
      console.error(e);
      // Revert on error
      if (nozzle === 'n1') vacuumN1 = !vacuumN1;
      else vacuumN2 = !vacuumN2;
    }
  }

  async function doBlow(nozzle) {
    try {
      await blow(nozzle);
    } catch (e) {
      console.error(e);
    }
  }

  async function toggleLed() {
    try {
      ledOn = !ledOn;
      if (ledOn) {
        const r = parseInt(ledColor.slice(1, 3), 16);
        const g = parseInt(ledColor.slice(3, 5), 16);
        const b = parseInt(ledColor.slice(5, 7), 16);
        await setLed(r, g, b);
      } else {
        await ledOff();
      }
    } catch (e) {
      console.error(e);
      ledOn = !ledOn;
    }
  }
</script>

<div class="actuator-panel">
  <h3>Actuators</h3>

  <div class="section-label">Vacuum</div>
  <div class="row">
    <button class="toggle-btn" class:active={vacuumN1} onclick={() => toggleVacuum('n1')}>
      N1 {vacuumN1 ? 'ON' : 'OFF'}
    </button>
    <button class="toggle-btn" class:active={vacuumN2} onclick={() => toggleVacuum('n2')}>
      N2 {vacuumN2 ? 'ON' : 'OFF'}
    </button>
  </div>

  <div class="section-label">Blow</div>
  <div class="row">
    <button class="action-btn" onclick={() => doBlow('n1')}>N1 Blow</button>
    <button class="action-btn" onclick={() => doBlow('n2')}>N2 Blow</button>
  </div>

  <div class="section-label">LED</div>
  <div class="row">
    <button class="toggle-btn" class:active={ledOn} onclick={toggleLed}>
      {ledOn ? 'ON' : 'OFF'}
    </button>
    <input type="color" bind:value={ledColor} class="color-picker" />
  </div>
</div>

<style>
  .actuator-panel {
    background: #16213e;
    border-radius: 8px;
    padding: 1rem;
  }

  h3 {
    margin: 0 0 0.75rem;
    font-size: 0.9rem;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: #888;
  }

  .section-label {
    font-size: 0.8rem;
    color: #666;
    margin: 0.75rem 0 0.25rem;
  }

  .section-label:first-of-type {
    margin-top: 0;
  }

  .row {
    display: flex;
    gap: 0.5rem;
  }

  .toggle-btn, .action-btn {
    flex: 1;
    padding: 0.5rem;
    border: 1px solid #444;
    border-radius: 6px;
    background: #1a1a2e;
    color: #eee;
    cursor: pointer;
    font-size: 0.85rem;
    font-weight: 600;
  }

  .toggle-btn.active {
    background: #27ae60;
    border-color: #27ae60;
  }

  .action-btn:hover {
    background: #0f3460;
  }

  .action-btn:active {
    background: #e94560;
  }

  .color-picker {
    width: 48px;
    height: 36px;
    border: none;
    border-radius: 6px;
    cursor: pointer;
    background: none;
  }
</style>
