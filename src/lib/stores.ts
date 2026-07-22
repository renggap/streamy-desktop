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
export const sourceMode = writable<SourceMode>('file');

export const profiles = writable<ConnectionProfile[]>([]);
export const bitrate = writable(128);
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
