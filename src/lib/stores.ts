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
export const sourceMode = writable<SourceMode>('file');

export const profiles = writable<ConnectionProfile[]>([]);
export const bitrate = writable(128);
export const streamStatus = writable<StreamStatus>({ state: 'disconnected' });

export interface StreamStatus {
  state: 'connecting' | 'connected' | 'disconnected' | 'error';
  message?: string;
}
