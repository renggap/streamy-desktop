import { writable } from 'svelte/store';
import type { Track } from './api';

export const playlist = writable<Track[]>([]);
export const nowPlaying = writable<Track | null>(null);
export const currentIndex = writable<number | null>(null);
export const errorMsg = writable<string | null>(null);
