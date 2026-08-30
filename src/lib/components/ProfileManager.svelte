<script lang="ts">
  import { profiles, errorMsg } from '../stores';
  import { listProfiles, saveProfile, deleteProfile } from '../api';
  import type { ConnectionProfile } from '../api';

  let editing = $state<ConnectionProfile>(blank());
  let password = $state('');
  let isEditing = $state(false);

  function blank(): ConnectionProfile {
    return {
      name: '',
      host: '',
      port: 8000,
      user: '',
      mount: '/',
      stream_id: 1,
      station_name: 'Streamy',
      genre: '',
      url: '',
      public: true,
      azuracast: false,
    };
  }

  async function refresh() {
    profiles.set(await listProfiles());
  }

  function edit(p: ConnectionProfile) {
    editing = { ...p };
    password = '';
    isEditing = true;
  }

  function reset() {
    editing = blank();
    password = '';
    isEditing = false;
  }

  async function save() {
    const exists = $profiles.some((p) => p.name === editing.name);
    if (!editing.name || !editing.host) {
      errorMsg.set('Name and host are required.');
      return;
    }
    if (!exists && !password) {
      errorMsg.set('Password is required for a new profile.');
      return;
    }
    // Blank password on edit -> null -> keep the existing stored password.
    const pw = password.length > 0 ? password : null;
    try {
      await saveProfile({ ...editing }, pw);
      await refresh();
      reset();
    } catch (e) {
      errorMsg.set(String(e));
    }
  }

  async function remove(name: string) {
    try {
      await deleteProfile(name);
      await refresh();
      if (isEditing && editing.name === name) reset();
    } catch (e) {
      errorMsg.set(String(e));
    }
  }
</script>

<div class="space-y-3">
  <h3 class="text-xs font-semibold uppercase tracking-wider text-gray-500">
    {isEditing ? 'Edit profile' : 'New profile'}
  </h3>
  <div class="grid grid-cols-2 gap-2 text-xs text-gray-300">
    <label class="flex flex-col gap-1"
      >Name <input class="rounded border border-gray-700 bg-gray-900 px-2 py-1.5 text-xs text-gray-200" bind:value={editing.name} disabled={isEditing} /></label
    >
    <label class="flex flex-col gap-1"
      >Host <input class="rounded border border-gray-700 bg-gray-900 px-2 py-1.5 text-xs text-gray-200" bind:value={editing.host} /></label
    >
    <label class="flex flex-col gap-1"
      >Port <input type="number" class="rounded border border-gray-700 bg-gray-900 px-2 py-1.5 text-xs text-gray-200" bind:value={editing.port} /></label
    >
    <label class="flex flex-col gap-1"
      >Stream ID <input type="number" class="rounded border border-gray-700 bg-gray-900 px-2 py-1.5 text-xs text-gray-200" bind:value={editing.stream_id} /></label
    >
    <label class="flex flex-col gap-1"
      >Mount
      <input
        class="rounded border border-gray-700 bg-gray-900 px-2 py-1.5 text-xs text-gray-200"
        bind:value={editing.mount}
        placeholder="/"
      /></label
    >
    <label class="flex flex-col gap-1"
      >Username
      <input
        class="rounded border border-gray-700 bg-gray-900 px-2 py-1.5 text-xs text-gray-200"
        bind:value={editing.user}
        placeholder="(Icecast source/DJ login)"
      /></label
    >
    <div class="col-span-2 text-[10px] leading-relaxed text-gray-500">
      Streamy auto-detects the protocol on connect — it tries Ultravox 2.1 (DNASv2) first, then falls
      back to Icecast / Liquidsoap harbor. <span class="text-gray-400">Stream ID</span> is used for
      Ultravox; <span class="text-gray-400">Mount</span> for Icecast. <span class="text-gray-400"
        >Username</span
      > is the Icecast source/DJ login, or the optional Ultravox DJ id (leave blank for Ultravox
      source login).
    </div>
    <label class="col-span-2 flex items-center gap-2 text-xs text-gray-300">
      <input type="checkbox" class="h-3.5 w-3.5 accent-lime-600" bind:checked={editing.azuracast} />
      AzuraCast server
      <span class="text-[10px] text-gray-500"
        >— DJ/source connects via Icecast even if it relays to Shoutcast DNAS for listeners. Skips the
        Ultravox probe so DJ login is immediate.</span
      >
    </label>
    <label class="col-span-2 flex flex-col gap-1"
      >Password
      <input
        type="password"
        class="rounded border border-gray-700 bg-gray-900 px-2 py-1.5 text-xs text-gray-200"
        bind:value={password}
        placeholder={isEditing ? '(unchanged)' : ''}
      /></label
    >
    <label class="flex flex-col gap-1"
      >Station name <input class="rounded border border-gray-700 bg-gray-900 px-2 py-1.5 text-xs text-gray-200" bind:value={editing.station_name} /></label
    >
    <label class="flex flex-col gap-1"
      >Genre <input class="rounded border border-gray-700 bg-gray-900 px-2 py-1.5 text-xs text-gray-200" bind:value={editing.genre} /></label
    >
    <label class="col-span-2 flex flex-col gap-1"
      >URL <input class="rounded border border-gray-700 bg-gray-900 px-2 py-1.5 text-xs text-gray-200" bind:value={editing.url} /></label
    >
  </div>
  <div class="flex gap-2">
    <button class="rounded bg-lime-600 px-3 py-1 text-xs text-white hover:bg-lime-500" onclick={save}
      >{isEditing ? 'Update' : 'Save profile'}</button
    >
    {#if isEditing}
      <button class="rounded bg-gray-800 px-3 py-1 text-xs text-gray-300 hover:bg-gray-700" onclick={reset}
        >Cancel</button
      >
    {/if}
  </div>

  <div class="border-t border-gray-700 pt-3">
    <div class="mb-2 text-xs font-semibold uppercase tracking-wider text-gray-500">Saved</div>
    <div class="space-y-2">
      {#each $profiles as p (p.name)}
        <div class="retro-chrome flex items-center gap-3 rounded-lg px-3 py-2.5">
          <span class="flex h-9 w-9 shrink-0 items-center justify-center rounded-md bg-gradient-to-br from-lime-600/30 to-lime-600/30 text-base">📡</span>
          <div class="min-w-0 flex-1">
            <span class="truncate text-sm font-medium text-gray-100">{p.name}</span>
            <div class="truncate text-[11px] text-gray-500"
              >{p.host}<span class="text-gray-500">:{p.port}</span>{p.mount || '/'}</div
            >
          </div>
          <div class="flex shrink-0 gap-1">
            <button
              class="rounded-md bg-gray-800 px-2.5 py-1.5 text-[11px] font-medium text-lime-300 hover:bg-gray-700"
              onclick={() => edit(p)}>Edit</button
            >
            <button
              class="rounded-md bg-gray-800 px-2.5 py-1.5 text-[11px] font-medium text-red-300 hover:bg-gray-700"
              onclick={() => remove(p.name)}>Delete</button
            >
          </div>
        </div>
      {:else}
        <div class="rounded-lg border border-dashed border-gray-700 px-4 py-6 text-center text-xs text-gray-500">
          No saved profiles — fill the form above.
        </div>
      {/each}
    </div>
  </div>
</div>
