<script lang="ts">
  import { tick } from 'svelte';
  import { resolve } from '$app/paths';
  import { rememberWorkspaceEntry } from '$lib/shared/workspace/workspace';
  import { filterTechnologies, groupTechnologiesByLevel } from './catalog';
  import TechnologyCard from './TechnologyCard.svelte';
  import TechnologyInspector from './TechnologyInspector.svelte';
  import type { TechnologyCatalog, TechnologyLane, TechnologyRecord } from './types';
  import { normalizeSearchText } from '$lib/shared/search/search-core';

  interface Props {
    catalog: TechnologyCatalog;
    initialQuery?: string;
    initialSelectedId?: string;
    initialLane?: TechnologyLane | 'all';
    initialLevel?: number;
  }

  let {
    catalog,
    initialQuery = '',
    initialSelectedId = '',
    initialLane = 'all',
    initialLevel = 10,
  }: Props = $props();
  let query = $state('');
  let lane = $state<TechnologyLane | 'all'>('all');
  let selectedLevel = $state(10);
  let selectedId = $state<string | null>(null);
  let levelScroll = $state<HTMLDivElement>();
  let selectionTrigger: HTMLElement | null = null;
  let appliedInitialState = $state<string | null>(null);
  const visibleTechnologies = $derived(
    catalog.technologies.filter(
      (technology) => !technology.localization_fallback && technology.name_ko.trim().length > 0,
    ),
  );

  const exactTechnologyForEntry = (displayQuery: string, entityId: string) => {
    const normalizedId = normalizeSearchText(entityId);
    if (normalizedId.length > 0) {
      const idMatch = visibleTechnologies.find(
        (technology) => normalizeSearchText(technology.id) === normalizedId,
      );
      if (idMatch) return idMatch;
    }

    const normalizedQuery = normalizeSearchText(displayQuery);
    if (normalizedQuery.length === 0) return null;
    const idMatch = visibleTechnologies.find(
      (technology) => normalizeSearchText(technology.id) === normalizedQuery,
    );
    if (idMatch) return idMatch;
    const nameMatches = visibleTechnologies.filter(
      (technology) => normalizeSearchText(technology.name_ko) === normalizedQuery,
    );
    return nameMatches.length === 1 ? nameMatches[0] : null;
  };

  $effect(() => {
    const displayQuery = initialQuery.trim();
    const entityId = initialSelectedId.trim();
    const signature = `${displayQuery}\u0000${entityId}\u0000${initialLane}\u0000${initialLevel.toString()}`;
    if (signature === appliedInitialState) return;
    appliedInitialState = signature;
    query = displayQuery;
    lane = initialLane;
    selectedLevel = initialLevel;
    const exact = exactTechnologyForEntry(displayQuery, entityId);
    selectedId = exact && (initialLane === 'all' || exact.lane === initialLane) ? exact.id : null;
  });

  const filtered = $derived(filterTechnologies(catalog, query, lane));
  const laneGroups = $derived(groupTechnologiesByLevel(filterTechnologies(catalog, '', lane)));
  const matchingGroups = $derived(groupTechnologiesByLevel(filtered));
  const selectedLevelIndex = $derived(
    laneGroups.findIndex((group) => group.level === selectedLevel),
  );
  const groups = $derived.by(() => {
    if (query.trim().length > 0) return matchingGroups;
    if (selectedLevelIndex < 0) return [];
    return laneGroups.slice(selectedLevelIndex, selectedLevelIndex + 4);
  });
  const selected = $derived(
    selectedId === null
      ? null
      : (visibleTechnologies.find((technology) => technology.id === selectedId) ?? null),
  );
  const resultSummary = $derived(
    `${filtered.length.toString()}개 일치 · ${groups.length.toString()}개 레벨`,
  );

  $effect(() => {
    const availableGroups = laneGroups;
    const currentLevel = selectedLevel;
    if (
      availableGroups.length === 0 ||
      availableGroups.some((group) => group.level === currentLevel)
    ) {
      return;
    }
    const nearest = availableGroups.reduce((candidate, group) =>
      Math.abs(group.level - currentLevel) < Math.abs(candidate.level - currentLevel)
        ? group
        : candidate,
    );
    selectedLevel = nearest.level;
    selectedId = null;
  });

  $effect(() => {
    const current = selectedId;
    if (current !== null && !filtered.some((technology) => technology.id === current)) {
      selectedId = null;
    }
  });

  const selectTechnology = async (technology: TechnologyRecord, trigger: HTMLButtonElement) => {
    selectionTrigger = trigger;
    selectedId = technology.id;
    const parameters = new URLSearchParams({ q: technology.name_ko, id: technology.id });
    rememberWorkspaceEntry({
      kind: 'technology',
      id: technology.id,
      name_ko: technology.name_ko,
      href: `${resolve('/technology/', {})}?${parameters.toString()}`,
      image_path: technology.icon_path,
    });
    await tick();
    if (window.matchMedia('(max-width: 1023px)').matches) {
      document.querySelector<HTMLElement>('#technology-detail')?.focus();
    }
  };

  const closeDetail = async () => {
    selectedId = null;
    await tick();
    selectionTrigger?.focus();
    selectionTrigger = null;
  };

  const selectLane = (nextLane: TechnologyLane | 'all') => {
    lane = nextLane;
    selectedId = null;
  };

  const selectLevel = (level: number) => {
    selectedLevel = level;
    selectedId = null;
  };

  const selectAdjacentLevel = (offset: number) => {
    const target = laneGroups[selectedLevelIndex + offset];
    if (target) selectLevel(target.level);
  };

  $effect(() => {
    const currentLevel = selectedLevel;
    const currentLane = lane;
    queueMicrotask(() => {
      if (selectedLevel !== currentLevel || lane !== currentLane) return;
      levelScroll
        ?.querySelector<HTMLElement>('[aria-pressed="true"]')
        ?.scrollIntoView({ block: 'nearest', inline: 'center' });
    });
  });
</script>

<section class="technology-screen" class:detail-open={selected !== null}>
  <header class="page-head">
    <h1>기술 도감</h1>
    <div class="lane-filter" role="group" aria-label="기술 분류">
      {#each [{ id: 'all', label: '전체' }, { id: 'normal', label: '일반 기술' }, { id: 'ancient', label: '고대 기술' }] as option (option.id)}
        <button
          type="button"
          class:active={lane === option.id}
          aria-pressed={lane === option.id}
          onclick={() => selectLane(option.id as TechnologyLane | 'all')}>{option.label}</button
        >
      {/each}
    </div>
  </header>

  <div class="toolbar">
    <label class="search-field">
      <span class="visually-hidden">기술·해금 항목 검색</span>
      <input
        type="search"
        bind:value={query}
        aria-label="기술·해금 항목 검색"
        placeholder="한글 표시명 또는 해금 항목"
        autocomplete="off"
      />
      {#if query.length > 0}
        <button type="button" aria-label="검색어 지우기" onclick={() => (query = '')}>지우기</button
        >
      {/if}
    </label>
    {#if query.trim().length > 0}
      <span class="result-summary" aria-live="polite">{resultSummary}</span>
    {/if}
  </div>

  {#if laneGroups.length > 0 && query.trim().length === 0}
    <nav class="level-jump" aria-label="기술 레벨 선택">
      <span>레벨</span>
      <button
        class="level-step"
        type="button"
        aria-label="이전 기술 레벨"
        disabled={selectedLevelIndex <= 0}
        onclick={() => selectAdjacentLevel(-1)}>이전</button
      >
      <div bind:this={levelScroll} class="level-scroll">
        {#each laneGroups as group (group.level)}
          <button
            type="button"
            class:active={selectedLevel === group.level}
            aria-pressed={selectedLevel === group.level}
            aria-label={`레벨 ${group.level.toString()}`}
            onclick={() => selectLevel(group.level)}>{group.level}</button
          >
        {/each}
      </div>
      <button
        class="level-step"
        type="button"
        aria-label="다음 기술 레벨"
        disabled={selectedLevelIndex < 0 || selectedLevelIndex >= laneGroups.length - 1}
        onclick={() => selectAdjacentLevel(1)}>다음</button
      >
    </nav>
  {/if}

  <div class="board-layout" class:with-inspector={selected !== null}>
    <div class="level-list lane-{lane}" aria-label="기술 레벨 목록">
      {#if groups.length === 0}
        <div class="empty-state">
          <h2>일치하는 기술이 없습니다.</h2>
          <p>검색어를 줄이거나 기술 분류를 전체로 바꿔 보세요.</p>
          <button
            type="button"
            onclick={() => {
              query = '';
              lane = 'all';
              selectedLevel = initialLevel;
            }}>검색과 필터 초기화</button
          >
        </div>
      {:else}
        <div class="lane-columns lane-{lane}" aria-hidden="true">
          <span class="level-column-label">레벨</span>
          {#if lane !== 'ancient'}
            <span class="normal-column-label">일반 기술</span>
          {/if}
          {#if lane !== 'normal'}
            <span class="ancient-column-label">고대 기술</span>
          {/if}
        </div>
        {#each groups as group (group.level)}
          <section
            class="level-band lane-{lane}"
            id={`technology-level-${group.level.toString()}`}
            aria-label={`레벨 ${group.level.toString()} 기술`}
          >
            <header class="level-heading">
              <strong>{group.level}</strong>
            </header>

            {#if lane !== 'ancient'}
              <div
                class="lane-block normal"
                class:empty-lane={group.normal.length === 0}
                aria-label={group.normal.length > 0
                  ? `레벨 ${group.level.toString()} 일반 기술`
                  : undefined}
                aria-hidden={group.normal.length === 0}
              >
                <div class="card-grid">
                  {#each group.normal as technology (technology.id)}
                    <TechnologyCard
                      {technology}
                      selected={selectedId === technology.id}
                      onselect={selectTechnology}
                    />
                  {/each}
                </div>
              </div>
            {/if}

            {#if lane !== 'normal'}
              <div
                class="lane-block ancient"
                class:empty-lane={group.ancient.length === 0}
                data-testid="ancient-lane"
                aria-label={group.ancient.length > 0
                  ? `레벨 ${group.level.toString()} 고대 기술`
                  : undefined}
                aria-hidden={group.ancient.length === 0}
              >
                <div class="card-grid ancient-grid">
                  {#each group.ancient as technology (technology.id)}
                    <TechnologyCard
                      {technology}
                      selected={selectedId === technology.id}
                      onselect={selectTechnology}
                    />
                  {/each}
                </div>
              </div>
            {/if}
          </section>
        {/each}
      {/if}
    </div>

    {#if selected}
      <TechnologyInspector technology={selected} onclose={closeDetail} />
    {/if}
  </div>
</section>

<style>
  .technology-screen {
    container-name: technology;
    container-type: inline-size;
    padding: 22px clamp(14px, 2.2vw, 34px) 42px;
  }

  .page-head {
    display: flex;
    min-height: 58px;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    padding: 0 16px;
    border: 1px solid var(--border);
    border-radius: 8px 8px 0 0;
    background: var(--ink);
  }

  .page-head h1 {
    margin: 0;
    font-size: 1.2rem;
  }

  h1 {
    margin: 8px 0 7px;
    font-size: clamp(1.75rem, 3.2vw, 2.65rem);
    line-height: 1;
    letter-spacing: -0.04em;
  }

  .toolbar {
    display: grid;
    grid-template-columns: minmax(260px, 1fr) auto;
    gap: 12px;
    align-items: center;
    padding: 10px 14px;
    border: 1px solid var(--border);
    border-top: 0;
    background: var(--surface);
  }

  .search-field {
    position: relative;
    display: grid;
    gap: 6px;
  }

  .visually-hidden {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
    clip-path: inset(50%);
    white-space: nowrap;
  }

  .search-field > span {
    color: var(--muted-strong);
    font-size: 0.75rem;
    font-weight: 720;
  }

  input {
    width: 100%;
    min-height: 42px;
    padding: 9px 72px 9px 12px;
    border: 1px solid var(--border-strong);
    border-radius: 5px;
    color: var(--text);
    background: var(--ink);
    font-size: 0.82rem;
  }

  input::placeholder {
    color: var(--muted);
  }

  .search-field button {
    position: absolute;
    right: 6px;
    bottom: 6px;
    min-height: 30px;
    padding-inline: 9px;
    border: 1px solid var(--border);
    border-radius: 4px;
    color: var(--muted-strong);
    background: var(--surface-raised);
    cursor: pointer;
    font-size: 0.75rem;
  }

  .lane-filter {
    display: flex;
    min-width: 0;
    gap: 5px;
    overflow-x: auto;
    scrollbar-width: none;
  }

  .lane-filter::-webkit-scrollbar,
  .level-scroll::-webkit-scrollbar {
    display: none;
  }

  .lane-filter button,
  .level-jump button,
  .empty-state button {
    min-height: 42px;
    padding-inline: 12px;
    border: 1px solid var(--border);
    border-radius: 5px;
    color: var(--muted-strong);
    background: var(--ink);
    cursor: pointer;
    font-size: 0.75rem;
    font-weight: 720;
  }

  .lane-filter button:hover,
  .lane-filter button.active {
    border-color: var(--accent);
    color: var(--text);
    background: rgb(0 215 233 / 13%);
  }

  .result-summary {
    align-self: center;
    color: var(--muted-strong);
    font-size: 0.75rem;
    white-space: nowrap;
  }

  .level-jump {
    position: sticky;
    z-index: 20;
    top: 64px;
    display: grid;
    grid-template-columns: auto auto minmax(0, 1fr) auto;
    align-items: center;
    gap: 12px;
    padding: 8px 10px;
    border: 1px solid var(--border);
    border-top: 0;
    background: var(--ink);
  }

  .level-jump > span {
    color: var(--tech-normal);
    font-size: 0.75rem;
    font-weight: 850;
    letter-spacing: 0.12em;
  }

  .level-scroll {
    display: flex;
    min-width: 0;
    gap: 5px;
    overflow-x: auto;
    scrollbar-width: none;
  }

  .level-jump button {
    min-width: 38px;
    min-height: 32px;
    padding-inline: 8px;
    flex: 0 0 auto;
    font-size: 0.75rem;
  }

  .level-jump button.active {
    border-color: var(--tech-normal);
    color: var(--void);
    background: var(--tech-normal);
  }

  .level-jump button:disabled {
    opacity: 0.38;
    cursor: not-allowed;
  }

  .level-step {
    min-width: 54px !important;
  }

  .board-layout {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    gap: 16px;
    align-items: start;
    margin-top: 12px;
  }

  .board-layout.with-inspector {
    grid-template-columns: minmax(0, 1fr) minmax(300px, 360px);
  }

  .level-list {
    display: grid;
    min-width: 0;
    max-height: calc(100dvh - 286px);
    grid-auto-rows: max-content;
    gap: 0;
    overflow: auto;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--ink);
    scrollbar-gutter: stable;
  }

  .lane-columns,
  .level-band {
    --level-column: 74px;
    --ancient-column: 172px;
    scroll-margin-top: 112px;
    display: grid;
    grid-template-columns: var(--level-column) minmax(0, 1fr) var(--ancient-column);
  }

  .lane-columns {
    position: sticky;
    z-index: 3;
    top: 0;
    min-height: 34px;
    color: var(--text);
    background: var(--ink);
  }

  .lane-columns > span {
    display: flex;
    align-items: center;
    min-width: 0;
    padding: 7px 12px;
    border-bottom: 1px solid currentcolor;
    font-size: 0.75rem;
    font-weight: 850;
  }

  .level-column-label {
    justify-content: center;
    border-right: 1px solid var(--border);
    color: var(--muted-strong);
  }

  .normal-column-label {
    color: var(--tech-normal);
    background: var(--tech-normal-surface);
  }

  .ancient-column-label {
    border-left: 1px solid color-mix(in srgb, var(--tech-ancient), var(--border) 58%);
    color: var(--tech-ancient);
    background: var(--tech-ancient-surface);
  }

  .lane-columns.lane-normal,
  .level-band.lane-normal,
  .lane-columns.lane-ancient,
  .level-band.lane-ancient {
    grid-template-columns: var(--level-column) minmax(0, 1fr);
  }

  .level-band + .level-band {
    border-top: 1px solid var(--border);
  }

  .level-heading {
    display: grid;
    min-height: 126px;
    place-content: center;
    justify-items: center;
    padding: 12px 8px;
    border-right: 2px solid color-mix(in srgb, var(--tech-normal), transparent 28%);
    background: var(--surface-raised);
  }

  .level-heading strong {
    display: grid;
    width: 44px;
    height: 44px;
    place-items: center;
    border: 1px solid var(--tech-normal);
    border-radius: 4px;
    color: var(--tech-normal);
    font-weight: 850;
    font-size: 1rem;
    background: color-mix(in srgb, var(--tech-normal-surface), var(--surface-raised) 38%);
  }

  .lane-block {
    min-width: 0;
    padding: 10px;
    background: color-mix(in srgb, var(--tech-normal-surface), var(--ink) 74%);
  }

  .lane-block.ancient {
    border-left: 1px solid color-mix(in srgb, var(--tech-ancient), var(--border) 58%);
    background: color-mix(in srgb, var(--tech-ancient-surface), var(--ink) 18%);
  }

  .lane-block.empty-lane {
    min-height: 126px;
  }

  .card-grid {
    display: grid;
    min-width: 0;
    grid-template-columns: repeat(auto-fit, minmax(104px, 1fr));
    gap: 8px;
  }

  .ancient-grid {
    grid-template-columns: minmax(0, 1fr);
  }

  .empty-state {
    display: grid;
    min-height: 320px;
    place-items: center;
    align-content: center;
    padding: 32px;
    border: 1px dashed var(--border-strong);
    border-radius: 8px;
    text-align: center;
  }

  .empty-state h2 {
    margin: 12px 0 5px;
    font-size: 1rem;
  }

  .empty-state p {
    margin: 0 0 16px;
    color: var(--muted-strong);
    font-size: 0.78rem;
  }

  @media (max-width: 1220px) {
    .toolbar {
      grid-template-columns: minmax(240px, 1fr) auto;
    }

    .result-summary {
      grid-column: 1 / -1;
    }
  }

  @container technology (max-width: 800px) {
    .toolbar {
      grid-template-columns: 1fr;
      align-items: stretch;
    }

    .lane-filter button {
      flex: 1 0 auto;
    }

    .result-summary {
      grid-column: auto;
    }

    .lane-columns,
    .level-band {
      --level-column: 62px;
      --ancient-column: 138px;
    }

    .card-grid {
      grid-template-columns: repeat(auto-fit, minmax(88px, 1fr));
    }

    .ancient-grid {
      grid-template-columns: minmax(0, 1fr);
    }
  }

  @media (max-width: 719px) {
    .technology-screen {
      padding: 12px 10px 82px;
    }

    .level-jump {
      top: 58px;
    }

    .level-jump button {
      min-width: 44px;
      min-height: 44px;
    }

    .page-head {
      min-height: 52px;
      gap: 10px;
      padding: 8px 10px;
    }

    .page-head h1 {
      flex: 0 0 auto;
      font-size: 1rem;
    }

    .lane-filter button {
      min-height: 38px;
      padding-inline: 10px;
    }

    .toolbar {
      gap: 9px;
      padding: 10px;
    }
  }

  @media (max-width: 520px) {
    .level-jump {
      grid-template-columns: auto minmax(0, 1fr) auto;
      gap: 7px;
    }

    .level-jump > span {
      display: none;
    }

    .lane-columns,
    .level-band {
      --level-column: 48px;
      --ancient-column: 92px;
    }

    .level-heading {
      min-height: 116px;
      padding-inline: 4px;
    }

    .level-heading strong {
      width: 38px;
      height: 38px;
      font-size: 0.9rem;
    }

    .lane-block {
      padding: 7px;
    }

    .card-grid {
      grid-template-columns: 1fr;
      gap: 6px;
    }

    .lane-block.empty-lane {
      min-height: 116px;
    }

    .lane-columns > span {
      justify-content: center;
      padding-inline: 4px;
      font-size: 0.7rem;
    }
  }

  @media (max-width: 1023px) {
    .board-layout.with-inspector {
      grid-template-columns: 1fr;
    }

    .lane-columns {
      position: static;
    }

    .level-list {
      max-height: none;
      overflow: visible;
      padding-right: 0;
    }

    .technology-screen.detail-open .page-head,
    .technology-screen.detail-open .toolbar,
    .technology-screen.detail-open .level-jump,
    .technology-screen.detail-open .level-list {
      display: none;
    }

    .technology-screen.detail-open .board-layout {
      margin-top: 0;
    }
  }
</style>
