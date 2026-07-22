<script lang="ts">
  import { onMount } from 'svelte';
  import { get } from 'svelte/store';
  import { getCurrentWebview } from '@tauri-apps/api/webview';
  import { listen } from '@tauri-apps/api/event';
  import {
    nowPlaying,
    currentIndex,
    playing,
    errorMsg,
    playlist,
    levels,
    inputDevices,
    outputDevices,
  } from './lib/stores';
  import {
    next,
    listPlaylist,
    addToPlaylist,
    isAudio,
    listInputDevices,
    listOutputDevices,
  } from './lib/api';
  import type { LevelMeter, Track } from './lib/api';
  import Playlist from './lib/components/Playlist.svelte';
  import Transport from './lib/components/Transport.svelte';
  import NowPlaying from './lib/components/NowPlaying.svelte';
  import SourceToggle from './lib/components/SourceToggle.svelte';
  import Meters from './lib/components/Meters.svelte';
  import DeviceChoosers from './lib/components/DeviceChoosers.svelte';

  onMount(() => {
    // Restore playlist state (survives a webview reload / right-click refresh).
    listPlaylist().then((snap) => {
      playlist.set(snap.tracks);
      if (snap.current != null && snap.tracks[snap.current]) {
        currentIndex.set(snap.current);
        nowPlaying.set(snap.tracks[snap.current]);
        playing.set(true);
      }
    });
    listInputDevices().then(inputDevices.set);
    listOutputDevices().then(outputDevices.set);

    const np = listen<Track>('now_playing', (e) => {
      nowPlaying.set(e.payload);
      playing.set(true);
      const idx = get(playlist).findIndex((t) => t.path === e.payload.path);
      currentIndex.set(idx >= 0 ? idx : null);
    });
    const ended = listen('track_ended', () => {
      playing.set(false);
      next();
    });
    const lvl = listen<LevelMeter>('levels', (e) => levels.set(e.payload));
    const err = listen<string>('error', (e) => errorMsg.set(e.payload));

    const dd = getCurrentWebview().onDragDropEvent((e) => {
      if (e.payload.type === 'drop') {
        const paths = e.payload.paths.filter(isAudio);
        if (paths.length) addToPlaylist(paths).then((tracks) => playlist.set(tracks));
      }
    });

    return () => {
      np.then((u) => u());
      ended.then((u) => u());
      lvl.then((u) => u());
      err.then((u) => u());
      dd.then((u) => u());
    };
  });
</script>

<main class="flex h-screen flex-col bg-zinc-950 text-zinc-200">
  {#if $errorMsg}
    <div
      class="flex items-start gap-2 border-b border-red-900 bg-red-950/60 px-4 py-2 text-sm text-red-200"
    >
      <span class="flex-1">{$errorMsg}</span>
      <button class="text-red-300 hover:text-red-100" onclick={() => errorMsg.set(null)}
        >✕</button
      >
    </div>
  {/if}

  <div class="flex items-center gap-4 border-b border-zinc-800 px-4 py-2">
    <SourceToggle />
    <div class="min-w-[8rem] max-w-[14rem] flex-1">
      <Meters />
    </div>
    <div class="ml-auto min-w-0 max-w-[45%]">
      <DeviceChoosers />
    </div>
  </div>

  <NowPlaying />
  <Playlist />
  <Transport />
</main>
