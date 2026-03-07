const BASE = '';

async function request(method, path, body) {
  const opts = {
    method,
    headers: { 'Content-Type': 'application/json' },
  };
  if (body !== undefined) {
    opts.body = JSON.stringify(body);
  }
  const res = await fetch(`${BASE}${path}`, opts);
  if (!res.ok) {
    const err = await res.json().catch(() => ({ error: res.statusText }));
    throw new Error(err.error || res.statusText);
  }
  return res.json();
}

export async function moveTo(x, y, z, feedrate) {
  const body = {};
  if (x !== undefined) body.x = x;
  if (y !== undefined) body.y = y;
  if (z !== undefined) body.z = z;
  if (feedrate !== undefined) body.feedrate = feedrate;
  return request('POST', '/api/move', body);
}

export async function moveSafe(x, y) {
  return request('POST', '/api/move/safe', { x, y });
}

export async function home() {
  return request('POST', '/api/home');
}

export async function getPosition() {
  return request('GET', '/api/position');
}

export async function setVacuum(nozzle, on) {
  return request('POST', '/api/vacuum', { nozzle, action: on ? 'on' : 'off' });
}

export async function blow(nozzle, duration_ms = 100) {
  return request('POST', '/api/blow', { nozzle, duration_ms });
}

export async function setLed(r, g, b, brightness = 255) {
  return request('POST', '/api/led', { r, g, b, brightness });
}

export async function ledOff() {
  return request('POST', '/api/led', { off: true });
}

export async function getCameraList() {
  return request('GET', '/api/camera/list');
}

export async function sendGcode(command) {
  return request('POST', '/api/gcode', { command });
}
