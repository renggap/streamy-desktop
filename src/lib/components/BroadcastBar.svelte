<script lang="ts">
  import { profiles, streamStatus, bitrate, codec, view, errorMsg, streamTitle, streamStats, titleOwned } from '../stores';
  import { notify } from '../stores';
  import { connectStream, disconnectStream, setStreamTitle, BITRATES } from '../api';

  const TITLE_GALLERY_MAX = 10;

  // localStorage can throw in hardened/privacy browser contexts — guard every
  // access so a locked-down storage never breaks the bar on load.
  function localGet(k: string): string | null {
    try {
      return localStorage.getItem(k);
    } catch {
      return null;
    }
  }
  function localSet(k: string, v: string) {
    try {
      localStorage.setItem(k, v);
    } catch {
      /* best-effort persistence only */
    }
  }

  // ponytail: localStorage is enough; move to profiles.json "last_used" if
  // selection ever needs to sync with the Rust side.
  let selected = $state(localGet('streamy.profile') ?? '');
  // The title is one piece of state: the `streamTitle` store. It survives an
  // Update press, view switches (the bar unmounts) and app restarts. A track
  // change still pushes its own title to the server, but never silently
  // replaces what the DJ typed once they have taken ownership (see App.svelte).
  let metaStatus = $state<'idle' | 'sending' | 'ok' | 'err'>('idle');
  let metaTimer: ReturnType<typeof setTimeout> | undefined;
  let gallery = $state<string[]>(loadGallery());
  let showGallery = $state(false);

  function loadGallery(): string[] {
    try {
      const v = JSON.parse(localGet('streamy.titles') ?? '[]');
      return Array.isArray(v) ? v.filter((x) => typeof x === 'string').slice(0, TITLE_GALLERY_MAX) : [];
    } catch {
      return [];
    }
  }
  function saveGallery(list: string[]) {
    gallery = list.slice(0, TITLE_GALLERY_MAX);
    localSet('streamy.titles', JSON.stringify(gallery));
  }

  $effect(() => {
    if (selected) localSet('streamy.profile', selected);
  });

  $effect(() => {
    // fall back to first profile on fresh start or if saved profile was deleted
    if (selected && $profiles.length && !$profiles.some((p) => p.name === selected)) selected = '';
    if (!selected && $profiles.length) selected = $profiles[0].name;
  });

  const connected = $derived($streamStatus.state === 'connected');
  const busy = $derived($streamStatus.state === 'connecting');
  const hasProfile = $derived($profiles.length > 0);
  const statsLine = $derived(
    connected
      ? `▲ ${$streamStats.kbps.toFixed(1)} kb/s · ${($streamStats.total_bytes / (1024 * 1024)).toFixed(1)} MB`
      : ''
  );

  async function connect(name: string) {
    await connectStream(name, $bitrate, $codec);
  }

  async function toggle() {
    if (connected) {
      notify('info', 'Disconnecting…', selected || undefined);
      await disconnectStream();
      return;
    }
    const name = selected || $profiles[0]?.name;
    if (!name) {
      notify('error', 'No profile', 'Add a connection profile first.');
      view.set('settings');
      return;
    }
    selected = name;
    notify('info', 'Connecting…', name);
    try {
      await connect(name);
    } catch (e) {
      errorMsg.set(String(e));
      notify('error', 'Connect failed', String(e));
    }
  }

  async function liveReconnect() {
    if (!connected) return;
    const name = selected || $profiles[0]?.name;
    if (!name) return;
    notify('info', 'Reconnecting…', `${$codec.toUpperCase()} · ${$bitrate} kbps`);
    try {
      await disconnectStream();
      await connect(name);
    } catch (e) {
      errorMsg.set(String(e));
    }
  }

  function onBitrateChange() {
    liveReconnect();
  }
  function onCodecChange() {
    liveReconnect();
  }

  async function pushTitle(t: string) {
    clearTimeout(metaTimer);
    metaStatus = 'sending';
    try {
      await setStreamTitle(t);
      metaStatus = 'ok';
      notify('success', 'Title updated', t);
    } catch (e) {
      metaStatus = 'err';
      errorMsg.set(String(e));
      notify('error', 'Title update failed', String(e));
    }
    metaTimer = setTimeout(() => {
      metaStatus = 'idle';
    }, 2500);
  }

  async function sendTitle() {
    const t = ($streamTitle || '').trim();
    if (!t || !connected) return;
    streamTitle.set(t);
    titleOwned.set(true);
    await pushTitle(t);
  }

  // --- Title gallery: up to TITLE_GALLERY_MAX saved titles, one click re-sends ---
  function pinTitle() {
    const t = ($streamTitle || '').trim();
    if (!t) return;
    if (gallery.includes(t)) {
      notify('info', 'Already saved', t);
      return;
    }
    if (gallery.length >= TITLE_GALLERY_MAX) {
      notify('error', 'Gallery full', `Max ${TITLE_GALLERY_MAX} titles. Remove one first.`);
      return;
    }
    saveGallery([...gallery, t]);
    notify('success', 'Title saved', t);
  }

  function useTitle(t: string) {
    streamTitle.set(t);
    titleOwned.set(true);
    if (connected) pushTitle(t);
  }

  function removeTitle(t: string) {
    saveGallery(gallery.filter((x) => x !== t));
  }

  const label = $derived(
    connected ? 'DISCONNECT' : busy ? 'CONNECTING…' : hasProfile ? 'CONNECT' : 'ADD PROFILE'
  );
</script>

<div class="joko-card rounded-xl p-4">
  <div class="mb-3 flex items-center justify-between">
    <h3 class="joko-label">Broadcast</h3>
    <!-- On-air indicator -->
    <div class="flex items-center gap-2 text-xs font-semibold uppercase tracking-wider">
      <span class="relative flex h-3 w-3">
        {#if connected}
          <span class="onair-ping absolute inline-flex h-full w-full rounded-full bg-lime-400"></span>
        {/if}
        <span
          class="relative inline-flex h-3 w-3 rounded-full {connected
            ? 'onair-beat bg-lime-400'
            : 'bg-gray-300'}"
        ></span>
      </span>
      <span class={connected ? 'text-lime-400' : 'text-gray-400'}>{connected ? 'ON AIR' : 'OFF AIR'}</span>
    </div>
  </div>

  <div class="grid grid-cols-2 gap-4">
    <!-- Left: server + codec + bitrate -->
    <div class="space-y-2 text-xs">
      <label class="block">
        <span class="joko-label mb-1 block">Server</span>
        <select
          class="w-full rounded-lg border border-gray-600 bg-gray-900 px-2 py-1.5 text-gray-100 transition-colors focus:border-lime-500 disabled:opacity-60"
          bind:value={selected}
          disabled={connected}
        >
          {#if !hasProfile}<option value="">(no profiles)</option>{/if}
          {#each $profiles as p (p.name)}<option value={p.name}>{p.name}</option>{/each}
        </select>
      </label>
      <div class="grid grid-cols-2 gap-2">
        <label class="block">
          <span class="joko-label mb-1 block">Codec</span>
          <select
            class="w-full rounded-lg border border-gray-600 bg-gray-900 px-2 py-1.5 text-gray-100 transition-colors focus:border-lime-500"
            bind:value={$codec}
            onchange={onCodecChange}
          >
            <option value="mp3">MP3</option>
            <option value="aac">AAC</option>
          </select>
        </label>
        <label class="block">
          <span class="joko-label mb-1 block">Bitrate</span>
          <select
            class="w-full rounded-lg border border-gray-600 bg-gray-900 px-2 py-1.5 text-gray-100 transition-colors focus:border-lime-500"
            bind:value={$bitrate}
            onchange={onBitrateChange}
          >
            {#each BITRATES as b (b)}<option value={b}>{b} kbps</option>{/each}
          </select>
        </label>
      </div>
    </div>

    <!-- Right: big connect button -->
    <div class="flex flex-col">
      <button
        class="flex-1 rounded-lg text-sm font-bold uppercase tracking-wider transition-all
          {connected
            ? 'bg-red-600 text-white shadow-md hover:bg-red-700 focus:outline-none focus:ring-2 focus:ring-red-500 focus:ring-offset-2 focus:ring-offset-gray-800 active:scale-[0.98]'
            : 'bg-lime-600 text-white shadow-md hover:bg-lime-700 focus:outline-none focus:ring-2 focus:ring-lime-500 focus:ring-offset-2 focus:ring-offset-gray-800 active:scale-[0.98]'}"
        onclick={toggle}
        disabled={busy}
      >
        {busy ? '…' : label}
      </button>
      <div class="mt-2 text-center text-[10px] uppercase tracking-wider text-gray-400">
        {#if connected}
          {$codec.toUpperCase()} · {$bitrate} kbps
        {:else if $streamStatus.state === 'error'}
          <span class="text-red-400">{$streamStatus.message ?? 'error'}</span>
        {:else}
          idle
        {/if}
      </div>
      {#if connected}
        <div class="mt-1 text-[10px] tabular-nums text-lime-400">{statsLine}</div>
      {/if}
    </div>
  </div>

  <!-- Stream title (songtitle): Ultravox sends it in-band (0x3902); Icecast uses
       the /admin/metadata endpoint. Both reuse the source/DJ creds — no separate
       admin password. -->
  <div class="mt-3 border-t border-gray-600 pt-3">
    <div class="mb-1 flex items-center justify-between">
      <span class="joko-label">Stream title</span>
      <button
        class="text-[10px] font-semibold uppercase tracking-wider text-lime-400 transition-colors hover:text-lime-300"
        onclick={() => (showGallery = !showGallery)}
        aria-expanded={showGallery}
        aria-controls="title-gallery"
        >Titles ({gallery.length}/{TITLE_GALLERY_MAX})</button
      >
    </div>
    <div class="flex gap-2">
      <input
        class="min-w-0 flex-1 rounded-lg border border-gray-600 bg-gray-900 px-2 py-1.5 text-xs text-gray-100 transition-colors placeholder:text-gray-400 focus:border-lime-500"
        placeholder="song / show name"
        bind:value={$streamTitle}
        oninput={() => (titleOwned.set(!!$streamTitle.trim()))}
        onkeydown={(e) => e.key === 'Enter' && sendTitle()}
      />
      <button
        class="rounded-lg bg-gray-900 px-2.5 py-1.5 text-xs text-gray-300 shadow-md transition-colors hover:bg-gray-700 hover:text-white disabled:opacity-40"
        onclick={pinTitle}
        disabled={!$streamTitle.trim()}
        title="Save to gallery"
        aria-label="Save title to gallery">📌</button
      >
      <button
        class="rounded-lg bg-gray-900 px-3 py-1.5 text-xs font-semibold text-white shadow-md transition-colors hover:bg-gray-700 disabled:opacity-40"
        onclick={sendTitle}
        disabled={!connected || metaStatus === 'sending'}
      >
        {metaStatus === 'sending' ? 'Sending…' : 'Update'}
      </button>
    </div>
    {#if metaStatus === 'ok'}
      <div class="mt-1 text-[10px] text-lime-400">✓ Title sent to server</div>
    {:else if metaStatus === 'err'}
      <div class="mt-1 text-[10px] text-red-400">✕ Update failed</div>
    {/if}

    {#if showGallery}
      <div id="title-gallery" class="mt-2 space-y-1">
        {#each gallery as t (t)}
          <div class="flex items-center gap-2 rounded-lg border border-gray-600 bg-gray-900 px-2 py-1">
            <button
              class="min-w-0 flex-1 truncate text-left text-xs text-gray-100 transition-colors hover:text-lime-300"
              class:font-semibold={$streamTitle === t}
              class:text-lime-400={$streamTitle === t}
              onclick={() => useTitle(t)}
              title="{t} — click to use"
              >{t}</button
            >
            <button
              class="shrink-0 rounded px-1.5 py-0.5 text-[11px] text-gray-500 transition-colors hover:bg-red-900/40 hover:text-red-400"
              onclick={() => removeTitle(t)}
              aria-label="Remove {t}"
              >✕</button
            >
          </div>
        {:else}
          <div class="rounded-lg border border-dashed border-gray-500 px-3 py-2 text-[11px] text-gray-400">
            No saved titles yet. Type a title and press 📌.
          </div>
        {/each}
      </div>
    {/if}
  </div>
</div>

<style>
  .onair-ping {
    animation: onair-ping 1.2s cubic-bezier(0, 0, 0.2, 1) infinite;
  }
  .onair-beat {
    animation: onair-beat 1.2s ease-in-out infinite;
  }
  @keyframes onair-beat {
    0%, 100% { transform: scale(1); }
    25% { transform: scale(1.45); }
    40% { transform: scale(1); }
    55% { transform: scale(1.3); }
  }
  @keyframes onair-ping {
    0% { transform: scale(1); opacity: 0.8; }
    75%, 100% { transform: scale(2.2); opacity: 0; }
  }
  @media (prefers-reduced-motion: reduce) {
    .onair-ping, .onair-beat { animation: none; }
  }
</style>
