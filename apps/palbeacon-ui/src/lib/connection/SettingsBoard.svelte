<script lang="ts">
  import { onMount } from 'svelte';
  import { toast } from 'svelte-sonner';
  import PalSwitch from '$lib/shared/ui/PalSwitch.svelte';
  import {
    loadNativeAppSettings,
    loadNativeAutostartEnabled,
    updateNativeAppSettings,
    updateNativeAutostartEnabled,
  } from './native';
  import type { NativeAppSettings, NativeAppSettingsDocument } from './types';

  interface Props {
    load?: () => Promise<NativeAppSettingsDocument>;
    save?: (
      expectedVersion: number,
      settings: NativeAppSettings,
    ) => Promise<NativeAppSettingsDocument>;
    loadAutostart?: () => Promise<boolean>;
    setAutostart?: (enabled: boolean) => Promise<void>;
  }

  let {
    load = loadNativeAppSettings,
    save = updateNativeAppSettings,
    loadAutostart = loadNativeAutostartEnabled,
    setAutostart = updateNativeAutostartEnabled,
  }: Props = $props();
  let document = $state<NativeAppSettingsDocument | null>(null);
  let draft = $state<NativeAppSettings | null>(null);
  let autostartEnabled = $state<boolean | null>(null);
  let draftAutostart = $state<boolean | null>(null);
  let loading = $state(true);
  let saving = $state(false);
  let error = $state<string | null>(null);

  const settingsDirty = $derived(
    document !== null &&
      draft !== null &&
      JSON.stringify(document.settings) !== JSON.stringify(draft),
  );
  const autostartDirty = $derived(
    autostartEnabled !== null && draftAutostart !== null && autostartEnabled !== draftAutostart,
  );
  const dirty = $derived(settingsDirty || autostartDirty);

  const refresh = async () => {
    loading = true;
    error = null;
    try {
      const [next, nextAutostart] = await Promise.all([load(), loadAutostart()]);
      document = next;
      draft = { ...next.settings };
      autostartEnabled = nextAutostart;
      draftAutostart = nextAutostart;
    } catch (reason) {
      console.error('설정을 불러오지 못했습니다.', reason);
      error = '설정을 불러오지 못했습니다.';
    } finally {
      loading = false;
    }
  };

  const submit = async () => {
    if (!document || !draft || draftAutostart === null || autostartEnabled === null || !dirty)
      return;
    saving = true;
    error = null;
    let autostartApplied = false;
    try {
      if (autostartDirty) {
        await setAutostart(draftAutostart);
        autostartApplied = true;
      }
      const next = settingsDirty ? await save(document.version, { ...draft }) : document;
      document = next;
      draft = { ...next.settings };
      autostartEnabled = draftAutostart;
      toast.success('설정을 저장했습니다.');
    } catch (reason) {
      if (autostartApplied) {
        await setAutostart(autostartEnabled).catch(() => undefined);
      }
      console.error('설정을 저장하지 못했습니다.', reason);
      error = '설정을 저장하지 못했습니다.';
    } finally {
      saving = false;
    }
  };

  onMount(() => {
    void refresh();
  });
</script>

<section class="settings-screen pal-screen">
  <header class="page-head pal-page-head">
    <div>
      <h1>앱 설정</h1>
    </div>
  </header>

  {#if error}
    <section class="state error" role="alert">
      <p>{error}</p>
      <button type="button" disabled={loading || saving} onclick={() => void refresh()}>
        최신 설정 다시 읽기
      </button>
    </section>
  {/if}

  {#if document && draft && draftAutostart !== null}
    <form
      class="settings-layout"
      onsubmit={(event) => {
        event.preventDefault();
        void submit();
      }}
    >
      <section class="preference-panel">
        <header><h2>시작 동작</h2></header>
        <div class="setting-row">
          <span
            ><strong>Windows 로그인 시 실행</strong><small
              >현재 사용자 시작프로그램에 등록합니다.</small
            ></span
          >
          <PalSwitch bind:checked={draftAutostart} label="Windows 로그인 시 실행" />
        </div>
        <div class="setting-row">
          <span
            ><strong>수동 시작 시 팰월드 실행</strong><small
              >사용자가 앱을 직접 열었을 때만 적용합니다.</small
            ></span
          >
          <PalSwitch
            bind:checked={draft.launch_game_on_manual_start}
            label="수동 시작 시 팰월드 실행"
          />
        </div>
        <div class="setting-row">
          <span
            ><strong>수동 시작 시 앱 창 열기</strong><small
              >백그라운드 실행 대신 관리 화면을 표시합니다.</small
            ></span
          >
          <PalSwitch
            bind:checked={draft.open_app_on_manual_start}
            label="수동 시작 시 앱 창 열기"
          />
        </div>
      </section>

      {#if dirty || saving}
        <footer class="save-bar">
          <p aria-live="polite">{saving ? '저장 중입니다.' : '저장하지 않은 변경이 있습니다.'}</p>
          <button type="submit" disabled={saving}>{saving ? '저장 중' : '설정 저장'}</button>
        </footer>
      {/if}
    </form>
  {:else if loading}
    <section class="state" aria-live="polite">
      <strong>Windows 설정을 읽는 중입니다.</strong>
    </section>
  {/if}
</section>

<style>
  .state p,
  .save-bar p {
    margin: 0;
    color: var(--muted-strong);
    font-size: 0.78rem;
    line-height: 1.7;
  }
  .settings-layout {
    display: grid;
    max-width: 760px;
    grid-template-columns: minmax(0, 1fr);
    gap: 12px;
    margin-top: 12px;
  }
  .preference-panel,
  .state,
  .save-bar {
    border: 1px solid var(--border);
    border-radius: 7px;
    background: var(--ink);
  }
  section > header {
    display: flex;
    min-height: 56px;
    align-items: center;
    justify-content: space-between;
    padding: 12px 14px;
    border-bottom: 1px solid var(--border);
  }
  .setting-row {
    display: grid;
    min-height: 82px;
    grid-template-columns: minmax(0, 1fr) auto;
    align-items: center;
    gap: 20px;
    padding: 14px 16px;
    border-bottom: 1px solid var(--border);
    cursor: pointer;
  }
  .setting-row span {
    display: grid;
    gap: 6px;
  }
  .setting-row strong {
    font-size: 0.86rem;
  }
  .setting-row small {
    color: var(--muted);
    font-size: 0.75rem;
    line-height: 1.45;
  }
  .save-bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 18px;
    padding: 14px 16px;
  }
  button {
    min-height: 42px;
    padding: 0 16px;
    border: 1px solid var(--border-strong);
    border-radius: 5px;
    color: var(--text);
    background: var(--surface-raised);
    cursor: pointer;
    font-weight: 800;
  }
  button[type='submit'] {
    border-color: color-mix(in srgb, var(--accent), transparent 20%);
    background: var(--accent);
  }
  button:disabled {
    cursor: not-allowed;
    opacity: 0.5;
  }
  .state {
    margin-top: 12px;
    padding: 18px;
  }
  .state.error {
    border-color: color-mix(in srgb, var(--danger), transparent 35%);
  }
  .state button {
    margin-top: 12px;
  }
  @media (max-width: 520px) {
    .page-head,
    .save-bar {
      align-items: stretch;
      flex-direction: column;
    }
    .setting-row {
      min-height: 92px;
    }
    .save-bar button {
      width: 100%;
    }
  }
</style>
