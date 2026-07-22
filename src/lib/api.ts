import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';

export interface Track {
  path: string;
  title: string;
  artist: string;
  duration_secs: number;
}

export const ping = () => invoke<string>('ping');

export const addToPlaylist = (paths: string[]) =>
  invoke<Track[]>('add_to_playlist', { paths });

export const playIndex = (i: number) => invoke<void>('play_index', { i });
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
