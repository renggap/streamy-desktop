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

export const profiles = writable<ConnectionProfile[]>([]);
export const bitrate = writable(128);
export const codec = writable<'mp3' | 'aac'>('mp3');
export const streamStatus = writable<StreamStatus>({ state: 'disconnected' });

export interface StreamStatus {
  state: 'connecting' | 'connected' | 'disconnected' | 'error';
  message?: string;
}

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

export type View = 'home' | 'stream' | 'settings';
export const view = writable<View>('home');

export interface Position {
  pct: number;
  elapsed_secs: number;
  total_secs: number;
}
export const position = writable<Position>({ pct: 0, elapsed_secs: 0, total_secs: 0 });
