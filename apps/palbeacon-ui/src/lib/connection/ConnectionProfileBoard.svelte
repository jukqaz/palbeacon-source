<script lang="ts">
  import { onMount } from 'svelte';
  import OwnedPalPanel from './OwnedPalPanel.svelte';
  import {
    loadNativeProfileSnapshot,
    probeNativeSaveSource,
    selectNativeSaveSourceDirectory,
    stageNativeSaveSource,
  } from './native';
  import type {
    NativeOwnedPalSnapshot,
    NativeProfileSnapshot,
    NativeSaveSourceProbe,
    NativeStagedSave,
  } from './types';

  interface Props {
    load?: () => Promise<NativeProfileSnapshot>;
    selectSource?: () => Promise<string | null>;
    probeSource?: (sourcePath: string) => Promise<NativeSaveSourceProbe>;
    stageSource?: (sourcePath: string) => Promise<NativeStagedSave>;
    inspectSave?: (importId: string, ownerUid: string | null) => Promise<NativeOwnedPalSnapshot>;
  }

  let {
    load = loadNativeProfileSnapshot,
    selectSource = selectNativeSaveSourceDirectory,
    probeSource = probeNativeSaveSource,
    stageSource = stageNativeSaveSource,
    inspectSave,
  }: Props = $props();
  let snapshot = $state<NativeProfileSnapshot | null>(null);
  let error = $state<string | null>(null);
  let loading = $state(true);
  let sourcePath = $state('');
  let sourceProbe = $state<NativeSaveSourceProbe | null>(null);
  let saveActionError = $state<string | null>(null);
  let saveActionMessage = $state<string | null>(null);
  let selecting = $state(false);
  let probing = $state(false);
  let staging = $state(false);

  const stageCandidate = $derived(
    sourceProbe?.candidates.length === 1 &&
      (sourceProbe.status === 'ready' || sourceProbe.status === 'ready_without_players')
      ? sourceProbe.candidates[0]
      : null,
  );

  const refresh = async () => {
    loading = true;
    error = null;
    try {
      snapshot = await load();
    } catch (reason) {
      console.error('Windows 상태를 불러오지 못했습니다.', reason);
      error = 'Windows 상태를 불러오지 못했습니다.';
    } finally {
      loading = false;
    }
  };

  const formatBytes = (value: number): string => {
    const units = [
      { threshold: 1024 ** 3, label: 'GB' },
      { threshold: 1024 ** 2, label: 'MB' },
      { threshold: 1024, label: 'KB' },
    ];
    const unit = units.find(({ threshold }) => value >= threshold);
    if (!unit) return `${new Intl.NumberFormat('ko-KR').format(value)} B`;
    return `${new Intl.NumberFormat('ko-KR', { maximumFractionDigits: 1 }).format(value / unit.threshold)} ${unit.label}`;
  };

  const probeStatusLabel = (probe: NativeSaveSourceProbe): string => {
    switch (probe.status) {
      case 'ready':
        return '보호 복사 가능';
      case 'ready_without_players':
        return '월드만 복사 가능';
      case 'multiple_save_worlds':
        return '월드를 하나로 좁혀 주세요';
      case 'no_save_world':
        return '월드 세이브를 찾지 못함';
    }
  };

  const inspectSource = async () => {
    const trimmedPath = sourcePath.trim();
    if (!trimmedPath) return;
    probing = true;
    sourceProbe = null;
    saveActionError = null;
    saveActionMessage = null;
    try {
      sourceProbe = await probeSource(trimmedPath);
    } catch (reason) {
      console.error('세이브 폴더를 검사하지 못했습니다.', reason);
      saveActionError = '세이브 폴더를 검사하지 못했습니다.';
    } finally {
      probing = false;
    }
  };

  const chooseSource = async () => {
    selecting = true;
    saveActionError = null;
    saveActionMessage = null;
    try {
      const selected = await selectSource();
      if (selected === null) return;
      sourcePath = selected;
      await inspectSource();
    } catch (reason) {
      console.error('세이브 폴더를 선택하지 못했습니다.', reason);
      saveActionError = '세이브 폴더를 선택하지 못했습니다.';
    } finally {
      selecting = false;
    }
  };

  const stageProtectedCopy = async () => {
    if (!stageCandidate) return;
    staging = true;
    saveActionError = null;
    saveActionMessage = null;
    try {
      const staged = await stageSource(stageCandidate.world_root);
      if (snapshot) {
        snapshot.save.latest = staged;
        snapshot.save.message_ko = snapshot.save.parser_ready
          ? '보호 복사본과 읽기 전용 내 팰 해석기가 준비됐습니다.'
          : '보호 복사본은 준비됐지만 내 팰 해석기 파일이 없습니다.';
      }
      saveActionMessage = '원본을 변경하지 않고 보호 복사본을 만들었습니다.';
    } catch (reason) {
      console.error('세이브 보호 복사를 만들지 못했습니다.', reason);
      saveActionError = '세이브 보호 복사를 만들지 못했습니다.';
    } finally {
      staging = false;
    }
  };

  onMount(() => {
    void refresh();
  });
</script>

<section class="connection-screen pal-screen">
  <header class="page-head pal-page-head">
    <div>
      <h1>내 데이터</h1>
    </div>
    <button type="button" disabled={loading} onclick={() => void refresh()}>
      {loading ? '확인 중' : '새로고침'}
    </button>
  </header>

  {#if error}
    <section class="state error" role="alert">
      <strong>Windows 연결을 확인하지 못했습니다.</strong>
      <p>{error}</p>
      <button type="button" onclick={() => void refresh()}>다시 확인</button>
    </section>
  {:else if snapshot}
    <div class="connection-layout">
      <section class="save-panel">
        <header><strong>내 세이브</strong></header>
        <div class="save-import">
          <button
            type="button"
            disabled={selecting || probing || staging}
            onclick={() => void chooseSource()}
          >
            {selecting || probing ? '세이브 찾는 중' : '세이브 찾기'}
          </button>
        </div>

        {#if saveActionError}
          <div class="inline-state error" role="alert">{saveActionError}</div>
        {/if}
        {#if saveActionMessage}
          <div class="inline-state success" role="status">{saveActionMessage}</div>
        {/if}

        {#if sourceProbe}
          <section class="probe-result" aria-label="세이브 폴더 검사 결과">
            <div class="probe-head">
              <div>
                <small>검사 결과</small>
                <strong>{probeStatusLabel(sourceProbe)}</strong>
              </div>
              <button
                type="button"
                disabled={!stageCandidate || staging || probing}
                onclick={() => void stageProtectedCopy()}
              >
                {staging ? '복사 중' : '보호 복사 만들기'}
              </button>
            </div>
            {#each sourceProbe.candidates as candidate, candidateIndex (candidate.world_root)}
              <article class="candidate">
                <strong>세이브 {candidateIndex + 1}</strong>
                <span
                  >플레이어 {candidate.player_file_count.toLocaleString('ko-KR')}명 · {formatBytes(
                    candidate.total_bytes,
                  )}</span
                >
              </article>
            {/each}
          </section>
        {/if}
        {#if snapshot.save.latest}
          <dl>
            <div>
              <dt>플레이어 파일</dt>
              <dd>{snapshot.save.latest.player_file_count}개</dd>
            </div>
            <div>
              <dt>복사 크기</dt>
              <dd>{formatBytes(snapshot.save.latest.total_bytes)}</dd>
            </div>
          </dl>
          <OwnedPalPanel
            staged={snapshot.save.latest}
            parserReady={snapshot.save.parser_ready}
            inspect={inspectSave}
          />
        {:else}
          <div class="empty">
            <strong>가져온 보호 복사본 없음</strong>
            <p>원본 세이브를 직접 수정하지 않습니다.</p>
          </div>
        {/if}
      </section>
    </div>
  {:else}
    <section class="state" aria-live="polite">
      <strong>Windows 상태를 읽는 중입니다.</strong>
    </section>
  {/if}
</section>

<style>
  section > p {
    margin: 0;
    color: var(--muted-strong);
    font-size: 0.82rem;
    line-height: 1.7;
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
  button:disabled {
    cursor: wait;
    opacity: 0.6;
  }
  .connection-layout {
    display: grid;
    max-width: 920px;
    grid-template-columns: minmax(0, 1fr);
    gap: 12px;
    margin-top: 12px;
  }
  .save-panel,
  .state {
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
  dl {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    margin: 0;
  }
  dl div {
    min-width: 0;
    padding: 13px 16px;
    border-right: 1px solid var(--border);
    border-bottom: 1px solid var(--border);
  }
  dt {
    color: var(--muted);
    font-size: 0.75rem;
  }
  dd {
    margin: 5px 0 0;
    overflow-wrap: anywhere;
    font-size: 0.78rem;
    font-weight: 800;
  }
  .save-import {
    display: grid;
    gap: 8px;
    padding: 14px 16px;
    border-bottom: 1px solid var(--border);
  }
  .candidate span {
    color: var(--muted);
    font-size: 0.75rem;
    line-height: 1.5;
  }
  button:focus-visible {
    outline: 2px solid var(--tech-normal);
    outline-offset: 2px;
  }
  .inline-state {
    padding: 10px 16px;
    border-bottom: 1px solid var(--border);
    font-size: 0.75rem;
    font-weight: 750;
  }
  .inline-state.error {
    color: var(--danger);
  }
  .inline-state.success {
    color: var(--success);
  }
  .probe-result {
    border-bottom: 1px solid var(--border);
  }
  .probe-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 12px 16px;
  }
  .probe-head > div {
    display: grid;
    gap: 4px;
  }
  .probe-head small {
    color: var(--muted);
    font-size: 0.75rem;
    text-transform: uppercase;
  }
  .candidate {
    display: grid;
    gap: 5px;
    padding: 12px 16px;
    border-top: 1px solid var(--border);
    background: color-mix(in srgb, var(--surface-raised), transparent 42%);
  }
  .candidate strong {
    font-size: 0.78rem;
  }
  .empty,
  .state {
    padding: 24px;
  }
  .empty p,
  .state p {
    color: var(--muted);
    font-size: 0.75rem;
  }
  .state.error {
    margin-top: 12px;
    border-color: color-mix(in srgb, var(--danger), transparent 35%);
  }
  .state button {
    margin-top: 12px;
  }
  @media (max-width: 520px) {
    .page-head {
      align-items: stretch;
      flex-direction: column;
    }
    dl {
      grid-template-columns: 1fr;
    }
    .probe-head {
      align-items: stretch;
      flex-direction: column;
    }
  }
</style>
