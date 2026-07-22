<script lang="ts">
  import { inputDevices, outputDevices, selectedInput, errorMsg } from '../stores';
  import { setInputDevice } from '../api';

  async function onInput(e: Event) {
    const name = (e.currentTarget as HTMLSelectElement).value;
    selectedInput.set(name);
    try {
      await setInputDevice(name);
    } catch (e) {
      errorMsg.set(String(e));
    }
  }
</script>

<div class="flex flex-col gap-1 text-xs text-zinc-400">
  <label class="flex items-center gap-2">
    <span class="w-14 shrink-0 text-zinc-500">Mic</span>
    <select
      class="min-w-0 flex-1 rounded bg-zinc-800 px-2 py-1 text-zinc-200"
      value={$selectedInput}
      onchange={onInput}
    >
      {#each $inputDevices as d (d.id)}
        <option value={d.id}>{d.name}</option>
      {/each}
    </select>
  </label>
  <div class="flex items-center gap-2">
    <span class="w-14 shrink-0 text-zinc-500">Output</span>
    <span class="truncate text-zinc-300">{$outputDevices[0]?.name ?? 'system default'}</span>
  </div>
</div>
