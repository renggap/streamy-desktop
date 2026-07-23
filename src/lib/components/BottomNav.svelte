<script lang="ts">
  import { view } from '../stores';
  import type { View } from '../stores';

  const tabs: { id: View; label: string; icon: string }[] = [
    { id: 'home', label: 'Home', icon: '☰' },
    { id: 'stream', label: 'Stream', icon: '📡' },
    { id: 'settings', label: 'Settings', icon: '⚙' },
  ];
</script>

<nav
  class="sticky bottom-0 flex items-stretch border-t border-zinc-800 bg-zinc-950/95 pb-[env(safe-area-inset-bottom)]"
>
  <!-- Streamy Desktop wordmark -->
  <div class="flex shrink-0 items-center gap-2 border-r border-zinc-800 px-4">
    <span class="eq" aria-hidden="true">
      <i></i><i></i><i></i><i></i>
    </span>
    <span class="leading-none">
      <span class="block bg-gradient-to-r from-emerald-400 to-cyan-400 bg-clip-text text-sm font-extrabold tracking-tight text-transparent">
        Streamy
      </span>
      <span class="block text-[8px] font-semibold tracking-[0.34em] text-zinc-500">DESKTOP</span>
    </span>
  </div>

  {#each tabs as t (t.id)}
    <button
      class="relative flex flex-1 flex-col items-center justify-center gap-0.5 py-2 text-[10px] uppercase tracking-wider transition-colors {$view === t.id
        ? 'text-cyan-400'
        : 'text-zinc-500 hover:text-zinc-300'}"
      onclick={() => view.set(t.id)}
    >
      {#if $view === t.id}
        <span class="absolute inset-x-3 top-0 h-0.5 rounded-full bg-cyan-400"></span>
      {/if}
      <span class="text-base">{t.icon}</span>{t.label}
    </button>
  {/each}
</nav>

<style>
  /* Mini live equalizer mark. */
  .eq {
    display: inline-flex;
    align-items: flex-end;
    gap: 2px;
    height: 18px;
    padding: 2px;
    border-radius: 4px;
    background: linear-gradient(#0a1410, #050a08);
    border: 1px solid #064e3b;
    box-shadow: inset 0 0 6px rgba(16, 185, 129, 0.2);
  }
  .eq i {
    width: 3px;
    height: 30%;
    border-radius: 1px;
    background: linear-gradient(#34d399, #06b6d4);
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
