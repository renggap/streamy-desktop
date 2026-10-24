<script lang="ts">
  import { profiles, errorMsg } from '../stores';
  import { notify } from '../stores';
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
      notify('error', 'Missing fields', 'Name and host are required.');
      return;
    }
    if (!exists && !password) {
      errorMsg.set('Password is required for a new profile.');
      notify('error', 'Password required', 'New profiles need a password.');
      return;
    }
    // Blank password on edit -> null -> keep the existing stored password.
    const pw = password.length > 0 ? password : null;
    try {
      await saveProfile({ ...editing }, pw);
      await refresh();
      reset();
      notify('success', 'Profile saved', editing.name);
    } catch (e) {
      errorMsg.set(String(e));
    }
  }

  async function remove(name: string) {
    try {
      await deleteProfile(name);
      await refresh();
      if (isEditing && editing.name === name) reset();
      notify('info', 'Profile deleted', name);
    } catch (e) {
      errorMsg.set(String(e));
    }
  }
</script>

<div class="space-y-3">
  <h3 class="joko-label">
    {isEditing ? 'Edit profile' : 'New profile'}
  </h3>
  <div class="grid grid-cols-2 gap-2 text-xs text-gray-300">
    <label class="flex flex-col gap-1"
      >Name <input class="rounded-lg border border-gray-600 bg-gray-900 px-2 py-1.5 text-xs text-gray-100 focus:border-lime-500" bind:value={editing.name} disabled={isEditing} /></label
    >
    <label class="flex flex-col gap-1"
      >Host <input class="rounded-lg border border-gray-600 bg-gray-900 px-2 py-1.5 text-xs text-gray-100 focus:border-lime-500" bind:value={editing.host} /></label
    >
    <label class="flex flex-col gap-1"
      >Port <input type="number" class="rounded-lg border border-gray-600 bg-gray-900 px-2 py-1.5 text-xs text-gray-100 focus:border-lime-500" bind:value={editing.port} /></label
    >
    <label class="flex flex-col gap-1"
      >Stream ID <input type="number" class="rounded-lg border border-gray-600 bg-gray-900 px-2 py-1.5 text-xs text-gray-100 focus:border-lime-500" bind:value={editing.stream_id} /></label
    >
    <label class="flex flex-col gap-1"
      >Mount
      <input
        class="rounded-lg border border-gray-600 bg-gray-900 px-2 py-1.5 text-xs text-gray-100 focus:border-lime-500"
        bind:value={editing.mount}
        placeholder="/"
      /></label
    >
    <label class="flex flex-col gap-1"
      >Username
      <input
        class="rounded-lg border border-gray-600 bg-gray-900 px-2 py-1.5 text-xs text-gray-100 focus:border-lime-500"
        bind:value={editing.user}
        placeholder="(Icecast source/DJ login)"
      /></label
    >
    <div class="col-span-2 text-[10px] leading-relaxed text-gray-400">
      Streamy auto-detects the protocol on connect — it tries Ultravox 2.1 (DNASv2) first, then falls
      back to Icecast / Liquidsoap harbor. <span class="text-gray-300">Stream ID</span> is used for
      Ultravox; <span class="text-gray-300">Mount</span> for Icecast. <span class="text-gray-300"
        >Username</span
      > is the Icecast source/DJ login, or the optional Ultravox DJ id (leave blank for Ultravox
      source login).
    </div>
    <label class="col-span-2 flex items-center gap-2 text-xs text-gray-300">
      <input type="checkbox" class="h-4 w-4 accent-lime-500" bind:checked={editing.azuracast} />
      AzuraCast server
      <span class="text-[10px] text-gray-400"
        >— DJ/source connects via Icecast even if it relays to Shoutcast DNAS for listeners. Skips the
        Ultravox probe so DJ login is immediate.</span
      >
    </label>
    <label class="col-span-2 flex flex-col gap-1"
      >Password
      <input
        type="password"
        class="rounded-lg border border-gray-600 bg-gray-900 px-2 py-1.5 text-xs text-gray-100 focus:border-lime-500"
        bind:value={password}
        placeholder={isEditing ? '(unchanged)' : ''}
      /></label
    >
    <label class="flex flex-col gap-1"
      >Station name <input class="rounded-lg border border-gray-600 bg-gray-900 px-2 py-1.5 text-xs text-gray-100 focus:border-lime-500" bind:value={editing.station_name} /></label
    >
    <label class="flex flex-col gap-1"
      >Genre <input class="rounded-lg border border-gray-600 bg-gray-900 px-2 py-1.5 text-xs text-gray-100 focus:border-lime-500" bind:value={editing.genre} /></label
    >
    <label class="col-span-2 flex flex-col gap-1"
      >URL <input class="rounded-lg border border-gray-600 bg-gray-900 px-2 py-1.5 text-xs text-gray-100 focus:border-lime-500" bind:value={editing.url} /></label
    >
  </div>
  <div class="flex gap-2">
    <button
      class="rounded-lg bg-lime-600 px-3 py-1.5 text-xs font-semibold text-white shadow-md transition-all duration-200 hover:bg-lime-700 active:scale-95"
      onclick={save}
      >{isEditing ? 'Update' : 'Save profile'}</button
    >
    {#if isEditing}
      <button
        class="rounded-lg bg-gray-800 px-3 py-1.5 text-xs font-semibold text-gray-200 shadow-sm transition-colors hover:bg-gray-700"
        onclick={reset}
        >Cancel</button
      >
    {/if}
  </div>

  <div class="border-t border-gray-600 pt-3">
    <div class="joko-label mb-2">Saved</div>
    <div class="space-y-2">
      {#each $profiles as p (p.name)}
        <div class="flex items-center gap-3 rounded-xl border border-gray-600 bg-gray-800 px-3 py-2.5 shadow-sm">
          <span class="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg bg-lime-900/40 text-base">📡</span>
          <div class="min-w-0 flex-1">
            <span class="truncate text-sm font-medium text-gray-100">{p.name}</span>
            <div class="truncate text-[11px] text-gray-400"
              >{p.host}<span class="text-gray-400">:{p.port}</span>{p.mount || '/'}</div
            >
          </div>
          <div class="flex shrink-0 gap-1">
            <button
              class="rounded-lg bg-gray-800 px-2.5 py-1.5 text-[11px] font-medium text-gray-200 transition-colors hover:bg-gray-700"
              onclick={() => edit(p)}>Edit</button
            >
            <button
              class="rounded-lg bg-red-900/30 px-2.5 py-1.5 text-[11px] font-medium text-red-400 transition-colors hover:bg-red-900/50"
              onclick={() => remove(p.name)}>Delete</button
            >
          </div>
        </div>
      {:else}
        <div class="rounded-xl border border-dashed border-gray-500 px-4 py-6 text-center text-xs text-gray-400">
          No saved profiles — fill the form above.
        </div>
      {/each}
    </div>
  </div>
</div>
