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
    profiles,
    streamStatus,
    position,
    view,
    selectedInput,
    recording,
    recordingPath,
    recStartedAt,
  } from './lib/stores';
  import {
    next,
    listPlaylist,
    addToPlaylist,
    isAudio,
    listInputDevices,
    listOutputDevices,
    listProfiles,
    setStreamTitle,
    getSettings,
  } from './lib/api';
  import type { LevelMeter, Track } from './lib/api';
  import type { Position, RecordingStatus, StreamStatus } from './lib/stores';
  import NowPlaying from './lib/components/NowPlaying.svelte';
  import Playlist from './lib/components/Playlist.svelte';
  import SourceToggle from './lib/components/SourceToggle.svelte';
  import Meters from './lib/components/Meters.svelte';
  import DeviceChoosers from './lib/components/DeviceChoosers.svelte';
  import BroadcastBar from './lib/components/BroadcastBar.svelte';
  import RecordBar from './lib/components/RecordBar.svelte';
  import ProfileManager from './lib/components/ProfileManager.svelte';
  import BottomNav from './lib/components/BottomNav.svelte';

  onMount(() => {
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
    listProfiles().then(profiles.set);
    // Restore the persisted mic choice for display (backend re-applies it too).
    getSettings().then((s) => {
      if (s.input_device) selectedInput.set(s.input_device);
    });

    const np = listen<Track>('now_playing', (e) => {
      nowPlaying.set(e.payload);
      playing.set(true);
      const idx = get(playlist).findIndex((t) => t.path === e.payload.path);
      currentIndex.set(idx >= 0 ? idx : null);
      setStreamTitle(e.payload.title);
    });
    const ended = listen('track_ended', () => {
      playing.set(false);
      next();
    });
    const lvl = listen<LevelMeter>('levels', (e) => levels.set(e.payload));
    const pos = listen<Position>('position', (e) => position.set(e.payload));
    const status = listen<StreamStatus>('stream_status', (e) => streamStatus.set(e.payload));
    const rec = listen<RecordingStatus>('recording_status', (e) => {
      const s = e.payload;
      if (s.state === 'recording') {
        recording.set(true);
        recordingPath.set(s.path ?? null);
        recStartedAt.set(Date.now());
      } else {
        recording.set(false);
        recStartedAt.set(null);
        if (s.state === 'stopped') recordingPath.set(s.path ?? null);
        if (s.state === 'error') errorMsg.set(`Recording error: ${s.message ?? 'unknown'}`);
      }
    });
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
      pos.then((u) => u());
      status.then((u) => u());
      rec.then((u) => u());
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
      <button class="text-red-300 hover:text-red-100" onclick={() => errorMsg.set(null)}>✕</button>
    </div>
  {/if}

  <NowPlaying />

  <section class="min-h-0 flex-1 overflow-y-auto">
    {#if $view === 'home'}
      <Playlist />
    {:else if $view === 'stream'}
      <div class="space-y-4 p-4">
        <BroadcastBar />
        <RecordBar />
        <div class="border-t border-zinc-800 pt-3">
          <h3 class="mb-2 text-xs font-semibold uppercase tracking-wider text-zinc-500">Source</h3>
          <SourceToggle />
        </div>
        <div class="border-t border-zinc-800 pt-3">
          <h3 class="mb-2 text-xs font-semibold uppercase tracking-wider text-zinc-500">Levels</h3>
          <Meters />
        </div>
      </div>
    {:else}
      <div class="space-y-5 p-4">
        <div>
          <h3 class="mb-2 text-xs font-semibold uppercase tracking-wider text-zinc-500">Devices</h3>
          <DeviceChoosers />
        </div>
        <div class="border-t border-zinc-800 pt-3">
          <ProfileManager />
        </div>
      </div>
    {/if}
  </section>

  <BottomNav />
</main>
