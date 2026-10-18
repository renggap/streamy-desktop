<script lang="ts">
  import { profiles, errorMsg, notify, view, profileEditing } from '../stores';
  import { listProfiles, saveProfile } from '../api';
  import type { ConnectionProfile } from '../api';
  import { get } from 'svelte/store';

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

  // Seed from the store: profile name to edit, '' = new profile.
  const editingName = $state(get(profileEditing) ?? '');
  const existing = get(profiles).find((p) => p.name === editingName);
  let editing = $state<ConnectionProfile>(existing ? { ...existing } : blank());
  let password = $state('');
  let showPw = $state(false);
  const isEditing = $derived(editingName !== '');

  function close() {
    profileEditing.set(null);
    view.set('settings');
  }

  async function save() {
    if (!editing.name || !editing.host) {
      errorMsg.set('Name and host are required.');
      notify('error', 'Missing fields', 'Name and host are required.');
      return;
    }
    if (!isEditing && !password) {
      errorMsg.set('Password is required for a new profile.');
      notify('error', 'Password required', 'New profiles need a password.');
      return;
    }
    // Blank password on edit -> null -> keep the existing stored password.
    const pw = password.length > 0 ? password : null;
    try {
      await saveProfile({ ...editing }, pw);
      profiles.set(await listProfiles());
      notify('success', 'Profile saved', editing.name);
      close();
    } catch (e) {
      errorMsg.set(String(e));
    }
  }
</script>

<div class="space-y-3">
  <div class="flex items-center justify-between">
    <h3 class="joko-label">{isEditing ? 'Edit profile' : 'New profile'}</h3>
    <button
      class="rounded-lg bg-gray-800 px-3 py-1.5 text-xs font-semibold text-gray-200 shadow-sm transition-colors hover:bg-gray-700"
      onclick={close}>Back</button
    >
  </div>
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
      Streamy auto-detects the protocol on connect — it tries SHOUTcast v1 first (works on DNAS1
      and DNAS2), then Ultravox 2.1 (DNASv2 DJ login), then Icecast / Liquidsoap harbor. <span
        class="text-gray-300">Stream ID</span> is used for
      Ultravox; <span class="text-gray-300">Mount</span> for Icecast. <span class="text-gray-300"
        >Username</span
      > is the Icecast source/DJ login, or the optional Ultravox DJ id (leave blank for Ultravox
      source login).
    </div>
    <label class="col-span-2 flex items-center gap-2 text-xs text-gray-300">
      <input type="checkbox" class="h-4 w-4 accent-lime-500" bind:checked={editing.azuracast} />
      AzuraCast server
      <span class="text-[10px] text-gray-400"
        >— DJ/source connects via Icecast even if it relays to Shoutcast DNAS for listeners. Skips
        protocol probing so DJ login is immediate.</span
      >
    </label>
    <label class="col-span-2 flex flex-col gap-1"
      >Password
      <div class="relative">
        <input
          type={showPw ? 'text' : 'password'}
          class="w-full rounded-lg border border-gray-600 bg-gray-900 px-2 py-1.5 pr-9 text-xs text-gray-100 focus:border-lime-500"
          bind:value={password}
          placeholder={isEditing ? '(unchanged)' : ''}
          autocomplete="new-password"
        />
        <button
          type="button"
          class="absolute inset-y-0 right-0 flex w-9 items-center justify-center text-gray-400 hover:text-gray-200"
          aria-label={showPw ? 'Hide password' : 'Show password'}
          title={showPw ? 'Hide password' : 'Show password'}
          onclick={() => (showPw = !showPw)}>
          {#if showPw}
            <svg
              xmlns="http://www.w3.org/2000/svg"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              stroke-width="2"
              stroke-linecap="round"
              stroke-linejoin="round"
              class="h-4 w-4"
            >
              <path d="M17.94 17.94A10.07 10.07 0 0 1 12 20c-7 0-11-8-11-8a18.45 18.45 0 0 1 5.06-5.94" />
              <path d="M9.9 4.24A9.12 9.12 0 0 1 12 4c7 0 11 8 11 8a18.5 18.5 0 0 1-2.16 3.19" />
              <path d="M14.12 14.12a3 3 0 1 1-4.24-4.24" />
              <path d="m1 1 22 22" />
            </svg>
          {:else}
            <svg
              xmlns="http://www.w3.org/2000/svg"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              stroke-width="2"
              stroke-linecap="round"
              stroke-linejoin="round"
              class="h-4 w-4"
            >
              <path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z" />
              <circle cx="12" cy="12" r="3" />
            </svg>
          {/if}
        </button>
      </div></label
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
  </div>
</div>
