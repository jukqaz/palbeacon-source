<script lang="ts">
  import { resolve } from '$app/paths';
  import { inspectNativeStagedSave, rememberNativePersonalSelection } from './native';
  import type { NativeOwnedPalSnapshot, NativeStagedSave } from './types';
  import { normalizeSearchText } from '$lib/shared/search/search-core';
  import { applyOwnedPalSnapshot, clearPersonalPalData } from '$lib/personal/personal-data';

  interface Props {
    staged: NativeStagedSave;
    parserReady: boolean;
    inspect?:
      | ((importId: string, ownerUid: string | null) => Promise<NativeOwnedPalSnapshot>)
      | undefined;
    rememberSelection?: ((importId: string, ownerUid: string) => Promise<void>) | undefined;
  }

  let {
    staged,
    parserReady,
    inspect = inspectNativeStagedSave,
    rememberSelection = rememberNativePersonalSelection,
  }: Props = $props();
  let snapshot = $state<NativeOwnedPalSnapshot | null>(null);
  let loading = $state(false);
  let error = $state<string | null>(null);
  let query = $state('');
  let visibleLimit = $state(24);

  const normalizedQuery = $derived(normalizeSearchText(query));
  const matchingPals = $derived(
    (snapshot?.pals ?? []).filter((pal) => {
      if (!(pal.nickname || pal.species_name_ko)) return false;
      if (!normalizedQuery) return true;
      return [
        pal.species_name_ko ?? '',
        pal.nickname ?? '',
        pal.species_id,
        ...pal.passive_names_ko,
        ...pal.active_skill_names_ko,
      ].some((value) => normalizeSearchText(value).includes(normalizedQuery));
    }),
  );
  const visiblePals = $derived(matchingPals.slice(0, visibleLimit));
  const ownedCatalogHref = `${resolve('/pals/', {})}?mine=1`;
  const ownedPlanHref = `${resolve('/plan/', {})}?mine=1`;

  const runInspection = async (ownerUid: string | null) => {
    loading = true;
    error = null;
    query = '';
    visibleLimit = 24;
    try {
      const next = await inspect(staged.import_id, ownerUid);
      snapshot = next;
      if (next.selected_owner_uid === ownerUid && ownerUid !== null) {
        applyOwnedPalSnapshot(next);
        void rememberSelection(staged.import_id, ownerUid).catch(() => undefined);
      } else {
        clearPersonalPalData();
      }
    } catch (reason) {
      console.error('내 팰 데이터를 해석하지 못했습니다.', reason);
      error = '내 팰 데이터를 불러오지 못했습니다.';
    } finally {
      loading = false;
    }
  };
</script>

{#if parserReady}
  <section class="owned-panel" aria-label="내 팰">
    <div class="owned-head">
      <div>
        <strong>내 팰</strong>
      </div>
      <button type="button" disabled={loading} onclick={() => void runInspection(null)}>
        {loading ? '불러오는 중' : '내 팰 열기'}
      </button>
    </div>

    {#if error}
      <div class="parser-state error" role="alert">
        <strong>내 팰을 읽지 못했습니다.</strong>
        <span>{error}</span>
      </div>
    {:else if snapshot}
      <div class="owner-list" aria-label="플레이어 선택">
        {#each snapshot.players as player (player.uid)}
          <button
            type="button"
            class:active={snapshot.selected_owner_uid === player.uid}
            disabled={loading}
            aria-pressed={snapshot.selected_owner_uid === player.uid}
            onclick={() => void runInspection(player.uid)}
          >
            <strong>{player.nickname || '이름 없는 플레이어'}</strong>
            <span>Lv.{player.level ?? '—'} · 내 팰 {player.pal_count}마리</span>
          </button>
        {/each}
      </div>
      {#if snapshot.selected_owner_uid}
        <nav class="owned-actions" aria-label="내 팰 다음 행동">
          <a class="primary-action" href={ownedCatalogHref}>내 팰 도감</a>
          <a href={ownedPlanHref}>내 팰로 계획</a>
        </nav>
        <label class="pal-search">
          <span>내 팰 검색</span>
          <input
            bind:value={query}
            type="search"
            placeholder="이름, 별명 또는 스킬"
            oninput={() => (visibleLimit = 24)}
          />
        </label>
        <div class="result-count">
          <strong>{matchingPals.length.toLocaleString('ko-KR')}마리</strong>
        </div>
        <div class="pal-list">
          {#each visiblePals as pal (pal.instance_id)}
            <article>
              <div>
                <strong>{pal.nickname || pal.species_name_ko || ''}</strong>
              </div>
              <span>Lv.{pal.level}</span>
              {#if pal.passive_names_ko.length > 0}
                <small>{pal.passive_names_ko.slice(0, 3).join(' · ')}</small>
              {/if}
            </article>
          {/each}
        </div>
        {#if visiblePals.length < matchingPals.length}
          <button class="load-more" type="button" onclick={() => (visibleLimit += 24)}>
            다음 {Math.min(24, matchingPals.length - visiblePals.length).toLocaleString(
              'ko-KR',
            )}마리 보기
          </button>
        {/if}
      {:else}
        <p class="muted">플레이어를 선택하면 해당 캐릭터가 소유한 팰만 표시합니다.</p>
      {/if}
    {/if}
  </section>
{/if}

<style>
  .owned-panel {
    border-top: 1px solid var(--border);
  }
  .owned-head,
  .result-count {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 12px 16px;
  }
  .owned-head > div {
    display: grid;
    gap: 4px;
  }
  .load-more {
    width: calc(100% - 32px);
    margin: 0 16px 16px;
  }
  button,
  input,
  .owned-actions a {
    min-height: 42px;
    border: 1px solid var(--border-strong);
    border-radius: 5px;
    color: var(--text);
    background: var(--surface-raised);
  }
  button {
    padding: 0 14px;
    cursor: pointer;
    font-weight: 800;
  }
  .owned-actions {
    display: flex;
    gap: 8px;
    padding: 12px 16px;
    border-top: 1px solid var(--border);
  }
  .owned-actions a {
    display: inline-flex;
    min-height: 44px;
    align-items: center;
    justify-content: center;
    padding: 0 14px;
    font-size: 0.78rem;
    font-weight: 800;
    text-decoration: none;
  }
  .owned-actions a.primary-action {
    border-color: var(--accent);
    color: var(--void);
    background: var(--accent);
  }
  button:disabled {
    cursor: wait;
    opacity: 0.58;
  }
  button:focus-visible,
  input:focus-visible,
  .owned-actions a:focus-visible {
    outline: 2px solid var(--tech-normal);
    outline-offset: 2px;
  }
  .muted,
  .parser-state {
    margin: 0;
    padding: 14px 16px;
    border-top: 1px solid var(--border);
    color: var(--muted);
    font-size: 0.75rem;
    line-height: 1.55;
  }
  .parser-state {
    display: grid;
    gap: 5px;
  }
  .parser-state.error {
    color: var(--danger);
  }
  .owner-list {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 7px;
    padding: 12px 16px;
    border-top: 1px solid var(--border);
  }
  .owner-list button {
    display: grid;
    height: auto;
    min-width: 0;
    gap: 5px;
    padding: 11px 12px;
    text-align: left;
  }
  .owner-list button.active {
    border-color: var(--tech-normal);
    box-shadow: inset 3px 0 0 var(--tech-normal);
  }
  .owner-list span {
    color: var(--muted);
    font-size: 0.75rem;
  }
  .pal-search {
    display: grid;
    gap: 7px;
    padding: 12px 16px;
    border-top: 1px solid var(--border);
    font-size: 0.75rem;
    font-weight: 800;
  }
  .pal-search input {
    min-width: 0;
    padding: 0 11px;
  }
  .result-count {
    border-top: 1px solid var(--border);
    font-size: 0.75rem;
  }
  .pal-list {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 7px;
    padding: 0 16px 16px;
  }
  .pal-list article {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: 6px 10px;
    min-width: 0;
    padding: 10px 12px;
    border: 1px solid var(--border);
    border-radius: 5px;
    background: color-mix(in srgb, var(--surface-raised), transparent 30%);
  }
  .pal-list article > div {
    display: grid;
    min-width: 0;
    gap: 3px;
  }
  .pal-list article strong {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .pal-list article strong,
  .pal-list article > span {
    font-size: 0.75rem;
  }
  .pal-list article small {
    color: var(--muted);
    font-size: 0.75rem;
  }
  .pal-list article small {
    grid-column: 1 / -1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  @media (max-width: 520px) {
    .owner-list,
    .pal-list {
      grid-template-columns: 1fr;
    }
    .owned-actions {
      display: grid;
      grid-template-columns: 1fr;
    }
  }
</style>
