<script lang="ts">
  import { profiles, streamStatus, bitrate, view, errorMsg } from '../stores';
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

  async function toggle() {
    if (connected) {
      await disconnectStream();
      return;
    }
    // Connect to the selected profile, or the first one if none is picked yet.
    const name = selected || $profiles[0]?.name;
    if (!name) {
      view.set('settings'); // no profile at all -> go create one
      return;
    }
    selected = name;
    try {
      await connectStream(name, $bitrate);
    } catch (e) {
      errorMsg.set(String(e));
    }
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
    <span class="text-zinc-500">Bitrate</span>
    <select class="flex-1 rounded bg-zinc-800 px-2 py-1 text-zinc-200" bind:value={$bitrate} disabled={connected}>
      {#each BITRATES as b (b)}<option value={b}>{b} kbps</option>{/each}
    </select>
    <button
      class="rounded px-3 py-1 font-medium {connected
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
