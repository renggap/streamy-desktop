<script lang="ts">
  import { getVersion } from '@tauri-apps/api/app';
  import { onMount } from 'svelte';
  import { view } from '../stores';
  import type { View } from '../stores';

  let version = '';
  onMount(() => {
    getVersion().then((v) => (version = v)).catch(() => {});
  });

  const tabs: { id: View; label: string; icon: string }[] = [
    { id: 'home', label: 'Home', icon: '☰' },
    { id: 'stream', label: 'Stream', icon: '📡' },
    { id: 'settings', label: 'Settings', icon: '⚙' },
  ];
</script>

<nav
  class="sticky bottom-0 flex items-stretch border-t border-gray-200 bg-white/95 pb-[env(safe-area-inset-bottom)]"
>
  <!-- Streamy Desktop wordmark -->
  <div class="flex shrink-0 items-center gap-2 border-r border-gray-200 px-4">
    <span class="eq" aria-hidden="true">
      <i></i><i></i><i></i><i></i>
    </span>
    <span class="leading-none">
      <span class="block bg-gradient-to-r from-lime-500 to-lime-600 bg-clip-text text-sm font-extrabold tracking-tight text-transparent">
        Streamy
      </span>
      <span class="block text-[8px] font-semibold tracking-[0.34em] text-gray-400">DESKTOP</span>
      {#if version}
        <span class="block text-[8px] tabular-nums text-gray-400">v{version}</span>
      {/if}
    </span>
  </div>

  {#each tabs as t (t.id)}
    <button
      class="relative flex flex-1 flex-col items-center justify-center gap-0.5 py-2 text-[10px] font-medium uppercase tracking-wider transition-colors {$view === t.id
        ? 'text-lime-600'
        : 'text-gray-400 hover:text-gray-700'}"
      onclick={() => view.set(t.id)}
    >
      {#if $view === t.id}
        <span class="absolute inset-x-3 top-0 h-0.5 rounded-full bg-lime-500"></span>
      {/if}
      <span class="text-base">{t.icon}</span>{t.label}
    </button>
  {/each}
</nav>

<style>
  /* Mini live equalizer mark; lime on light chip to match the Joko UI accent. */
  .eq {
    display: inline-flex;
    align-items: flex-end;
    gap: 2px;
    height: 18px;
    padding: 2px;
    border-radius: 4px;
    background: #f9fafb; /* gray-50 */
    border: 1px solid #e5e7eb; /* gray-200 */
  }
  .eq i {
    width: 3px;
    height: 30%;
    border-radius: 1px;
    background: #84cc16; /* lime-500 */
    animation: eq-bounce 1.1s ease-in-out infinite;
  }
  .eq i:nth-child(2) { animation-delay: 0.18s; height: 60%; }
  .eq i:nth-child(3) { animation-delay: 0.36s; height: 90%; }
  .eq i:nth-child(4) { animation-delay: 0.54s; height: 50%; }
  @keyframes eq-bounce {
    0%, 100% { transform: scaleY(0.4); }
    50% { transform: scaleY(1.3); }
  }
  @media (prefers-reduced-motion: reduce) {
    .eq i { animation: none; }
  }
</style>
