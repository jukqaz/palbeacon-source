<script lang="ts">
  import { onMount, tick, untrack } from 'svelte';
  import { get } from 'svelte/store';
  import { resolve } from '$app/paths';
  import { createVirtualizer } from '@tanstack/svelte-virtual';
  import { replaceCurrentQueryParameter } from '$lib/shared/navigation/query-state';
  import { safeGameDescription } from '$lib/shared/data/display-text';
  import { normalizeSearchText } from '$lib/shared/search/search-core';
  import WorkspaceAction from '$lib/shared/workspace/WorkspaceAction.svelte';
  import { rememberWorkspaceEntry } from '$lib/shared/workspace/workspace';
  import { ownedSpeciesCount, personalPalData } from '$lib/personal/personal-data';
  import { rarityAccent } from './catalog';
  import type { CatalogRecord, UnifiedCatalog } from './types';

  interface Props {
    catalog: UnifiedCatalog;
    title?: string;
    initialQuery?: string;
    initialSelectedId?: string;
    initialOwnedOnly?: boolean;
  }

  type PalDetailTab = 'overview' | 'work' | 'skills' | 'drops';
  type PalSort = 'paldex' | 'name' | 'hp' | 'attack' | 'defense';
  type PalViewMode = 'grid' | 'list';
  type WorkMatch = 'all' | 'any';
  type FilterSection = 'elements' | 'work' | 'detail';

  let {
    catalog,
    title = '팰 도감',
    initialQuery = '',
    initialSelectedId = '',
    initialOwnedOnly = false,
  }: Props = $props();
  let query = $state(untrack(() => initialQuery));
  let ownedOnly = $state(untrack(() => initialOwnedOnly));
  let selectedElements = $state<string[]>([]);
  let selectedWorks = $state<string[]>([]);
  let workMatch = $state<WorkMatch>('all');
  let minimumWorkLevel = $state(1);
  let nocturnalOnly = $state(false);
  let minimumHp = $state<string | number>('');
  let minimumAttack = $state<string | number>('');
  let minimumDefense = $state<string | number>('');
  let sort = $state<PalSort>('paldex');
  let viewMode = $state<PalViewMode>('grid');
  let filtersOpen = $state(false);
  let filterSection = $state<FilterSection>('elements');
  let selectedId = $state<string | null>(null);
  let activeTab = $state<PalDetailTab>('overview');
  let wide = $state(false);
  let mobileDetailOpen = $state(false);
  let narrowLimit = $state(24);
  let desktopGridLimit = $state(48);
  let narrowFilterSignature = $state('');
  let appliedInitialState = $state<string | null>(null);
  let listScroller = $state<HTMLDivElement | null>(null);
  let lastListTrigger = $state<HTMLButtonElement | null>(null);
  let filterShell = $state<HTMLDivElement | null>(null);
  let lastFilterTrigger = $state<HTMLButtonElement | null>(null);
  let pinnedWork = $state<string | null>(null);
  let failedMedia = $state(new Set<string>());

  const pals = $derived(
    catalog.records.pals.filter(
      (pal) => !pal.localization_fallback && pal.name_ko.trim().length > 0,
    ),
  );
  const selected = $derived(
    selectedId === null ? null : (pals.find((pal) => pal.id === selectedId) ?? null),
  );
  const elementOptions = $derived.by(() => {
    const options = new Map<string, string>();
    for (const pal of pals) {
      for (const element of pal.elements ?? []) options.set(element.id, element.name_ko);
    }
    return [...options.entries()].toSorted((left, right) => left[1].localeCompare(right[1], 'ko'));
  });
  const workOptions = $derived.by(() => {
    const options = new Map<string, string>();
    for (const pal of pals) {
      for (const work of pal.work_suitability ?? []) options.set(work.id, work.name_ko);
    }
    return [...options.entries()].toSorted((left, right) => left[1].localeCompare(right[1], 'ko'));
  });

  const metricNumber = (pal: CatalogRecord, label: string): number => {
    const value = pal.metrics.find((metric) => metric.label === label)?.value;
    if (!value) return -1;
    const parsed = Number(value.replaceAll(',', ''));
    return Number.isFinite(parsed) ? parsed : -1;
  };

  const paldexNumber = (pal: CatalogRecord) =>
    pal.pal_profile?.paldex_number ?? Number.MAX_SAFE_INTEGER;

  const paldexLabel = (pal: CatalogRecord): string | null => {
    const number = pal.pal_profile?.paldex_number;
    if (number === null || number === undefined) return null;
    return `No. ${number.toString().padStart(3, '0')}${pal.pal_profile?.paldex_suffix ?? ''}`;
  };

  const sortPals = (left: CatalogRecord, right: CatalogRecord): number => {
    if (sort === 'name') return left.name_ko.localeCompare(right.name_ko, 'ko');
    if (sort === 'hp') return metricNumber(right, 'HP') - metricNumber(left, 'HP');
    if (sort === 'attack') return metricNumber(right, '공격') - metricNumber(left, '공격');
    if (sort === 'defense') return metricNumber(right, '방어') - metricNumber(left, '방어');
    return (
      paldexNumber(left) - paldexNumber(right) ||
      (left.pal_profile?.paldex_suffix ?? '').localeCompare(
        right.pal_profile?.paldex_suffix ?? '',
      ) ||
      left.name_ko.localeCompare(right.name_ko, 'ko')
    );
  };

  const searchablePalText = (pal: CatalogRecord) =>
    normalizeSearchText(
      [
        pal.name_ko,
        pal.id,
        pal.description_ko ?? '',
        ...pal.tags,
        ...pal.search_terms,
        ...(pal.elements ?? []).map((element) => element.name_ko),
        ...(pal.work_suitability ?? []).flatMap((work) => [
          work.name_ko,
          `${work.name_ko} ${work.level.toString()}`,
        ]),
      ].join(' '),
    );

  const threshold = (value: string | number): number | null => {
    if (String(value).trim().length === 0) return null;
    const parsed = Number(value);
    return Number.isFinite(parsed) && parsed >= 0 ? parsed : null;
  };

  const hasSelectedElement = (pal: CatalogRecord) =>
    selectedElements.length === 0 ||
    (pal.elements ?? []).some((element) => selectedElements.includes(element.id));

  const hasSelectedWork = (pal: CatalogRecord) => {
    if (selectedWorks.length === 0) return true;
    const available = new Set(
      (pal.work_suitability ?? [])
        .filter((work) => work.level >= minimumWorkLevel)
        .map((work) => work.id),
    );
    return workMatch === 'all'
      ? selectedWorks.every((workId) => available.has(workId))
      : selectedWorks.some((workId) => available.has(workId));
  };

  const meetsMinimum = (pal: CatalogRecord, label: string, value: string | number) => {
    const minimum = threshold(value);
    return minimum === null || metricNumber(pal, label) >= minimum;
  };

  const filteredPals = $derived.by(() => {
    const normalizedQuery = normalizeSearchText(query);
    return pals
      .filter(
        (pal) =>
          (normalizedQuery.length === 0 || searchablePalText(pal).includes(normalizedQuery)) &&
          (!ownedOnly ||
            !$personalPalData.ready ||
            ownedSpeciesCount($personalPalData, pal.id) > 0) &&
          hasSelectedElement(pal) &&
          hasSelectedWork(pal) &&
          (!nocturnalOnly || pal.pal_profile?.nocturnal === true) &&
          meetsMinimum(pal, 'HP', minimumHp) &&
          meetsMinimum(pal, '공격', minimumAttack) &&
          meetsMinimum(pal, '방어', minimumDefense),
      )
      .toSorted(sortPals);
  });
  const narrowPals = $derived(filteredPals.slice(0, narrowLimit));
  const renderedGridPals = $derived(filteredPals.slice(0, wide ? desktopGridLimit : narrowLimit));
  const detailFilterCount = $derived(
    Number(nocturnalOnly) +
      Number(threshold(minimumHp) !== null) +
      Number(threshold(minimumAttack) !== null) +
      Number(threshold(minimumDefense) !== null),
  );
  const activeFilterCount = $derived(
    Number(ownedOnly && $personalPalData.ready) +
      Number(selectedElements.length > 0) +
      Number(selectedWorks.length > 0) +
      Number(selectedWorks.length > 0 && minimumWorkLevel > 1) +
      Number(nocturnalOnly) +
      Number(threshold(minimumHp) !== null) +
      Number(threshold(minimumAttack) !== null) +
      Number(threshold(minimumDefense) !== null),
  );

  const visibleMetrics = $derived(
    (selected?.metrics ?? []).filter((metric) => ['HP', '공격', '방어'].includes(metric.label)),
  );
  const primaryWork = $derived.by(() => {
    const work = selected?.work_suitability;
    if (!work?.length) return null;
    const leading = work.toSorted(
      (left, right) => right.level - left.level || left.name_ko.localeCompare(right.name_ko, 'ko'),
    )[0]!;
    const remaining = work.length - 1;
    return `${leading.name_ko} Lv. ${leading.level.toString()}${remaining > 0 ? ` 외 ${remaining.toString()}개` : ''}`;
  });
  const visibleTabs = $derived.by(() => {
    if (!selected) return [];
    return [
      { id: 'overview' as const, label: '개요' },
      ...(selected.work_suitability?.length
        ? [{ id: 'work' as const, label: `작업 ${selected.work_suitability.length.toString()}` }]
        : []),
      ...(selected.pal_skills?.length
        ? [{ id: 'skills' as const, label: `스킬 ${selected.pal_skills.length.toString()}` }]
        : []),
      ...(selected.pal_drops?.length
        ? [{ id: 'drops' as const, label: `획득 ${selected.pal_drops.length.toString()}` }]
        : []),
    ];
  });
  const dropGroups = $derived.by(() => {
    const drops = selected?.pal_drops ?? [];
    return [
      {
        id: 'normal' as const,
        label: '일반 드롭',
        drops: drops.filter((drop) => drop.variant === 'normal'),
      },
      {
        id: 'boss' as const,
        label: '보스 드롭',
        drops: drops.filter((drop) => drop.variant === 'boss'),
      },
    ].filter((group) => group.drops.length > 0);
  });

  const virtualizer = createVirtualizer<HTMLDivElement, HTMLDivElement>({
    count: 0,
    getScrollElement: () => listScroller,
    estimateSize: () => 86,
    overscan: 8,
  });

  const exactRecordForEntry = (displayQuery: string, entityId: string) => {
    const normalizedId = normalizeSearchText(entityId);
    if (normalizedId.length > 0) {
      const idMatch = pals.find((pal) => normalizeSearchText(pal.id) === normalizedId);
      if (idMatch) return idMatch;
    }
    const normalizedQuery = normalizeSearchText(displayQuery);
    if (normalizedQuery.length === 0) return null;
    const exactMatches = pals.filter(
      (pal) =>
        normalizeSearchText(pal.name_ko) === normalizedQuery ||
        normalizeSearchText(pal.id) === normalizedQuery,
    );
    return exactMatches.length === 1 ? exactMatches[0] : null;
  };

  $effect(() => {
    const displayQuery = initialQuery.trim();
    const entityId = initialSelectedId.trim();
    const signature = `${displayQuery}\u0000${entityId}`;
    if (signature === appliedInitialState) return;
    appliedInitialState = signature;
    query = displayQuery;
    selectedElements = [];
    selectedWorks = [];
    workMatch = 'all';
    minimumWorkLevel = 1;
    nocturnalOnly = false;
    minimumHp = '';
    minimumAttack = '';
    minimumDefense = '';
    sort = 'paldex';
    const exact = exactRecordForEntry(displayQuery, entityId);
    selectedId = exact?.id ?? null;
    activeTab = 'overview';
    mobileDetailOpen = exact !== null;
  });

  $effect(() => {
    const scroller = listScroller;
    const count = wide && viewMode === 'list' ? filteredPals.length : 0;
    untrack(() => {
      get(virtualizer).setOptions({
        count,
        getScrollElement: () => scroller,
        estimateSize: () => 86,
        overscan: 8,
      });
    });
  });

  $effect(() => {
    const results = filteredPals;
    const currentId = selectedId;
    if (currentId !== null && !results.some((pal) => pal.id === currentId)) {
      selectedId = wide ? (results[0]?.id ?? null) : null;
      activeTab = 'overview';
      mobileDetailOpen = false;
      return;
    }
    if (wide && currentId === null && results[0]) selectedId = results[0].id;
  });

  $effect(() => {
    if (!visibleTabs.some((tab) => tab.id === activeTab)) activeTab = 'overview';
  });

  $effect(() => {
    const signature = [
      query,
      selectedElements.join(','),
      selectedWorks.join(','),
      workMatch,
      minimumWorkLevel,
      nocturnalOnly,
      minimumHp,
      minimumAttack,
      minimumDefense,
      sort,
      viewMode,
    ].join('\u0000');
    if (signature === narrowFilterSignature) return;
    narrowFilterSignature = signature;
    narrowLimit = 24;
    desktopGridLimit = 48;
  });

  onMount(() => {
    const media = window.matchMedia('(min-width: 1024px)');
    const update = () => {
      const wasWide = wide;
      wide = media.matches;
      if (wasWide && !wide) mobileDetailOpen = false;
    };
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key !== 'Escape' || !filtersOpen) return;
      filtersOpen = false;
      void tick().then(() => lastFilterTrigger?.focus());
    };
    const closeOnOutsidePointer = (event: PointerEvent) => {
      if (!filtersOpen || !filterShell || filterShell.contains(event.target as Node)) return;
      filtersOpen = false;
    };
    update();
    media.addEventListener('change', update);
    window.addEventListener('keydown', closeOnEscape);
    window.addEventListener('pointerdown', closeOnOutsidePointer);
    return () => {
      media.removeEventListener('change', update);
      window.removeEventListener('keydown', closeOnEscape);
      window.removeEventListener('pointerdown', closeOnOutsidePointer);
    };
  });

  const mediaAvailable = (path: string | null | undefined): path is string =>
    Boolean(path) && !failedMedia.has(path as string);

  const markMediaFailed = (path: string | null | undefined) => {
    if (!path || failedMedia.has(path)) return;
    failedMedia = new Set([...failedMedia, path]);
  };

  const selectPal = (pal: CatalogRecord, trigger: HTMLButtonElement) => {
    lastListTrigger = trigger;
    selectedId = pal.id;
    activeTab = 'overview';
    mobileDetailOpen = !wide;
    rememberWorkspaceEntry({
      kind: 'pal',
      id: pal.id,
      name_ko: pal.name_ko,
      href: palHref(pal),
      image_path: pal.image_path,
    });
  };

  const closeMobileDetail = async () => {
    mobileDetailOpen = false;
    await tick();
    lastListTrigger?.focus();
  };

  const resetFilters = () => {
    query = '';
    setOwnedOnly(false);
    selectedElements = [];
    selectedWorks = [];
    workMatch = 'all';
    minimumWorkLevel = 1;
    nocturnalOnly = false;
    minimumHp = '';
    minimumAttack = '';
    minimumDefense = '';
    sort = 'paldex';
    filtersOpen = false;
    mobileDetailOpen = false;
  };

  const setOwnedOnly = (next: boolean) => {
    ownedOnly = next;
    replaceCurrentQueryParameter('mine', next ? '1' : null);
  };

  const openFilter = (section: FilterSection, trigger: HTMLButtonElement) => {
    const shouldClose = filtersOpen && filterSection === section;
    filterSection = section;
    filtersOpen = !shouldClose;
    lastFilterTrigger = trigger;
  };

  const toggleChoice = (values: string[], value: string) =>
    values.includes(value) ? values.filter((entry) => entry !== value) : [...values, value];

  const toggleElement = async (value: string) => {
    selectedElements = toggleChoice(selectedElements, value);
    if (wide || filterSection !== 'elements') return;
    filtersOpen = false;
    await tick();
    lastFilterTrigger?.focus();
  };

  const toggleWork = (value: string) => {
    selectedWorks = toggleChoice(selectedWorks, value);
  };

  const togglePinnedWork = (pal: CatalogRecord, workId: string, surface: string) => {
    const key = `${surface}:${pal.id}:${workId}`;
    pinnedWork = pinnedWork === key ? null : key;
  };

  const workKey = (pal: CatalogRecord, workId: string, surface: string) =>
    `${surface}:${pal.id}:${workId}`;

  const recordAriaLabel = (pal: CatalogRecord) =>
    [
      pal.name_ko,
      paldexLabel(pal),
      ownedSpeciesCount($personalPalData, pal.id) > 0
        ? `보유 ${ownedSpeciesCount($personalPalData, pal.id).toLocaleString('ko-KR')}마리`
        : null,
      ...(pal.elements ?? []).map((element) => element.name_ko),
      ...(pal.work_suitability ?? [])
        .slice(0, 2)
        .map((work) => `${work.name_ko} 레벨 ${work.level.toString()}`),
      '팰',
    ]
      .filter(Boolean)
      .join(', ');

  const quantityLabel = (minimum: number, maximum: number) =>
    minimum === maximum
      ? `${minimum.toLocaleString('ko-KR')}개`
      : `${minimum.toLocaleString('ko-KR')}–${maximum.toLocaleString('ko-KR')}개`;

  const probabilityLabel = (probabilityPpm: number) =>
    probabilityPpm === 1_000_000
      ? '확정'
      : `${new Intl.NumberFormat('ko-KR', { maximumFractionDigits: 2 }).format(probabilityPpm / 10_000)}%`;

  const itemHref = (name: string, id: string) => {
    const parameters = new URLSearchParams({ q: name, id });
    return `${resolve('/items/', {})}?${parameters.toString()}`;
  };

  const palHref = (pal: CatalogRecord) => {
    const parameters = new URLSearchParams({ q: pal.name_ko, id: pal.id });
    return `${resolve('/pals/', {})}?${parameters.toString()}`;
  };

  const breedingPlanHref = (pal: CatalogRecord) => {
    const parameters = new URLSearchParams({ target: pal.id });
    return `${resolve('/plan/breeding/', {})}?${parameters.toString()}`;
  };
</script>

{#snippet elementMarks(pal: CatalogRecord)}
  {#if pal.elements?.length}
    <span class="element-marks" aria-label="속성">
      {#each pal.elements as element (element.id)}
        <span>
          {#if mediaAvailable(element.icon_path)}
            <img
              src={element.icon_path}
              alt=""
              onerror={() => markMediaFailed(element.icon_path)}
            />
          {/if}
          {element.name_ko}
        </span>
      {/each}
    </span>
  {/if}
{/snippet}

{#snippet workIndicators(pal: CatalogRecord, surface: string)}
  {#if pal.work_suitability?.length}
    <span class="work-indicators" aria-label="대표 작업 적성">
      {#each pal.work_suitability.slice(0, 3) as work (work.id)}
        {@const key = workKey(pal, work.id, surface)}
        <span class="work-chip">
          <button
            type="button"
            class:pinned={pinnedWork === key}
            aria-label={`${work.name_ko} 레벨 ${work.level.toString()}`}
            aria-pressed={pinnedWork === key}
            title={`${work.name_ko} 레벨 ${work.level.toString()}`}
            onclick={() => togglePinnedWork(pal, work.id, surface)}
          >
            {#if mediaAvailable(work.icon_path)}
              <img src={work.icon_path} alt="" onerror={() => markMediaFailed(work.icon_path)} />
            {:else}
              <span class="work-letter">{work.name_ko.slice(0, 1)}</span>
            {/if}
            <b>{work.level}</b>
          </button>
          <span class:pinned={pinnedWork === key} class="work-tooltip" role="tooltip"
            >{work.name_ko} Lv. {work.level}</span
          >
        </span>
      {/each}
      {#if pal.work_suitability.length > 3}
        {@const moreKey = workKey(pal, 'more', surface)}
        <span class="work-chip">
          <button
            type="button"
            class="more-work"
            class:pinned={pinnedWork === moreKey}
            aria-label={`나머지 작업 적성 ${(pal.work_suitability.length - 3).toString()}개`}
            aria-pressed={pinnedWork === moreKey}
            title={pal.work_suitability
              .slice(3)
              .map((work) => `${work.name_ko} 레벨 ${work.level.toString()}`)
              .join(', ')}
            onclick={() => togglePinnedWork(pal, 'more', surface)}
            >+{(pal.work_suitability.length - 3).toString()}</button
          >
          <span class:pinned={pinnedWork === moreKey} class="work-tooltip" role="tooltip">
            {pal.work_suitability
              .slice(3)
              .map((work) => `${work.name_ko} Lv. ${work.level.toString()}`)
              .join(' · ')}
          </span>
        </span>
      {/if}
    </span>
  {/if}
{/snippet}

{#snippet palCard(pal: CatalogRecord)}
  <article
    class="pal-card"
    class:no-media={!mediaAvailable(pal.image_path)}
    class:selected={selectedId === pal.id}
  >
    <button
      type="button"
      class="pal-select pal-card-main"
      aria-pressed={selectedId === pal.id}
      aria-label={recordAriaLabel(pal)}
      onclick={(event) => selectPal(pal, event.currentTarget)}
    >
      {#if mediaAvailable(pal.image_path)}
        <span class="card-media">
          <img
            src={pal.image_path}
            alt=""
            loading="lazy"
            onerror={() => markMediaFailed(pal.image_path)}
          />
        </span>
      {/if}
      <span class="card-copy">
        {#if paldexLabel(pal)}<small>{paldexLabel(pal)}</small>{/if}
        <strong>{pal.name_ko}</strong>
        {#if ownedSpeciesCount($personalPalData, pal.id) > 0}<span class="owned-mark"
            >보유 {ownedSpeciesCount($personalPalData, pal.id).toLocaleString('ko-KR')}마리</span
          >{/if}
        {@render elementMarks(pal)}
      </span>
    </button>
    <footer>{@render workIndicators(pal, 'grid')}</footer>
  </article>
{/snippet}

{#snippet palRow(pal: CatalogRecord)}
  <article
    class="pal-row"
    class:no-media={!mediaAvailable(pal.image_path)}
    class:selected={selectedId === pal.id}
  >
    <button
      type="button"
      class="pal-select pal-row-main"
      aria-pressed={selectedId === pal.id}
      aria-label={recordAriaLabel(pal)}
      onclick={(event) => selectPal(pal, event.currentTarget)}
    >
      {#if mediaAvailable(pal.image_path)}
        <span class="row-media">
          <img
            src={pal.image_path}
            alt=""
            loading="lazy"
            onerror={() => markMediaFailed(pal.image_path)}
          />
        </span>
      {/if}
      <span class="row-copy">
        <span class="row-heading">
          <strong>{pal.name_ko}</strong>
          <span class="row-labels">
            {#if ownedSpeciesCount($personalPalData, pal.id) > 0}<small class="owned-mark"
                >보유 {ownedSpeciesCount($personalPalData, pal.id).toLocaleString(
                  'ko-KR',
                )}마리</small
              >{/if}
            {#if paldexLabel(pal)}<small>{paldexLabel(pal)}</small>{/if}
          </span>
        </span>
        {@render elementMarks(pal)}
      </span>
    </button>
    <span class="row-work-cell">{@render workIndicators(pal, 'list')}</span>
    <dl class="row-metrics" aria-label="기본 능력치">
      {#each ['HP', '공격', '방어'] as label (label)}
        {@const value = metricNumber(pal, label)}
        {#if value >= 0}
          <div>
            <dt class="sr-only">{label}</dt>
            <dd>{value.toLocaleString('ko-KR')}</dd>
          </div>
        {/if}
      {/each}
    </dl>
  </article>
{/snippet}

<section class:detail-open={mobileDetailOpen && selected !== null} class="pal-catalog-screen">
  <div class="pal-explorer">
    <section class="pal-index" aria-label="팰 목록">
      <header class="index-heading">
        <div class="index-title">
          <h1>{title}</h1>
          <span aria-live="polite">{filteredPals.length.toLocaleString('ko-KR')}종</span>
        </div>
        <div class="index-actions">
          <label>
            <span class="sr-only">팰 정렬</span>
            <select bind:value={sort} aria-label="팰 정렬">
              <option value="paldex">도감 번호순</option>
              <option value="name">이름순</option>
              <option value="hp">HP 높은순</option>
              <option value="attack">공격 높은순</option>
              <option value="defense">방어 높은순</option>
            </select>
          </label>
          <div class="view-switch" role="group" aria-label="보기 방식">
            <button
              type="button"
              class:active={viewMode === 'grid'}
              aria-pressed={viewMode === 'grid'}
              aria-label="그리드 보기"
              title="그리드 보기"
              onclick={() => (viewMode = 'grid')}>그리드</button
            >
            <button
              type="button"
              class:active={viewMode === 'list'}
              aria-pressed={viewMode === 'list'}
              aria-label="목록 보기"
              title="목록 보기"
              onclick={() => (viewMode = 'list')}>목록</button
            >
          </div>
        </div>
      </header>

      <div class="pal-tools" bind:this={filterShell}>
        <label class="search-field">
          <span class="sr-only">{title} 검색</span>
          <input
            type="search"
            bind:value={query}
            aria-label={`${title} 검색`}
            placeholder="이름·속성·작업 적성"
            autocomplete="off"
          />
          {#if query.length > 0}
            <button type="button" aria-label="검색어 지우기" onclick={() => (query = '')}
              >지우기</button
            >
          {/if}
        </label>
        <div class="tool-actions">
          <div class="filter-actions" aria-label="팰 필터">
            {#if $personalPalData.ready}
              <button
                type="button"
                class:active={ownedOnly}
                aria-pressed={ownedOnly}
                onclick={() => setOwnedOnly(!ownedOnly)}>내 팰만</button
              >
            {/if}
            <button
              type="button"
              class:active={selectedElements.length > 0}
              aria-expanded={filtersOpen && filterSection === 'elements'}
              aria-controls="pal-filter-panel"
              onclick={(event) => openFilter('elements', event.currentTarget)}
              >속성{selectedElements.length > 0
                ? ` ${selectedElements.length.toString()}`
                : ''}</button
            >
            <button
              type="button"
              class:active={selectedWorks.length > 0}
              aria-expanded={filtersOpen && filterSection === 'work'}
              aria-controls="pal-filter-panel"
              onclick={(event) => openFilter('work', event.currentTarget)}
              >작업 적성{selectedWorks.length > 0
                ? ` ${selectedWorks.length.toString()}`
                : ''}</button
            >
            <button
              type="button"
              class:active={detailFilterCount > 0}
              aria-expanded={filtersOpen && filterSection === 'detail'}
              aria-controls="pal-filter-panel"
              onclick={(event) => openFilter('detail', event.currentTarget)}
              >상세 필터{detailFilterCount > 0 ? ` ${detailFilterCount.toString()}` : ''}</button
            >
          </div>
        </div>

        {#if activeFilterCount > 0}
          <div class="active-filters" aria-label="적용 중인 필터">
            {#if ownedOnly && $personalPalData.ready}
              <button type="button" onclick={() => setOwnedOnly(false)}>내 팰 ×</button>
            {/if}
            {#if selectedElements.length > 0}
              <button type="button" onclick={() => (selectedElements = [])}
                >속성 {selectedElements.length} ×</button
              >
            {/if}
            {#if selectedWorks.length > 0}
              <button type="button" onclick={() => (selectedWorks = [])}
                >작업 {selectedWorks.length} · {workMatch === 'all' ? '모두' : '하나'} ×</button
              >
            {/if}
            {#if selectedWorks.length > 0 && minimumWorkLevel > 1}
              <button type="button" onclick={() => (minimumWorkLevel = 1)}
                >Lv. {minimumWorkLevel}+ ×</button
              >
            {/if}
            {#if nocturnalOnly}
              <button type="button" onclick={() => (nocturnalOnly = false)}>야행성 ×</button>
            {/if}
            {#if threshold(minimumHp) !== null}
              <button type="button" onclick={() => (minimumHp = '')}>HP {minimumHp}+ ×</button>
            {/if}
            {#if threshold(minimumAttack) !== null}
              <button type="button" onclick={() => (minimumAttack = '')}
                >공격 {minimumAttack}+ ×</button
              >
            {/if}
            {#if threshold(minimumDefense) !== null}
              <button type="button" onclick={() => (minimumDefense = '')}
                >방어 {minimumDefense}+ ×</button
              >
            {/if}
            <button class="reset-all" type="button" onclick={resetFilters}>초기화</button>
          </div>
        {/if}

        {#if filtersOpen || wide}
          <section id="pal-filter-panel" class="filter-panel" aria-label="팰 필터 선택">
            <header>
              <strong>필터</strong>
              <button type="button" aria-label="필터 닫기" onclick={() => (filtersOpen = false)}
                >닫기</button
              >
            </header>

            <div class="filter-sections">
              <section class:active-section={filterSection === 'elements'}>
                <h3>속성</h3>
                <div class="choice-grid element-choices">
                  {#each elementOptions as option (option[0])}
                    {@const element = pals
                      .flatMap((pal) => pal.elements ?? [])
                      .find((entry) => entry.id === option[0])}
                    <label class:checked={selectedElements.includes(option[0])}>
                      <input
                        type="checkbox"
                        aria-label={option[1]}
                        checked={selectedElements.includes(option[0])}
                        onchange={() => toggleElement(option[0])}
                      />
                      {#if element && mediaAvailable(element.icon_path)}
                        <img
                          src={element.icon_path}
                          alt=""
                          onerror={() => markMediaFailed(element.icon_path)}
                        />
                      {/if}
                      <span>{option[1]}</span>
                    </label>
                  {/each}
                </div>
              </section>

              <section class:active-section={filterSection === 'work'}>
                <h3>작업 적성</h3>
                <div class="work-filter-head">
                  <fieldset>
                    <legend>선택 작업 조건</legend>
                    <label
                      ><input type="radio" bind:group={workMatch} value="all" /> 모두 만족</label
                    >
                    <label
                      ><input type="radio" bind:group={workMatch} value="any" /> 하나 이상</label
                    >
                  </fieldset>
                  <label class="level-filter">
                    <span>선택 작업 최소 레벨</span>
                    <select bind:value={minimumWorkLevel} disabled={selectedWorks.length === 0}>
                      {#each Array.from({ length: 8 }, (_, index) => index + 1) as level (level)}
                        <option value={level}>Lv. {level}+</option>
                      {/each}
                    </select>
                  </label>
                </div>
                <div class="choice-grid work-choices">
                  {#each workOptions as option (option[0])}
                    {@const work = pals
                      .flatMap((pal) => pal.work_suitability ?? [])
                      .find((entry) => entry.id === option[0])}
                    <label class:checked={selectedWorks.includes(option[0])}>
                      <input
                        type="checkbox"
                        checked={selectedWorks.includes(option[0])}
                        onchange={() => toggleWork(option[0])}
                      />
                      {#if work && mediaAvailable(work.icon_path)}
                        <img
                          src={work.icon_path}
                          alt=""
                          onerror={() => markMediaFailed(work.icon_path)}
                        />
                      {/if}
                      <span>{option[1]}</span>
                    </label>
                  {/each}
                </div>
              </section>

              <section class:active-section={filterSection === 'detail'}>
                <h3>상세</h3>
                <div class="detail-filters">
                  <label class="boolean-filter">
                    <input type="checkbox" bind:checked={nocturnalOnly} />
                    <span>야행성만</span>
                  </label>
                  <div class="minimum-stats">
                    <label
                      ><span>HP 최소</span><input
                        type="number"
                        min="0"
                        bind:value={minimumHp}
                      /></label
                    >
                    <label
                      ><span>공격 최소</span><input
                        type="number"
                        min="0"
                        bind:value={minimumAttack}
                      /></label
                    >
                    <label
                      ><span>방어 최소</span><input
                        type="number"
                        min="0"
                        bind:value={minimumDefense}
                      /></label
                    >
                  </div>
                </div>
              </section>
            </div>

            <footer>
              <span aria-live="polite">{filteredPals.length.toLocaleString('ko-KR')}종</span>
              <button type="button" onclick={resetFilters}>전체 초기화</button>
            </footer>
          </section>
        {/if}
      </div>

      {#if filteredPals.length === 0}
        <div class="empty-state">
          <strong>조건에 맞는 팰이 없습니다.</strong>
          <button type="button" onclick={resetFilters}>초기화</button>
        </div>
      {:else}
        <div class="result-region" aria-label={`${title} 결과`} bind:this={listScroller}>
          {#if viewMode === 'list'}
            <div class="list-column-head" aria-hidden="true">
              <span>팰</span>
              <span>작업 적성</span>
              <span class="metric-head"><span>HP</span><span>공격</span><span>방어</span></span>
            </div>
          {/if}
          {#if viewMode === 'grid'}
            <div class="pal-grid">
              {#each renderedGridPals as pal (pal.id)}
                {@render palCard(pal)}
              {/each}
            </div>
            {#if renderedGridPals.length < filteredPals.length}
              <button
                class="load-more"
                type="button"
                onclick={() => (wide ? (desktopGridLimit += 48) : (narrowLimit += 24))}
                >다음 {Math.min(
                  wide ? 48 : 24,
                  filteredPals.length - renderedGridPals.length,
                ).toString()}종 보기</button
              >
            {/if}
          {:else if wide}
            <div
              class="virtual-canvas"
              style:height={`${$virtualizer.getTotalSize().toString()}px`}
            >
              {#each $virtualizer.getVirtualItems() as row (row.key)}
                {@const pal = filteredPals[row.index]}
                {#if pal}
                  <div
                    class="virtual-row"
                    data-index={row.index}
                    use:$virtualizer.measureElement
                    style:transform={`translateY(${row.start.toString()}px)`}
                  >
                    {@render palRow(pal)}
                  </div>
                {/if}
              {/each}
            </div>
          {:else}
            <div class="pal-list">
              {#each narrowPals as pal (pal.id)}
                {@render palRow(pal)}
              {/each}
              {#if narrowPals.length < filteredPals.length}
                <button class="load-more" type="button" onclick={() => (narrowLimit += 24)}
                  >다음 {Math.min(24, filteredPals.length - narrowPals.length).toString()}종 보기</button
                >
              {/if}
            </div>
          {/if}
        </div>
      {/if}
    </section>

    {#if selected}
      <aside class="pal-detail inspector" aria-label="선택한 팰 상세">
        <button class="mobile-back" type="button" aria-label="팰 목록" onclick={closeMobileDetail}
          >← 목록</button
        >

        <header class="pal-hero" class:no-media={!mediaAvailable(selected.image_path)}>
          <span class="hero-corner top-left" aria-hidden="true"></span>
          <span class="hero-corner bottom-right" aria-hidden="true"></span>
          {#if mediaAvailable(selected.image_path)}
            <span class="hero-media">
              <img
                src={selected.image_path}
                alt=""
                onerror={() => markMediaFailed(selected.image_path)}
              />
            </span>
          {/if}
          <span class="hero-identity">
            {#if paldexLabel(selected)}<small>{paldexLabel(selected)}</small>{/if}
            <h2>{selected.name_ko}</h2>
            {#if selected.elements?.length}
              <span class="element-list" aria-label="속성">
                {#each selected.elements as element (element.id)}
                  <span>
                    {#if mediaAvailable(element.icon_path)}
                      <img
                        src={element.icon_path}
                        alt=""
                        onerror={() => markMediaFailed(element.icon_path)}
                      />
                    {/if}
                    {element.name_ko}
                  </span>
                {/each}
              </span>
            {/if}
          </span>
        </header>

        <div class="workspace-detail-action">
          {#if ownedSpeciesCount($personalPalData, selected.id) > 0}<strong class="owned-detail"
              >보유 {ownedSpeciesCount($personalPalData, selected.id).toLocaleString(
                'ko-KR',
              )}마리</strong
            >{/if}
          <a class="plan-link" href={breedingPlanHref(selected)}>교배 계획</a>
          <WorkspaceAction
            entry={{
              kind: 'pal',
              id: selected.id,
              name_ko: selected.name_ko,
              href: palHref(selected),
              image_path: selected.image_path,
            }}
          />
        </div>

        {#if visibleMetrics.length > 0}
          <dl class="stat-strip">
            {#each visibleMetrics as metric (metric.label)}
              <div>
                <dt>{metric.label}</dt>
                <dd>{metric.value}</dd>
              </div>
            {/each}
          </dl>
        {/if}

        <div class="detail-tabs" role="tablist" aria-label="팰 상세 정보">
          {#each visibleTabs as tab (tab.id)}
            <button
              type="button"
              role="tab"
              aria-selected={activeTab === tab.id}
              class:active={activeTab === tab.id}
              onclick={() => (activeTab = tab.id)}>{tab.label}</button
            >
          {/each}
        </div>

        <div class="detail-content">
          {#if activeTab === 'overview'}
            <section class="overview-panel" role="tabpanel">
              {#if primaryWork || selected.pal_profile}
                <dl class="profile-facts">
                  {#if primaryWork}
                    <div class="primary-work">
                      <dt>주요 작업</dt>
                      <dd>{primaryWork}</dd>
                    </div>
                  {/if}
                  {#if selected.pal_profile}
                    <div>
                      <dt>활동</dt>
                      <dd>{selected.pal_profile.nocturnal ? '야행성' : '주행성'}</dd>
                    </div>
                    <div>
                      <dt>식사량</dt>
                      <dd>{selected.pal_profile.food_amount.toLocaleString('ko-KR')}</dd>
                    </div>
                  {/if}
                </dl>
              {/if}
              {#if safeGameDescription(selected.description_ko)}<p>
                  {safeGameDescription(selected.description_ko)}
                </p>{/if}
            </section>
          {:else if activeTab === 'work' && selected.work_suitability?.length}
            <section class="work-panel" role="tabpanel">
              <ul class="work-grid">
                {#each selected.work_suitability as work (work.id)}
                  <li class:no-media={!mediaAvailable(work.icon_path)}>
                    {#if mediaAvailable(work.icon_path)}
                      <img
                        src={work.icon_path}
                        alt=""
                        onerror={() => markMediaFailed(work.icon_path)}
                      />
                    {/if}
                    <strong>{work.name_ko}</strong>
                    <b>Lv. {work.level}</b>
                  </li>
                {/each}
              </ul>
            </section>
          {:else if activeTab === 'skills' && selected.pal_skills?.length}
            <section class="skills-panel" role="tabpanel">
              <ol class="skill-list">
                {#each selected.pal_skills as skill (skill.id)}
                  <li class:no-media={!mediaAvailable(skill.icon_path)}>
                    <span class="skill-level">Lv. <b>{skill.level}</b></span>
                    {#if mediaAvailable(skill.icon_path)}
                      <img
                        src={skill.icon_path}
                        alt=""
                        onerror={() => markMediaFailed(skill.icon_path)}
                      />
                    {/if}
                    <span class="skill-copy">
                      <strong>{skill.name_ko}</strong>
                      <span class="skill-meta">
                        <span>{skill.element_name_ko}</span>
                        {#if skill.power > 0}<span>위력 {skill.power}</span>{/if}
                        {#if skill.cooldown_seconds > 0}<span
                            >재사용 {skill.cooldown_seconds}초</span
                          >{/if}
                      </span>
                    </span>
                  </li>
                {/each}
              </ol>
            </section>
          {:else if activeTab === 'drops' && selected.pal_drops?.length}
            <section class="drops-panel" role="tabpanel">
              <div class="drop-groups">
                {#each dropGroups as group (group.id)}
                  <section class="drop-group">
                    <h3>{group.label}</h3>
                    <ul class="drop-list">
                      {#each group.drops as drop (`${drop.item_id}:${drop.variant}`)}
                        <li style={`--rarity-color:${rarityAccent(drop.rarity)}`}>
                          <a
                            href={itemHref(drop.name_ko, drop.item_id)}
                            class:no-media={!mediaAvailable(drop.image_path)}
                          >
                            {#if mediaAvailable(drop.image_path)}
                              <img
                                src={drop.image_path}
                                alt=""
                                onerror={() => markMediaFailed(drop.image_path)}
                              />
                            {/if}
                            <span><strong>{drop.name_ko}</strong></span>
                          </a>
                          <span class="drop-values"
                            ><strong>{probabilityLabel(drop.probability_ppm)}</strong><small
                              >{quantityLabel(drop.minimum_quantity, drop.maximum_quantity)}</small
                            ></span
                          >
                        </li>
                      {/each}
                    </ul>
                  </section>
                {/each}
              </div>
            </section>
          {/if}
        </div>
      </aside>
    {/if}
  </div>
</section>

<style>
  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
  }

  .pal-catalog-screen {
    padding: 14px clamp(12px, 2vw, 28px) 28px;
  }

  .pal-explorer {
    display: grid;
    min-height: clamp(560px, calc(100dvh - 144px), 780px);
    max-width: 1280px;
    grid-template-columns: minmax(0, 1fr) minmax(320px, 360px);
    margin: 0 auto;
    overflow: hidden;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--ink);
  }

  .pal-index {
    display: grid;
    min-width: 0;
    min-height: 0;
    grid-template-rows: auto auto minmax(0, 1fr);
    border-right: 1px solid var(--border);
    background: var(--ink);
  }

  .index-heading {
    display: flex;
    min-height: 58px;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 0 14px;
    border-bottom: 1px solid var(--border);
  }

  .index-heading h1 {
    margin: 0;
    font-size: 1.2rem;
  }

  .index-heading span {
    color: var(--muted-strong);
    font-size: 0.76rem;
    font-variant-numeric: tabular-nums;
  }

  .index-title,
  .index-actions {
    display: flex;
    min-width: 0;
    align-items: center;
    gap: 12px;
  }

  .index-actions {
    justify-content: flex-end;
  }

  .pal-tools {
    position: relative;
    display: grid;
    grid-template-columns: minmax(210px, 1fr) auto;
    gap: 8px;
    padding: 10px;
    border-bottom: 1px solid var(--border);
    background: var(--sidebar);
  }

  .search-field {
    position: relative;
  }

  .search-field input,
  .index-actions select,
  .filter-panel select,
  .filter-panel input[type='number'] {
    min-width: 0;
    min-height: 42px;
    border: 1px solid var(--border-strong);
    border-radius: 6px;
    color: var(--text-soft);
    background: var(--void);
    font-size: 0.78rem;
  }

  .search-field input {
    width: 100%;
    padding: 0 56px 0 12px;
  }

  .search-field button {
    position: absolute;
    top: 5px;
    right: 5px;
    min-height: 32px;
    padding: 0 8px;
    border: 0;
    border-radius: 4px;
    color: var(--muted-strong);
    background: var(--surface-raised);
    cursor: pointer;
    font-size: 0.7rem;
  }

  .tool-actions,
  .filter-actions,
  .view-switch {
    display: flex;
    min-width: 0;
    align-items: center;
    gap: 6px;
  }

  .tool-actions {
    justify-content: flex-end;
  }

  .filter-actions button,
  .view-switch button,
  .active-filters button,
  .filter-panel button {
    min-height: 42px;
    padding: 0 11px;
    border: 1px solid var(--border-strong);
    border-radius: 5px;
    color: var(--text-soft);
    background: var(--void);
    cursor: pointer;
    font-size: 0.74rem;
    font-weight: 760;
  }

  .filter-actions button.active,
  .view-switch button.active {
    border-color: color-mix(in srgb, var(--accent), white 8%);
    color: var(--text);
    background: color-mix(in srgb, var(--accent), var(--void) 88%);
  }

  .index-actions select {
    width: 142px;
    padding: 0 8px;
  }

  .view-switch {
    gap: 0;
    padding: 2px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--ink);
  }

  .view-switch button {
    min-height: 36px;
    padding-inline: 9px;
    border: 0;
    background: transparent;
  }

  .active-filters {
    display: flex;
    min-width: 0;
    grid-column: 1 / -1;
    flex-wrap: wrap;
    gap: 5px;
  }

  .active-filters button {
    min-height: 30px;
    padding-inline: 8px;
    color: var(--muted-strong);
    background: var(--surface);
    font-size: 0.68rem;
  }

  .active-filters .reset-all {
    margin-left: auto;
    color: color-mix(in srgb, var(--accent), white 20%);
  }

  .filter-panel {
    position: absolute;
    z-index: 20;
    top: calc(100% - 2px);
    right: 10px;
    width: min(700px, calc(100% - 20px));
    padding: 12px;
    border: 1px solid var(--border-strong);
    border-radius: 7px;
    background: var(--sidebar);
    box-shadow: 0 14px 30px color-mix(in srgb, black, transparent 42%);
  }

  .filter-panel > header,
  .filter-panel > footer {
    display: flex;
    min-height: 38px;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }

  .filter-panel > header {
    margin-bottom: 10px;
  }

  .filter-panel > header strong {
    font-size: 0.9rem;
  }

  .filter-panel > header button,
  .filter-panel > footer button {
    min-height: 34px;
    background: var(--surface);
  }

  .filter-panel > footer {
    margin-top: 12px;
    padding-top: 10px;
    border-top: 1px solid var(--border);
    color: var(--muted-strong);
    font-size: 0.74rem;
  }

  .filter-sections {
    display: grid;
    max-height: min(520px, calc(100dvh - 260px));
    gap: 14px;
    overflow: auto;
    padding-right: 4px;
    scrollbar-gutter: stable;
  }

  .filter-sections > section {
    min-width: 0;
    padding-top: 12px;
    border-top: 1px solid var(--border);
  }

  .filter-sections > section:first-child {
    padding-top: 0;
    border-top: 0;
  }

  .filter-sections h3 {
    margin: 0 0 9px;
    color: var(--muted-strong);
    font-size: 0.76rem;
  }

  .filter-sections > section.active-section h3 {
    color: color-mix(in srgb, var(--accent), white 18%);
  }

  .choice-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(112px, 1fr));
    gap: 6px;
  }

  .choice-grid label,
  .boolean-filter {
    position: relative;
    display: flex;
    min-width: 0;
    min-height: 42px;
    align-items: center;
    gap: 8px;
    padding: 6px 9px;
    border: 1px solid var(--border);
    border-radius: 5px;
    color: var(--text-soft);
    background: var(--void);
    cursor: pointer;
    font-size: 0.75rem;
  }

  .choice-grid label.checked,
  .boolean-filter:has(input:checked) {
    border-color: var(--accent);
    color: var(--text);
    background: color-mix(in srgb, var(--accent), var(--void) 90%);
  }

  .choice-grid label:has(input:focus-visible),
  .boolean-filter:has(input:focus-visible) {
    outline: 2px solid var(--focus);
    outline-offset: 2px;
  }

  .choice-grid input,
  .boolean-filter input,
  .work-filter-head input {
    width: 17px;
    height: 17px;
    flex: none;
    accent-color: var(--accent);
  }

  .choice-grid img {
    width: 24px;
    height: 24px;
    flex: none;
    object-fit: contain;
  }

  .work-filter-head {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(160px, auto);
    align-items: end;
    gap: 12px;
    margin-bottom: 10px;
  }

  .work-filter-head fieldset {
    display: flex;
    min-width: 0;
    flex-wrap: wrap;
    gap: 8px 16px;
    margin: 0;
    padding: 0;
    border: 0;
  }

  .work-filter-head legend,
  .level-filter > span,
  .minimum-stats span {
    width: 100%;
    margin-bottom: 6px;
    color: var(--muted);
    font-size: 0.68rem;
  }

  .work-filter-head fieldset label {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    color: var(--text-soft);
    font-size: 0.75rem;
  }

  .level-filter,
  .minimum-stats label {
    display: grid;
  }

  .level-filter select {
    width: 100%;
    padding-inline: 8px;
  }

  .detail-filters {
    display: grid;
    gap: 12px;
  }

  .boolean-filter {
    width: fit-content;
    padding-right: 14px;
  }

  .minimum-stats {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: 8px;
  }

  .minimum-stats input {
    width: 100%;
    padding-inline: 10px;
  }

  .result-region {
    min-height: 0;
    overflow: auto;
    overscroll-behavior: contain;
    scrollbar-gutter: stable;
  }

  .load-more {
    width: calc(100% - 20px);
    min-height: 46px;
    margin: 10px;
    border: 1px solid var(--border-strong);
    border-radius: 6px;
    color: var(--text-soft);
    background: var(--surface);
    cursor: pointer;
    font-size: 0.78rem;
    font-weight: 760;
  }

  .load-more:hover,
  .load-more:focus-visible {
    border-color: var(--accent);
    color: var(--text);
  }

  .virtual-canvas {
    position: relative;
    width: 100%;
  }

  .virtual-row {
    position: absolute;
    top: 0;
    left: 0;
    width: 100%;
  }

  .pal-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(170px, 1fr));
    align-content: start;
    gap: 10px;
    padding: 10px;
  }

  .pal-card {
    min-width: 0;
    overflow: visible;
    border: 1px solid var(--border);
    border-radius: 7px;
    background: var(--ink);
  }

  .pal-card:hover {
    border-color: var(--border-strong);
    background: var(--surface);
  }

  .pal-card.selected {
    border-color: var(--accent);
    box-shadow: inset 0 0 0 1px var(--accent);
  }

  .pal-card-main,
  .pal-row-main {
    width: 100%;
    border: 0;
    color: var(--text-soft);
    background: transparent;
    cursor: pointer;
    text-align: left;
  }

  .pal-card-main {
    display: grid;
    padding: 0;
  }

  .card-media {
    display: grid;
    width: 100%;
    aspect-ratio: 1.35;
    place-items: center;
    overflow: hidden;
    border-bottom: 1px solid var(--border);
    background: color-mix(in srgb, var(--surface), var(--void) 48%);
  }

  .card-media img {
    width: auto;
    height: auto;
    max-width: 72%;
    object-fit: contain;
  }

  .card-copy {
    display: grid;
    min-width: 0;
    gap: 4px;
    padding: 10px 11px 8px;
  }

  .card-copy > small {
    color: color-mix(in srgb, var(--accent), white 22%);
    font-size: 0.66rem;
    font-weight: 800;
    font-variant-numeric: tabular-nums;
  }

  .card-copy > strong {
    overflow: hidden;
    color: var(--text);
    font-size: 0.94rem;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .owned-mark {
    width: fit-content;
    color: var(--success);
    font-size: 0.68rem;
    font-weight: 800;
  }

  .pal-card footer {
    min-height: 43px;
    padding: 6px 9px;
    border-top: 1px solid var(--border);
  }

  .list-column-head {
    position: sticky;
    z-index: 4;
    top: 0;
    display: grid;
    min-height: 34px;
    grid-template-columns: minmax(250px, 1fr) minmax(145px, auto) minmax(188px, auto);
    align-items: center;
    gap: 12px;
    padding: 0 12px;
    border-bottom: 1px solid var(--border-strong);
    color: var(--muted);
    background: var(--sidebar);
    font-size: 0.64rem;
  }

  .metric-head {
    display: grid;
    grid-template-columns: repeat(3, minmax(52px, 1fr));
    gap: 8px;
    text-align: right;
  }

  .pal-row {
    display: grid;
    width: 100%;
    min-height: 86px;
    grid-template-columns: minmax(250px, 1fr) minmax(145px, auto) minmax(188px, auto);
    align-items: center;
    gap: 12px;
    padding: 8px 12px;
    border-bottom: 1px solid color-mix(in srgb, var(--border), transparent 28%);
    color: var(--text-soft);
    background: transparent;
  }

  .pal-row:hover {
    background: var(--surface);
  }

  .pal-row.selected {
    color: var(--text);
    background: color-mix(in srgb, var(--accent), var(--ink) 91%);
    box-shadow: inset 3px 0 0 var(--accent);
  }

  .pal-row-main {
    display: grid;
    min-width: 0;
    grid-template-columns: 58px minmax(0, 1fr);
    align-items: center;
    gap: 10px;
    padding: 0;
  }

  .pal-row.no-media .pal-row-main {
    grid-template-columns: minmax(0, 1fr);
  }

  .row-media {
    display: grid;
    width: 54px;
    height: 54px;
    place-items: center;
    overflow: hidden;
    border: 1px solid var(--border);
    border-radius: 6px;
    color: var(--muted);
    background: var(--surface);
    font-size: 0.62rem;
    text-align: center;
  }

  .row-media img {
    width: 100%;
    height: 100%;
    object-fit: contain;
  }

  .row-copy {
    display: grid;
    min-width: 0;
    gap: 4px;
  }

  .row-heading {
    display: flex;
    min-width: 0;
    align-items: baseline;
    justify-content: space-between;
    gap: 8px;
  }

  .row-heading strong {
    min-width: 0;
    overflow: hidden;
    color: var(--text);
    font-size: 0.9rem;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .row-heading small {
    flex: none;
    color: var(--muted);
    font-size: 0.69rem;
    font-variant-numeric: tabular-nums;
  }

  .row-labels {
    display: inline-flex;
    flex: none;
    align-items: center;
    gap: 8px;
  }

  .element-marks {
    display: flex;
    min-width: 0;
    flex-wrap: wrap;
    align-items: center;
    gap: 4px 8px;
    color: var(--muted-strong);
    font-size: 0.69rem;
  }

  .element-marks > span {
    display: inline-flex;
    align-items: center;
    gap: 3px;
  }

  .element-marks img {
    width: 13px;
    height: 13px;
    object-fit: contain;
  }

  .row-work-cell {
    min-width: 0;
  }

  .work-indicators {
    display: flex;
    min-width: 0;
    flex-wrap: wrap;
    align-items: center;
    gap: 5px;
  }

  .work-chip {
    position: relative;
    display: inline-flex;
  }

  .work-chip > button {
    position: relative;
    display: grid;
    width: 32px;
    height: 32px;
    place-items: center;
    padding: 3px;
    border: 1px solid var(--border);
    border-radius: 5px;
    color: var(--text-soft);
    background: var(--surface);
    cursor: pointer;
  }

  .work-chip > button:hover,
  .work-chip > button:focus-visible,
  .work-chip > button.pinned {
    border-color: var(--accent);
    color: var(--text);
  }

  .work-chip img {
    width: 22px;
    height: 22px;
    object-fit: contain;
  }

  .work-chip b {
    position: absolute;
    right: -3px;
    bottom: -3px;
    display: grid;
    min-width: 15px;
    height: 15px;
    place-items: center;
    border: 1px solid var(--border-strong);
    border-radius: 50%;
    color: var(--text);
    background: var(--void);
    font-size: 0.57rem;
  }

  .work-letter {
    font-size: 0.68rem;
    font-weight: 800;
  }

  .work-chip > button.more-work {
    font-size: 0.67rem;
    font-weight: 800;
  }

  .work-tooltip {
    position: absolute;
    z-index: 30;
    bottom: calc(100% + 7px);
    left: 50%;
    display: none;
    width: max-content;
    max-width: 240px;
    padding: 6px 8px;
    border: 1px solid var(--border-strong);
    border-radius: 4px;
    color: var(--text);
    background: var(--void);
    box-shadow: 0 7px 16px color-mix(in srgb, black, transparent 40%);
    font-size: 0.67rem;
    line-height: 1.45;
    transform: translateX(-50%);
    pointer-events: none;
  }

  .work-chip:hover .work-tooltip,
  .work-chip:focus-within .work-tooltip,
  .work-tooltip.pinned {
    display: block;
  }

  .row-metrics {
    display: grid;
    grid-template-columns: repeat(3, minmax(52px, 1fr));
    gap: 8px;
    margin: 0;
  }

  .row-metrics div {
    min-width: 0;
    text-align: right;
  }

  .row-metrics dt {
    color: var(--muted);
    font-size: 0.63rem;
  }

  .row-metrics dd {
    margin: 2px 0 0;
    color: var(--text);
    font-size: 0.82rem;
    font-weight: 800;
    font-variant-numeric: tabular-nums;
  }

  .empty-state {
    display: grid;
    min-height: 220px;
    place-content: center;
    justify-items: center;
    gap: 14px;
    padding: 24px;
    color: var(--text-soft);
    text-align: center;
  }

  .empty-state button {
    min-height: 44px;
    padding: 0 16px;
    border: 1px solid var(--border-strong);
    border-radius: 6px;
    color: var(--text);
    background: var(--surface-raised);
    cursor: pointer;
  }

  .pal-detail {
    min-width: 0;
    min-height: 0;
    overflow: auto;
    overscroll-behavior: contain;
    background: var(--void);
    scrollbar-gutter: stable;
  }

  .mobile-back {
    display: none;
  }

  .pal-hero {
    position: relative;
    display: grid;
    min-height: 210px;
    grid-template-columns: 140px minmax(0, 1fr);
    align-items: center;
    gap: 16px;
    padding: 20px;
    border-bottom: 1px solid var(--border);
    background: var(--ink);
  }

  .pal-hero.no-media {
    grid-template-columns: minmax(0, 1fr);
  }

  .workspace-detail-action {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 10px;
    padding: 8px 12px;
    border-bottom: 1px solid var(--border);
    background: var(--ink);
  }

  .owned-detail {
    margin-right: auto;
    color: var(--success);
    font-size: 0.76rem;
  }

  .plan-link {
    display: inline-flex;
    min-height: 36px;
    align-items: center;
    justify-content: center;
    padding-inline: 11px;
    border: 1px solid var(--accent);
    border-radius: 5px;
    color: var(--void);
    background: var(--accent);
    font-size: 0.75rem;
    font-weight: 800;
    text-decoration: none;
  }

  .hero-corner {
    position: absolute;
    width: 20px;
    height: 20px;
    border-color: var(--accent);
    border-style: solid;
    pointer-events: none;
  }

  .hero-corner.top-left {
    top: 14px;
    left: 14px;
    border-width: 2px 0 0 2px;
  }

  .hero-corner.bottom-right {
    right: 14px;
    bottom: 14px;
    border-width: 0 2px 2px 0;
  }

  .hero-media {
    display: grid;
    width: 140px;
    aspect-ratio: 1;
    place-items: center;
    overflow: hidden;
    border: 1px solid var(--border-strong);
    border-radius: 7px;
    color: var(--muted-strong);
    background: var(--surface);
    box-shadow: 5px 5px 0 color-mix(in srgb, var(--accent), transparent 72%);
  }

  .hero-media img {
    width: auto;
    height: auto;
    max-width: calc(100% - 8px);
    object-fit: contain;
  }

  .hero-identity {
    display: grid;
    min-width: 0;
    gap: 6px;
  }

  .hero-identity > small {
    color: color-mix(in srgb, var(--accent), white 24%);
    font-size: 0.76rem;
    font-weight: 800;
    letter-spacing: 0.08em;
  }

  .hero-identity h2 {
    margin: 0 0 8px;
    overflow-wrap: anywhere;
    font-size: clamp(1.75rem, 3vw, 2.35rem);
    line-height: 1.05;
  }

  .element-list {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .element-list > span {
    display: inline-flex;
    min-height: 30px;
    align-items: center;
    gap: 5px;
    padding: 0 9px;
    border: 1px solid var(--border);
    border-radius: 5px;
    color: var(--text-soft);
    background: var(--surface);
    font-size: 0.76rem;
  }

  .element-list img {
    width: 17px;
    height: 17px;
    object-fit: contain;
  }

  .stat-strip {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    margin: 0;
    border-bottom: 1px solid var(--border);
  }

  .stat-strip > div {
    padding: 13px 18px;
    border-right: 1px solid var(--border);
  }

  .stat-strip > div:last-child {
    border-right: 0;
  }

  .stat-strip dt {
    color: var(--muted);
    font-size: 0.75rem;
  }

  .stat-strip dd {
    margin: 3px 0 0;
    color: var(--text);
    font-size: 1.35rem;
    font-weight: 800;
    font-variant-numeric: tabular-nums;
  }

  .detail-tabs {
    display: flex;
    min-width: 0;
    overflow-x: auto;
    border-bottom: 1px solid var(--border);
    scrollbar-width: thin;
  }

  .detail-tabs button {
    position: relative;
    min-width: 82px;
    min-height: 48px;
    flex: 1;
    border: 0;
    color: var(--muted-strong);
    background: transparent;
    cursor: pointer;
    font-size: 0.8rem;
    font-weight: 760;
  }

  .detail-tabs button.active {
    color: var(--text);
  }

  .detail-tabs button.active::after {
    position: absolute;
    right: 22%;
    bottom: 0;
    left: 22%;
    height: 3px;
    background: var(--accent);
    content: '';
  }

  .detail-content {
    padding: clamp(18px, 3vw, 28px);
  }

  .overview-panel p {
    max-width: 720px;
    margin: 18px 0 0;
    padding-top: 16px;
    border-top: 1px solid var(--border);
    color: var(--text-soft);
    font-size: 0.92rem;
    line-height: 1.8;
    white-space: pre-line;
  }

  .profile-facts {
    display: grid;
    grid-template-columns: minmax(0, 1.5fr) repeat(2, minmax(62px, 0.75fr));
    gap: 14px;
    margin: 0;
  }

  .profile-facts div {
    display: grid;
    align-content: start;
    gap: 5px;
  }

  .profile-facts dt {
    color: var(--muted);
    font-size: 0.75rem;
  }

  .profile-facts dd {
    margin: 0;
    color: var(--text);
    font-size: 0.86rem;
    font-weight: 800;
  }

  .work-grid,
  .skill-list,
  .drop-list {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .work-grid {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
  }

  .work-grid li {
    display: grid;
    min-height: 54px;
    grid-template-columns: 32px minmax(0, 1fr) auto;
    align-items: center;
    gap: 8px;
    border-bottom: 1px solid var(--border);
  }

  .work-grid li.no-media {
    grid-template-columns: minmax(0, 1fr) auto;
  }

  .work-grid img,
  .drop-list img {
    width: 30px;
    height: 30px;
    object-fit: contain;
  }

  .work-grid strong {
    font-size: 0.82rem;
  }

  .work-grid b {
    color: color-mix(in srgb, var(--accent), white 24%);
    font-size: 0.76rem;
  }

  .skill-list li,
  .drop-list li {
    display: grid;
    min-height: 62px;
    grid-template-columns: minmax(0, 1fr) auto;
    align-items: center;
    gap: 14px;
    padding: 8px 0;
    border-bottom: 1px solid var(--border);
  }

  .skill-list li {
    min-height: 68px;
    grid-template-columns: 42px 32px minmax(0, 1fr);
    gap: 8px;
  }

  .skill-list li.no-media {
    grid-template-columns: 42px minmax(0, 1fr);
  }

  .skill-level {
    color: var(--muted-strong);
    font-size: 0.75rem;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }

  .skill-level b {
    color: var(--text);
  }

  .skill-list img {
    width: 32px;
    height: 32px;
    object-fit: contain;
  }

  .skill-copy,
  .drop-list a > span {
    display: grid;
    gap: 3px;
  }

  .skill-meta {
    display: flex;
    flex-wrap: wrap;
    gap: 2px 10px;
    color: var(--muted);
    font-size: 0.75rem;
  }

  .drop-list small,
  .drop-values small {
    color: var(--muted);
    font-size: 0.75rem;
  }

  .drop-values {
    display: flex;
    flex-wrap: wrap;
    justify-content: flex-end;
    gap: 5px 12px;
    text-align: right;
  }

  .drop-values strong {
    color: var(--text);
  }

  .drop-groups {
    display: grid;
    gap: 22px;
  }

  .drop-group h3 {
    margin: 0 0 6px;
    color: var(--text-soft);
    font-size: 0.78rem;
  }

  .drop-list li {
    box-shadow: inset 2px 0 0 var(--rarity-color);
    padding-left: 10px;
  }

  .drop-list a {
    display: grid;
    min-width: 0;
    grid-template-columns: 34px minmax(0, 1fr);
    align-items: center;
    gap: 10px;
    text-decoration: none;
  }

  .drop-list a.no-media {
    grid-template-columns: minmax(0, 1fr);
  }

  @media (min-width: 1024px) {
    .pal-catalog-screen {
      padding: 0;
    }

    .pal-explorer {
      height: calc(100dvh - 112px);
      min-height: 680px;
      max-width: none;
      grid-template-columns: minmax(0, 1fr);
      grid-template-rows: minmax(0, 1fr) auto;
      border: 0;
      border-radius: 0;
    }

    .pal-index {
      min-height: 0;
      grid-template-columns: 304px minmax(0, 1fr);
      grid-template-rows: 58px minmax(0, 1fr);
      border-right: 0;
    }

    .index-heading {
      grid-column: 2;
      grid-row: 1;
      padding-inline: 20px;
    }

    .index-heading h1 {
      color: var(--accent);
      font-size: 1rem;
    }

    .pal-tools {
      display: grid;
      min-height: 0;
      grid-column: 1;
      grid-row: 1 / span 2;
      grid-template-columns: minmax(0, 1fr);
      align-content: start;
      gap: 12px;
      overflow-y: auto;
      padding: 18px;
      border-right: 1px solid var(--border);
      border-bottom: 0;
      scrollbar-gutter: stable;
    }

    .tool-actions {
      display: block;
    }

    .filter-actions {
      display: none;
    }

    .active-filters {
      margin-bottom: 2px;
    }

    .active-filters .reset-all {
      margin-left: 0;
    }

    .filter-panel {
      position: static;
      width: auto;
      padding: 0;
      border: 0;
      background: transparent;
      box-shadow: none;
    }

    .filter-panel > header,
    .filter-panel > footer {
      display: none;
    }

    .filter-sections {
      max-height: none;
      gap: 18px;
      overflow: visible;
      padding: 0;
    }

    .filter-sections > section,
    .filter-sections > section:first-child {
      display: block;
      padding: 0 0 18px;
      border-top: 0;
      border-bottom: 1px solid var(--border);
    }

    .filter-sections h3,
    .filter-sections > section.active-section h3 {
      margin-bottom: 12px;
      color: var(--text-soft);
      font-size: 0.8rem;
    }

    .choice-grid {
      grid-template-columns: repeat(2, minmax(0, 1fr));
    }

    .element-choices {
      grid-template-columns: repeat(4, minmax(0, 1fr));
    }

    .element-choices label {
      justify-content: center;
      padding-inline: 5px;
    }

    .element-choices label span {
      display: none;
    }

    .element-choices input,
    .work-choices input {
      position: absolute;
      inset: 0;
      z-index: 1;
      width: 100%;
      height: 100%;
      margin: 0;
      opacity: 0;
      cursor: pointer;
    }

    .work-filter-head {
      grid-template-columns: minmax(0, 1fr);
    }

    .minimum-stats {
      grid-template-columns: minmax(0, 1fr);
    }

    .boolean-filter {
      width: 100%;
    }

    .result-region,
    .empty-state {
      min-height: 0;
      grid-column: 2;
      grid-row: 2;
    }

    .pal-grid {
      grid-template-columns: repeat(6, minmax(120px, 1fr));
      gap: 8px;
      padding: 10px 18px 18px;
    }

    .pal-card {
      border-radius: 3px;
    }

    .pal-card.selected {
      box-shadow: inset 0 0 0 1px var(--accent);
    }

    .pal-detail {
      display: grid;
      max-height: 226px;
      grid-column: 1;
      grid-row: 2;
      grid-template-columns: minmax(290px, 0.9fr) minmax(250px, 0.72fr) minmax(360px, 1.4fr);
      grid-template-rows: 48px minmax(0, 1fr);
      overflow: hidden;
      border-top: 1px solid var(--accent);
      background: var(--ink);
    }

    .pal-hero {
      min-height: 0;
      grid-column: 1;
      grid-row: 1 / span 2;
      grid-template-columns: 112px minmax(0, 1fr);
      gap: 14px;
      padding: 14px 18px;
      border-right: 1px solid var(--border);
      border-bottom: 0;
    }

    .hero-media {
      width: 108px;
    }

    .hero-identity h2 {
      font-size: 1.55rem;
    }

    .stat-strip {
      grid-column: 2;
      grid-row: 1 / span 2;
      grid-template-columns: minmax(0, 1fr);
      align-content: center;
      border-right: 1px solid var(--border);
      border-bottom: 0;
    }

    .stat-strip > div,
    .stat-strip > div:last-child {
      display: flex;
      min-height: 58px;
      align-items: center;
      justify-content: space-between;
      padding: 10px 20px;
      border-right: 0;
      border-bottom: 1px solid var(--border);
    }

    .stat-strip > div:last-child {
      border-bottom: 0;
    }

    .stat-strip dd {
      font-size: 1.05rem;
    }

    .detail-tabs {
      grid-column: 3;
      grid-row: 1;
    }

    .detail-tabs button {
      min-height: 48px;
    }

    .detail-content {
      min-height: 0;
      grid-column: 3;
      grid-row: 2;
      overflow: auto;
      padding: 14px 18px;
    }

    .overview-panel p {
      margin-top: 10px;
      padding-top: 10px;
      font-size: 0.82rem;
      line-height: 1.55;
    }

    .profile-facts {
      grid-template-columns: minmax(0, 1.5fr) repeat(2, minmax(72px, 0.7fr));
    }

    .work-grid {
      grid-template-columns: repeat(2, minmax(0, 1fr));
      column-gap: 16px;
    }
  }

  @media (max-width: 1023px) {
    .pal-catalog-screen {
      padding-inline: clamp(10px, 2vw, 18px);
    }

    .pal-explorer {
      display: block;
      min-height: calc(100dvh - 150px);
    }

    .pal-index {
      min-height: calc(100dvh - 150px);
      border-right: 0;
    }

    .pal-tools {
      grid-template-columns: minmax(0, 1fr);
    }

    .tool-actions {
      justify-content: flex-start;
      flex-wrap: wrap;
    }

    .filter-panel {
      right: auto;
      left: 10px;
    }

    .pal-detail {
      min-height: calc(100dvh - 150px);
      overflow: visible;
    }

    .pal-catalog-screen.detail-open .pal-index {
      display: none;
    }

    .pal-catalog-screen:not(.detail-open) .pal-detail {
      display: none;
    }

    .mobile-back {
      display: flex;
      position: sticky;
      z-index: 3;
      top: 112px;
      width: 100%;
      min-height: 46px;
      align-items: center;
      padding: 0 14px;
      border: 0;
      border-bottom: 1px solid var(--border);
      color: var(--text-soft);
      background: var(--sidebar);
      cursor: pointer;
      font-size: 0.78rem;
      font-weight: 760;
    }

    .result-region {
      overflow: visible;
    }
  }

  @media (max-width: 719px) {
    .pal-catalog-screen {
      padding: 0 0 18px;
    }

    .mobile-back {
      top: calc(102px + env(safe-area-inset-top));
    }

    .pal-explorer {
      min-height: calc(100dvh - 168px);
      border-right: 0;
      border-left: 0;
      border-radius: 0;
    }

    .pal-index,
    .pal-detail {
      min-height: calc(100dvh - 168px);
    }

    .index-heading {
      display: grid;
      min-height: 102px;
      grid-template-columns: repeat(auto-fit, minmax(min(100%, 10rem), 1fr));
      align-content: center;
      padding: 8px 12px;
    }

    .index-title {
      justify-content: space-between;
    }

    .index-actions {
      display: grid;
      width: 100%;
      grid-template-columns: repeat(auto-fit, minmax(min(100%, 8rem), 1fr));
      gap: 6px;
    }

    .index-actions label,
    .index-actions select,
    .index-actions .view-switch {
      width: 100%;
    }

    .index-actions .view-switch button {
      flex: 1;
    }

    .index-actions {
      width: 100%;
      justify-content: space-between;
    }

    .pal-tools {
      padding: 8px;
    }

    .tool-actions {
      display: block;
    }

    .filter-actions {
      display: flex;
      flex-wrap: wrap;
    }

    .index-actions select {
      width: 142px;
    }

    .filter-panel {
      right: 8px;
      left: 8px;
      width: auto;
    }

    .work-filter-head {
      grid-template-columns: minmax(0, 1fr);
    }

    .pal-grid {
      grid-template-columns: repeat(auto-fit, minmax(min(100%, 8rem), 1fr));
      gap: 7px;
      padding: 8px;
    }

    .pal-row {
      min-height: 96px;
      grid-template-columns: minmax(0, 1fr);
      gap: 7px;
      padding-inline: 10px;
    }

    .list-column-head {
      display: none;
    }

    .row-work-cell {
      padding-left: 68px;
    }

    .pal-row.no-media .row-work-cell {
      padding-left: 0;
    }

    .row-metrics {
      display: none;
    }

    .row-heading strong {
      white-space: normal;
    }

    .pal-hero {
      min-height: 180px;
      grid-template-columns: minmax(92px, 32%) minmax(0, 1fr);
      gap: 16px;
      padding: 20px 16px;
    }

    .pal-hero.no-media {
      grid-template-columns: minmax(0, 1fr);
    }

    .hero-media {
      width: 100%;
      max-width: 132px;
      box-shadow: 3px 3px 0 color-mix(in srgb, var(--accent), transparent 72%);
    }

    .hero-identity h2 {
      font-size: clamp(1.65rem, 8vw, 2.35rem);
      overflow-wrap: anywhere;
    }

    .stat-strip > div {
      padding: 10px 12px;
      text-align: center;
    }

    .stat-strip dd {
      font-size: 1.15rem;
    }

    .detail-tabs button {
      min-width: 88px;
    }

    .detail-content {
      padding: 16px 14px 30px;
    }

    .work-grid {
      grid-template-columns: minmax(0, 1fr);
    }

    .skill-list li {
      grid-template-columns: 42px 32px minmax(0, 1fr);
    }
  }

  @media (max-width: 420px) {
    .detail-tabs button {
      min-width: 76px;
    }

    .filter-actions {
      display: grid;
      grid-template-columns: repeat(auto-fit, minmax(min(100%, 4rem), 1fr));
      width: 100%;
    }

    .filter-actions button {
      min-width: 0;
      padding-inline: 5px;
    }

    .choice-grid {
      grid-template-columns: repeat(2, minmax(0, 1fr));
    }

    .minimum-stats {
      grid-template-columns: minmax(0, 1fr);
    }

    .card-copy {
      padding-inline: 8px;
    }

    .pal-card footer {
      padding-inline: 6px;
    }

    .work-grid {
      grid-template-columns: 1fr;
    }

    .drop-list li {
      grid-template-columns: 1fr;
    }

    .drop-values {
      justify-content: flex-start;
      padding-left: 44px;
      text-align: left;
    }
  }
</style>
