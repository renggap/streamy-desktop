<script lang="ts">
  import { sourceMode } from '../stores';
  import { setSourceMode } from '../api';
  import type { SourceMode } from '../api';

  const modes: SourceMode[] = ['file', 'mic', 'both'];

  function pick(m: SourceMode) {
    sourceMode.set(m);
    setSourceMode(m);
  }
</script>

<div class="flex items-center gap-1 text-xs">
  <span class="mr-1 text-zinc-500">Source</span>
  {#each modes as m (m)}
    <button
      class="rounded px-2 py-1 capitalize {$sourceMode === m
        ? 'bg-zinc-200 text-zinc-900'
        : 'bg-zinc-800 text-zinc-300 hover:bg-zinc-700'}"
      onclick={() => pick(m)}>{m}</button
    >
  {/each}
</div>
