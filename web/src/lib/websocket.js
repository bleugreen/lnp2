import { position, connected } from './stores.js';

function wsUrl(path) {
  const proto = location.protocol === 'https:' ? 'wss:' : 'ws:';
  return `${proto}//${location.host}${path}`;
}

function createReconnectingSocket(url, { onMessage, onOpen, onClose }) {
  let ws = null;
  let reconnectDelay = 500;
  let destroyed = false;

  function connect() {
    if (destroyed) return;
    ws = new WebSocket(url);
    ws.binaryType = 'arraybuffer';

    ws.onopen = () => {
      reconnectDelay = 500;
      if (onOpen) onOpen();
    };

    ws.onmessage = (e) => {
      if (onMessage) onMessage(e);
    };

    ws.onclose = () => {
      if (onClose) onClose();
      if (!destroyed) {
        setTimeout(connect, reconnectDelay);
        reconnectDelay = Math.min(reconnectDelay * 2, 5000);
      }
    };

    ws.onerror = () => {
      ws.close();
    };
  }

  connect();

  return {
    destroy() {
      destroyed = true;
      if (ws) ws.close();
    },
  };
}

export function createEventSocket() {
  return createReconnectingSocket(wsUrl('/api/events'), {
    onOpen() {
      connected.set(true);
      // Fetch current position immediately on connect
      fetch('/api/position').then(r => r.json()).then(pos => {
        position.set(pos);
      }).catch(() => {});
    },
    onClose() {
      connected.set(false);
    },
    onMessage(e) {
      try {
        const event = JSON.parse(e.data);
        if (event.type === 'Position') {
          position.set({
            x: event.x,
            y: event.y,
            z: event.z,
            a: event.a,
            b: event.b,
          });
        }
      } catch {}
    },
  });
}

export function createCameraSocket(name, onFrame) {
  return createReconnectingSocket(wsUrl(`/api/camera/stream?name=${name}`), {
    onMessage(e) {
      if (e.data instanceof ArrayBuffer) {
        onFrame(e.data);
      }
    },
  });
}
