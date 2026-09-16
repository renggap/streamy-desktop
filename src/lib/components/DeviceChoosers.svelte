<script lang="ts">
  import { inputDevices, outputDevices, selectedInput, selectedOutput, volume, errorMsg, micDenoise } from '../stores';
  import { notify } from '../stores';
  import { setInputDevice, setOutputDevice, setVolume, setMicDenoise } from '../api';

  async function onInput(e: Event) {
    const name = (e.currentTarget as HTMLSelectElement).value;
    selectedInput.set(name);
    try {
      await setInputDevice(name);
      notify('success', 'Microphone set', name);
    } catch (e) {
      errorMsg.set(String(e));
    }
  }

  // Mic noise suppression: broadcast-only (RNNoise). Never touches the
  // monitor path (the mic never plays locally anyway).
  async function onDenoise(e: Event) {
    const on = (e.currentTarget as HTMLInputElement).checked;
    micDenoise.set(on);
    try {
      await setMicDenoise(on);
      notify('success', on ? 'Noise suppression on' : 'Noise suppression off', on ? 'RNNoise cleaning the mic before broadcast' : undefined);
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
      notify('success', 'Output set', name ?? 'System default');
    } catch (e) {
      errorMsg.set(String(e));
    }
  }

  // Master volume: 0..200% (100 = unity). The store updates instantly for
  // display; the backend apply+persist is debounced so a drag doesn't fire one
  // IPC call + settings.json write per pixel (or overflow the engine action
  // ring, which would surface a spurious "audio control queue full").
  let volTimer: ReturnType<typeof setTimeout> | undefined;
  function onVolume(e: Event) {
    const pct = parseInt((e.currentTarget as HTMLInputElement).value, 10);
    volume.set(pct);
    clearTimeout(volTimer);
    volTimer = setTimeout(() => {
      setVolume(pct).catch((e) => errorMsg.set(String(e)));
    }, 120);
  }
</script>

<div class="space-y-3 text-xs">
  <label class="block">
    <span class="joko-label mb-1 block">Microphone</span>
    <select
      class="w-full rounded-lg border border-gray-600 bg-gray-900 px-2 py-1.5 text-gray-100 transition-colors focus:border-lime-500"
      value={$selectedInput}
      onchange={onInput}
    >
      {#each $inputDevices as d (d.id)}<option value={d.id}>{d.name}</option>{/each}
    </select>
  </label>
  <label class="flex cursor-pointer items-center justify-between gap-2">
    <span class="joko-label">Noise suppression</span>
    <input
      type="checkbox"
      class="h-4 w-4 accent-lime-500"
      checked={$micDenoise}
      onchange={onDenoise}
      aria-label="Mic noise suppression for broadcast"
    />
  </label>
  <label class="block">
    <span class="joko-label mb-1 block">Output</span>
    <select
      class="w-full rounded-lg border border-gray-600 bg-gray-900 px-2 py-1.5 text-gray-100 transition-colors focus:border-lime-500"
      value={$selectedOutput}
      onchange={onOutput}
    >
      <option value="">System default</option>
      {#each $outputDevices as d (d.id)}<option value={d.id}>{d.name}</option>{/each}
    </select>
  </label>
  <label class="block">
    <span class="joko-label mb-1 flex items-center justify-between">
      <span>Volume</span>
      <span class="tabular-nums text-gray-300">{$volume}%</span>
    </span>
    <input
      type="range"
      min="0"
      max="200"
      step="1"
      value={$volume}
      oninput={onVolume}
      class="w-full cursor-pointer"
      aria-label="Master volume"
    />
  </label>
</div>
