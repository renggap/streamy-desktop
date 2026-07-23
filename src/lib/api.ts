import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';

export interface Track {
  path: string;
  title: string;
  artist: string;
  duration_secs: number;
}

export interface PlaylistSnapshot {
  tracks: Track[];
  current: number | null;
}

export type SourceMode = 'file' | 'mic' | 'both';

export interface DeviceInfo {
  id: string;
  name: string;
  channels: number;
  default_sample_rate: number;
}

export interface LevelMeter {
  monitor: [number, number];
  mic: [number, number];
}

export const AUDIO_EXTS = ['wav', 'mp3', 'flac', 'ogg', 'oga', 'm4a', 'aac'];

export const isAudio = (path: string) =>
  AUDIO_EXTS.includes(path.split('.').pop()?.toLowerCase() ?? '');

export const ping = () => invoke<string>('ping');

export const addToPlaylist = (paths: string[]) =>
  invoke<Track[]>('add_to_playlist', { paths });

export const listPlaylist = () => invoke<PlaylistSnapshot>('list_playlist');

export const listInputDevices = () => invoke<DeviceInfo[]>('list_input_devices');
export const listOutputDevices = () => invoke<DeviceInfo[]>('list_output_devices');
export const setInputDevice = (name: string) =>
  invoke<void>('set_input_device', { name });
export const setOutputDevice = (name: string | null) =>
  invoke<void>('set_output_device', { name });
export const setRecordingDir = (dir: string | null) =>
  invoke<void>('set_recording_dir', { dir });
export const getSettings = () =>
  invoke<{
    input_device: string | null;
    output_device: string | null;
    recording_dir: string | null;
  }>('get_settings');
export const setSourceMode = (mode: SourceMode) =>
  invoke<void>('set_source_mode', { mode });
export const setCrossfade = (pos: number) =>
  invoke<void>('set_crossfade', { pos });
export const setEq = (low_db: number, mid_db: number, high_db: number) =>
  invoke<void>('set_eq', { lowDb: low_db, midDb: mid_db, highDb: high_db });
export const setDuck = (enabled: boolean, threshold: number, depth_db: number) =>
  invoke<void>('set_duck', { enabled, threshold, depthDb: depth_db });

// --- P2 streaming ---

export interface ConnectionProfile {
  name: string;
  host: string;
  port: number;
  user: string;
  mount: string;
  protocol: 'icy' | 'ultravox';
  stream_id: number;
  station_name: string;
  genre: string;
  url: string;
  public: boolean;
}

export const BITRATES = [16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320];

export const listProfiles = () => invoke<ConnectionProfile[]>('list_profiles');
export const saveProfile = (profile: ConnectionProfile, password: string | null) =>
  invoke<void>('save_profile', { profile, password });
export const deleteProfile = (name: string) =>
  invoke<void>('delete_profile', { name });
export const connectStream = (name: string, bitrate_kbps: number, codec: 'mp3' | 'aac') =>
  invoke<void>('connect_stream', { name, bitrateKbps: bitrate_kbps, codec });
export const disconnectStream = () => invoke<void>('disconnect_stream');
export const setStreamTitle = (title: string) =>
  invoke<void>('set_stream_title', { title });

export const startRecording = (format: 'mp3' | 'wav') =>
  invoke<string>('start_recording', { format });
export const stopRecording = () => invoke<void>('stop_recording');

export const playIndex = (i: number) => invoke<void>('play_index', { i });
export const play = () => invoke<void>('play');
export const next = () => invoke<void>('next');
export const prev = () => invoke<void>('prev');
export const pause = () => invoke<void>('pause');
export const seek = (pct: number) => invoke<void>('seek', { pct });

/** Native multi-select audio file dialog; returns absolute paths. */
export const openAudioFiles = async (): Promise<string[]> => {
  const sel = await open({
    multiple: true,
    filters: [
      { name: 'Audio', extensions: ['wav', 'mp3', 'flac', 'ogg', 'oga', 'm4a', 'aac'] },
    ],
  });
  if (!sel) return [];
  return Array.isArray(sel) ? sel : [sel];
};

/** Native folder picker; returns an absolute path or null if cancelled. */
export const openFolder = async (): Promise<string | null> => {
  const sel = await open({ directory: true, multiple: false });
  return sel ? (Array.isArray(sel) ? sel[0] : sel) : null;
};
