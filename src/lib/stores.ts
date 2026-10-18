import { writable } from 'svelte/store';
import type { ConnectionProfile, DeviceInfo, LevelMeter, SourceMode, Track } from './api';

export const playlist = writable<Track[]>([]);
export const nowPlaying = writable<Track | null>(null);
export const currentIndex = writable<number | null>(null);
export const playing = writable(false);
export const errorMsg = writable<string | null>(null);

export const levels = writable<LevelMeter>({ monitor: [0, 0], mic: [0, 0] });
export const inputDevices = writable<DeviceInfo[]>([]);
export const outputDevices = writable<DeviceInfo[]>([]);
export const selectedInput = writable<string>('');
export const selectedOutput = writable<string>('');
export const recDir = writable<string | null>(null);
/** Master output volume as a percent (0..=200, default 100 = unity). */
export const volume = writable<number>(100);
/** Monitor-only volume for local file playback, percent (0..=200, default 100). */
export const monitorFileVolume = writable<number>(100);
export const streamTitle = writable<string>('');
export const sourceMode = writable<SourceMode>('file');

// Profile editor screen: which profile is being edited ('' = new profile),
// null = editor closed. Lives in the store so the settings list (Edit/Add
// buttons) and the editor screen share one source of truth.
export const profileEditing = writable<string | null>(null);

// WebView2 runtime repair (in-app interstitial).
export type Wv2State = 'idle' | 'downloading' | 'launching' | 'done' | 'err';
export const wv2State = writable<Wv2State>('idle');
export const wv2Progress = writable<{ downloaded: number; total: number | null }>({
  downloaded: 0,
  total: null,
});

// P5 mixer state.
export const crossfade = writable(0); // 0=A .. 1=B
export const eqLow = writable(0);
export const eqMid = writable(0);
export const eqHigh = writable(0);
export const duckEnabled = writable(false);
export const duckThreshold = writable(0.15);
export const duckDepth = writable(12);
/** Mic noise suppression (RNNoise) for the broadcast voice path. */
export const micDenoise = writable(false);

export const profiles = writable<ConnectionProfile[]>([]);
export const bitrate = writable(24); // AAC talk-radio sweet spot (user default)
export const codec = writable<'mp3' | 'aac'>('aac');
export const streamStatus = writable<StreamStatus>({ state: 'disconnected' });

export interface StreamStatus {
  state: 'connecting' | 'connected' | 'disconnected' | 'error';
  message?: string;
}

export interface StreamStats {
  kbps: number;
  total_bytes: number;
}
export const streamStats = writable<StreamStats>({ kbps: 0, total_bytes: 0 });

export type RecFormat = 'mp3' | 'wav';
export const recFormat = writable<RecFormat>('mp3');
export const recording = writable(false);
export const recordingPath = writable<string | null>(null);
/** Epoch-ms when the current take started (for the elapsed indicator). */
export const recStartedAt = writable<number | null>(null);

export interface RecordingStatus {
  state: 'recording' | 'stopped' | 'error';
  path?: string;
  message?: string;
}

export type View = 'home' | 'stream' | 'settings' | 'profile-editor';
export const view = writable<View>('home');

export interface Position {
  pct: number;
  elapsed_secs: number;
  total_secs: number;
}
export const position = writable<Position>({ pct: 0, elapsed_secs: 0, total_secs: 0 });

// Toast notifications (Joko UI toast style) — press feedback for actions.
export interface Toast {
  id: number;
  kind: 'success' | 'error' | 'info';
  title: string;
  detail?: string;
  /** When set, the toast renders an action button (update Download). */
  action?: { label: string; run: () => void };
}
export const toasts = writable<Toast[]>([]);

let toastSeq = 0;
export function notify(kind: Toast['kind'], title: string, detail?: string) {
  const id = ++toastSeq;
  toasts.update((t) => [...t.slice(-2), { id, kind, title, detail }]); // max 3
  setTimeout(() => toasts.update((t) => t.filter((x) => x.id !== id)), 3200);
}
/** Sticky toast: stays until the user dismisses it (update notices). */
export function notifySticky(
  kind: Toast['kind'],
  title: string,
  detail?: string,
  action?: Toast['action'],
): number {
  const id = ++toastSeq;
  toasts.update((t) => [...t.slice(-2), { id, kind, title, detail, action }]);
  return id;
}
export function dismissToast(id: number) {
  toasts.update((t) => t.filter((x) => x.id !== id));
}
