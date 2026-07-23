<script lang="ts">
  import { inputDevices, outputDevices, selectedInput, selectedOutput, errorMsg } from '../stores';
  import { setInputDevice, setOutputDevice } from '../api';

  async function onInput(e: Event) {
    const name = (e.currentTarget as HTMLSelectElement).value;
    selectedInput.set(name);
    try {
      await setInputDevice(name);
    } catch (e) {
      errorMsg.set(String(e));
    }
  }

  async function onOutput(e: Event) {
    const v = (e.currentTarget as HTMLSelectElement).value;
    const name = v === '' ? null : v;
    selectedOutput.set(v);
    try {
      await setOutputDevice(name);
    } catch (e) {
      errorMsg.set(String(e));
    }
  }
</script>

<div class="space-y-3 text-xs">
  <label class="block">
    <span class="mb-1 block text-[10px] uppercase tracking-wider text-zinc-500">Microphone</span>
    <select
      class="w-full rounded border border-zinc-700 bg-zinc-900 px-2 py-1.5 text-zinc-200"
      value={$selectedInput}
      onchange={onInput}
    >
      {#each $inputDevices as d (d.id)}<option value={d.id}>{d.name}</option>{/each}
    </select>
  </label>
  <label class="block">
    <span class="mb-1 block text-[10px] uppercase tracking-wider text-zinc-500">Output</span>
    <select
      class="w-full rounded border border-zinc-700 bg-zinc-900 px-2 py-1.5 text-zinc-200"
      value={$selectedOutput}
      onchange={onOutput}
    >
      <option value="">System default</option>
      {#each $outputDevices as d (d.id)}<option value={d.id}>{d.name}</option>{/each}
    </select>
  </label>
</div>
