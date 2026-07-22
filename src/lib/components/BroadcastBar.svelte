<script lang="ts">
  import { profiles, streamStatus, bitrate } from '../stores';
  import { connectStream, disconnectStream, BITRATES } from '../api';
  import ProfileDialog from './ProfileDialog.svelte';

  let selected = $state('');
  let showProfiles = $state(false);

  // Select the first profile once the list loads.
  $effect(() => {
    if (!selected && $profiles.length) selected = $profiles[0].name;
  });

  const connected = $derived($streamStatus.state === 'connected');
  const busy = $derived($streamStatus.state === 'connecting');

  const statusText = $derived(
    $streamStatus.state === 'connected'
      ? '● Live'
      : $streamStatus.state === 'connecting'
        ? 'Connecting…'
        : $streamStatus.state === 'error'
          ? `✕ ${$streamStatus.message ?? 'error'}`
          : '○ Off air'
  );

  function toggle() {
    if (connected) disconnectStream();
    else if (selected) connectStream(selected, $bitrate);
  }
</script>

<div class="flex items-center gap-2 px-4 py-2 text-xs">
  <span class="text-zinc-500">Server</span>
  <select
    class="min-w-0 rounded bg-zinc-800 px-2 py-1 text-zinc-200"
    bind:value={selected}
    disabled={connected}
  >
    {#if $profiles.length === 0}
      <option value="">(no profiles)</option>
    {/if}
    {#each $profiles as p (p.name)}
      <option value={p.name}>{p.name}</option>
    {/each}
  </select>

  <select class="rounded bg-zinc-800 px-2 py-1 text-zinc-200" bind:value={$bitrate} disabled={connected}>
    {#each BITRATES as b (b)}
      <option value={b}>{b} kbps</option>
    {/each}
  </select>

  <button
    class="rounded px-3 py-1 font-medium {connected
      ? 'bg-red-700 text-white hover:bg-red-600'
      : 'bg-emerald-700 text-white hover:bg-emerald-600'}"
    onclick={toggle}
    disabled={busy || (!connected && !selected)}
  >
    {connected ? 'Disconnect' : busy ? '…' : 'Connect'}
  </button>

  <span class="text-zinc-400">{statusText}</span>
  <button class="ml-auto text-zinc-400 underline" onclick={() => (showProfiles = true)}
    >Profiles…</button
  >
</div>

{#if showProfiles}
  <ProfileDialog onClose={() => (showProfiles = false)} />
{/if}
