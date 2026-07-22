<script lang="ts">
  import { profiles, streamStatus, bitrate, codec, view, errorMsg } from '../stores';
  import { connectStream, disconnectStream, BITRATES } from '../api';

  let selected = $state('');

  $effect(() => {
    if (!selected && $profiles.length) selected = $profiles[0].name;
  });

  const connected = $derived($streamStatus.state === 'connected');
  const busy = $derived($streamStatus.state === 'connecting');
  const hasProfile = $derived($profiles.length > 0);

  const statusText = $derived(
    $streamStatus.state === 'connected'
      ? '● Live'
      : $streamStatus.state === 'connecting'
        ? 'Connecting…'
        : $streamStatus.state === 'error'
          ? `✕ ${$streamStatus.message ?? 'error'}`
          : '○ Off air'
  );

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

  // P4.3: changing codec/bitrate while live tears down and brings the stream
  // back up with the new config (the mount can't change codec mid-stream).
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

  const label = $derived(
    connected ? 'Disconnect' : busy ? '…' : hasProfile ? 'Connect' : 'Add profile'
  );
</script>

<div class="space-y-2">
  <div class="flex items-center gap-2 text-xs">
    <span class="text-zinc-500">Server</span>
    <select
      class="min-w-0 flex-1 rounded bg-zinc-800 px-2 py-1 text-zinc-200"
      bind:value={selected}
      disabled={connected}
    >
      {#if !hasProfile}<option value="">(no profiles)</option>{/if}
      {#each $profiles as p (p.name)}<option value={p.name}>{p.name}</option>{/each}
    </select>
  </div>
  <div class="flex items-center gap-2 text-xs">
    <span class="text-zinc-500">Codec</span>
    <select
      class="rounded bg-zinc-800 px-2 py-1 text-zinc-200"
      bind:value={$codec}
      onchange={onCodecChange}
    >
      <option value="mp3">MP3</option>
      <option value="aac">AAC</option>
    </select>
    <span class="text-zinc-500">Bitrate</span>
    <select
      class="flex-1 rounded bg-zinc-800 px-2 py-1 text-zinc-200"
      bind:value={$bitrate}
      onchange={onBitrateChange}
    >
      {#each BITRATES as b (b)}<option value={b}>{b} kbps</option>{/each}
    </select>
  </div>
  <div class="flex items-center gap-2">
    <button
      class="flex-1 rounded px-3 py-1 text-xs font-medium {connected
        ? 'bg-red-700 text-white hover:bg-red-600'
        : 'bg-emerald-700 text-white hover:bg-emerald-600'}"
      onclick={toggle}
      disabled={busy}
    >
      {label}
    </button>
  </div>
  <div class="text-xs text-zinc-400">{statusText}</div>
</div>
