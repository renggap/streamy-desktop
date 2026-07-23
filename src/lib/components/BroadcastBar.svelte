<script lang="ts">
  import { profiles, streamStatus, bitrate, codec, view, errorMsg, streamTitle } from '../stores';
  import { connectStream, disconnectStream, setStreamTitle, BITRATES } from '../api';

  let selected = $state('');
  let titleDraft = $state('');

  $effect(() => {
    if (!selected && $profiles.length) selected = $profiles[0].name;
  });

  const connected = $derived($streamStatus.state === 'connected');
  const busy = $derived($streamStatus.state === 'connecting');
  const hasProfile = $derived($profiles.length > 0);

  async function connect(name: string) {
    await connectStream(name, $bitrate, $codec);
  }

  async function toggle() {
    if (connected) {
      await disconnectStream();
      return;
    }
    const name = selected || $profiles[0]?.name;
    if (!name) {
      view.set('settings');
      return;
    }
    selected = name;
    try {
      await connect(name);
    } catch (e) {
      errorMsg.set(String(e));
    }
  }

  async function liveReconnect() {
    if (!connected) return;
    const name = selected || $profiles[0]?.name;
    if (!name) return;
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

  async function sendTitle() {
    const t = (titleDraft || '').trim();
    if (!t) return;
    try {
      await setStreamTitle(t);
      streamTitle.set(t);
    } catch (e) {
      errorMsg.set(String(e));
    }
  }

  const label = $derived(
    connected ? 'DISCONNECT' : busy ? 'CONNECTING…' : hasProfile ? 'CONNECT' : 'ADD PROFILE'
  );
</script>

<div class="retro-chrome rounded-lg p-4">
  <div class="mb-3 flex items-center justify-between">
    <h3 class="text-xs font-semibold uppercase tracking-[0.2em] text-zinc-400">Broadcast</h3>
    <!-- On-air indicator -->
    <div class="flex items-center gap-2 text-xs font-semibold uppercase tracking-wider">
      <span class="relative flex h-3 w-3">
        {#if connected}
          <span class="onair-ping absolute inline-flex h-full w-full rounded-full bg-red-500"></span>
        {/if}
        <span class="relative inline-flex h-3 w-3 rounded-full {connected ? 'bg-red-500' : 'bg-zinc-600'}"></span>
      </span>
      <span class={connected ? 'text-red-400' : 'text-zinc-500'}>{connected ? 'ON AIR' : 'OFF AIR'}</span>
    </div>
  </div>

  <div class="grid grid-cols-2 gap-4">
    <!-- Left: server + codec + bitrate -->
    <div class="space-y-2 text-xs">
      <label class="block">
        <span class="mb-1 block text-[10px] uppercase tracking-wider text-zinc-500">Server</span>
        <select
          class="w-full rounded border border-zinc-700 bg-zinc-900 px-2 py-1.5 text-zinc-200"
          bind:value={selected}
          disabled={connected}
        >
          {#if !hasProfile}<option value="">(no profiles)</option>{/if}
          {#each $profiles as p (p.name)}<option value={p.name}>{p.name}</option>{/each}
        </select>
      </label>
      <div class="grid grid-cols-2 gap-2">
        <label class="block">
          <span class="mb-1 block text-[10px] uppercase tracking-wider text-zinc-500">Codec</span>
          <select
            class="w-full rounded border border-zinc-700 bg-zinc-900 px-2 py-1.5 text-zinc-200"
            bind:value={$codec}
            onchange={onCodecChange}
          >
            <option value="mp3">MP3</option>
            <option value="aac">AAC</option>
          </select>
        </label>
        <label class="block">
          <span class="mb-1 block text-[10px] uppercase tracking-wider text-zinc-500">Bitrate</span>
          <select
            class="w-full rounded border border-zinc-700 bg-zinc-900 px-2 py-1.5 text-zinc-200"
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
            ? 'bg-red-700 text-white hover:bg-red-600 active:scale-[0.98]'
            : 'bg-gradient-to-b from-emerald-500 to-emerald-700 text-white hover:from-emerald-400 hover:to-emerald-600 active:scale-[0.98]'}"
        onclick={toggle}
        disabled={busy}
      >
        {busy ? '…' : label}
      </button>
      <div class="mt-2 text-center text-[10px] uppercase tracking-wider text-zinc-500">
        {#if connected}
          {$codec.toUpperCase()} · {$bitrate} kbps
        {:else if $streamStatus.state === 'error'}
          <span class="text-red-400">{$streamStatus.message ?? 'error'}</span>
        {:else}
          idle
        {/if}
      </div>
    </div>
  </div>

  <!-- Stream title (songtitle, in-band via Ultravox 0x3902 — no admin password) -->
  <div class="mt-3 border-t border-zinc-800 pt-3">
    <span class="mb-1 block text-[10px] uppercase tracking-wider text-zinc-500">Stream title</span>
    <div class="flex gap-2">
      <input
        class="min-w-0 flex-1 rounded border border-zinc-700 bg-zinc-900 px-2 py-1.5 text-xs text-zinc-200"
        placeholder={$streamTitle || 'song / show name'}
        bind:value={titleDraft}
        onkeydown={(e) => e.key === 'Enter' && sendTitle()}
      />
      <button
        class="rounded bg-cyan-700 px-3 py-1.5 text-xs font-medium text-white hover:bg-cyan-600 disabled:opacity-40"
        onclick={sendTitle}
        disabled={!connected}
      >Update</button>
    </div>
  </div>
</div>

<style>
  .onair-ping {
    animation: onair-ping 1.2s cubic-bezier(0, 0, 0.2, 1) infinite;
  }
  @keyframes onair-ping {
    0% { transform: scale(1); opacity: 0.8; }
    75%, 100% { transform: scale(2.2); opacity: 0; }
  }
</style>
