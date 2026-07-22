<script lang="ts">
  import { view } from '../stores';
  import type { View } from '../stores';

  const tabs: { id: View; label: string; icon: string }[] = [
    { id: 'home', label: 'Home', icon: '☰' },
    { id: 'stream', label: 'Stream', icon: '📡' },
    { id: 'settings', label: 'Settings', icon: '⚙' },
  ];
</script>

<nav class="sticky bottom-0 flex border-t border-zinc-800 bg-zinc-950/95 pb-[env(safe-area-inset-bottom)]">
  {#each tabs as t (t.id)}
    <button
      class="flex flex-1 flex-col items-center gap-0.5 py-2 text-[10px] {$view === t.id
        ? 'text-cyan-400'
        : 'text-zinc-500 hover:text-zinc-300'}"
      onclick={() => view.set(t.id)}
    >
      <span class="text-base">{t.icon}</span>{t.label}
    </button>
  {/each}
</nav>
