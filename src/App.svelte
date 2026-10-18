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
    selectedOutput,
    recDir,
    volume,
    monitorFileVolume,
    streamTitle,
    recording,
    recordingPath,
    recStartedAt,
    wv2Progress,
    streamStats,
    micDenoise,
    notifySticky,
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
    checkUpdate,
  } from './lib/api';
  import { openUrl } from '@tauri-apps/plugin-opener';
  import type { LevelMeter, Track } from './lib/api';
  import type { Position, RecordingStatus, StreamStatus, StreamStats } from './lib/stores';
  import NowPlaying from './lib/components/NowPlaying.svelte';
  import Playlist from './lib/components/Playlist.svelte';
  import SourceToggle from './lib/components/SourceToggle.svelte';
  import Meters from './lib/components/Meters.svelte';
  import DeviceChoosers from './lib/components/DeviceChoosers.svelte';
  import BroadcastBar from './lib/components/BroadcastBar.svelte';
  import RecordBar from './lib/components/RecordBar.svelte';
  import Mixer from './lib/components/Mixer.svelte';
  import ProfileManager from './lib/components/ProfileManager.svelte';
  import ProfileEditor from './lib/components/ProfileEditor.svelte';
  import RecordingSettings from './lib/components/RecordingSettings.svelte';
  import WebView2Repair from './lib/components/WebView2Repair.svelte';
  import WelcomeModal from './lib/components/WelcomeModal.svelte';
  import BottomNav from './lib/components/BottomNav.svelte';
  import Toaster from './lib/components/Toaster.svelte';

  onMount(() => {
    // Update notice: check the proxy once per launch; sticky toast with a
    // Download button. Failures are silent by design (check_update returns
    // None on any error).
    checkUpdate().then((info) => {
      if (!info) return;
      notifySticky(
        'info',
        `Update available: v${info.version}`,
        info.notes ?? 'Download the new build from the releases page.',
        {
          label: 'Download',
          run: () => {
            openUrl(info.url).catch((e) => console.error('open release page:', e));
          },
        },
      );
    });
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
    // Restore persisted device + recording-folder + volume choices for display.
    getSettings().then((s) => {
      if (s.input_device) selectedInput.set(s.input_device);
      if (s.output_device) selectedOutput.set(s.output_device);
      if (s.recording_dir) recDir.set(s.recording_dir);
      if (typeof s.volume === 'number') volume.set(Math.round(s.volume * 100));
      if (typeof s.monitor_file_volume === 'number')
        monitorFileVolume.set(Math.round(s.monitor_file_volume * 100));
      micDenoise.set(!!s.mic_denoise);
    });

    const np = listen<Track>('now_playing', (e) => {
      nowPlaying.set(e.payload);
      playing.set(true);
      const idx = get(playlist).findIndex((t) => t.path === e.payload.path);
      currentIndex.set(idx >= 0 ? idx : null);
      streamTitle.set(e.payload.title);
      setStreamTitle(e.payload.title);
    });
    const ended = listen('track_ended', () => {
      playing.set(false);
      next();
    });
    const lvl = listen<LevelMeter>('levels', (e) => levels.set(e.payload));
    const pos = listen<Position>('position', (e) => position.set(e.payload));
    const status = listen<StreamStatus>('stream_status', (e) => {
      streamStatus.set(e.payload);
      if (e.payload.state === 'connecting' || e.payload.state === 'disconnected') {
        streamStats.set({ kbps: 0, total_bytes: 0 });
      }
    });
    const stats = listen<StreamStats>('stream_stats', (e) => streamStats.set(e.payload));
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
    const wv2 = listen<{ downloaded: number; total: number | null }>('webview2_progress', (e) =>
      wv2Progress.set(e.payload),
    );

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
      stats.then((u) => u());
      rec.then((u) => u());
      err.then((u) => u());
      wv2.then((u) => u());
      dd.then((u) => u());
    };
  });
</script>

<main class="flex h-screen flex-col bg-gray-800 text-gray-100">
  <WelcomeModal />
  <Toaster />
  {#if $errorMsg}
    <div
      class="flex items-start gap-2 border-b border-red-900/50 bg-red-900/30 px-4 py-2 text-sm text-red-300"
    >
      <span class="flex-1">{$errorMsg}</span>
      <button class="text-red-500 hover:text-red-400" onclick={() => errorMsg.set(null)} aria-label="Dismiss error"
        >✕</button
      >
    </div>
  {/if}

  <NowPlaying />

  <section class="min-h-0 flex-1 overflow-y-auto">
    {#if $view === 'home'}
      <Playlist />
    {:else if $view === 'stream'}
      <div class="space-y-4 p-4">
        <BroadcastBar />
        <div class="grid grid-cols-2 gap-4">
          <div class="joko-card rounded-xl p-4">
            <h3 class="joko-label mb-2">Record</h3>
            <RecordBar />
          </div>
          <div class="joko-card rounded-xl p-4">
            <h3 class="joko-label mb-2">Source</h3>
            <SourceToggle />
          </div>
        </div>
        <div class="joko-card rounded-xl p-4">
          <h3 class="joko-label mb-2">Levels</h3>
          <Meters />
        </div>
        <div class="joko-card rounded-xl p-4">
          <h3 class="joko-label mb-2">Mixer</h3>
          <Mixer />
        </div>
      </div>
    {:else if $view === 'profile-editor'}
      <div class="p-4">
        <div class="joko-card rounded-xl p-4">
          <ProfileEditor />
        </div>
      </div>
    {:else}
      <div class="space-y-4 p-4">
        <div class="joko-card rounded-xl p-4">
          <h3 class="joko-label mb-3">Devices</h3>
          <DeviceChoosers />
        </div>
        <div class="joko-card rounded-xl p-4">
          <h3 class="joko-label mb-3">Recording folder</h3>
          <RecordingSettings />
        </div>
        <div class="joko-card rounded-xl p-4">
          <h3 class="joko-label mb-3">Connection profiles</h3>
          <ProfileManager />
        </div>
        <div class="joko-card rounded-xl p-4">
          <h3 class="joko-label mb-1">Runtime</h3>
          <p class="mb-3 text-[11px] text-gray-400">
            WebView2 is auto-installed with Streamy on supported Windows. Run repair if the app ever
            fails to start with a WebView2 message.
          </p>
          <WebView2Repair />
        </div>
      </div>
    {/if}
  </section>

  {#if $view !== 'profile-editor'}
    <BottomNav />
  {/if}
</main>
