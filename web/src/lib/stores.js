import { writable } from 'svelte/store';

export const position = writable({ x: 0, y: 0, z: 0, a: 0, b: 0 });
export const connected = writable(false);
export const jogFeedrate = writable(6000);
