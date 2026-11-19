// Talks to the Rust backend inside Tauri; falls back to a demo engine in a
// normal browser so the UI can be previewed without building the app.
import { mock } from './mock.js';

const T = window.__TAURI__;
export const isTauri = !!(T && T.core);

export function invoke(cmd, args = {}) {
  return isTauri ? T.core.invoke(cmd, args) : mock.invoke(cmd, args);
}

export function listen(event, cb) {
  return isTauri ? T.event.listen(event, (e) => cb(e.payload)) : mock.listen(event, cb);
}
