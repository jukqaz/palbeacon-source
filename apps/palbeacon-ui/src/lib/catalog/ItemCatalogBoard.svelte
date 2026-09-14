<script lang="ts">
  import { onMount, tick, untrack } from 'svelte';
  import { get } from 'svelte/store';
  import { resolve } from '$app/paths';
  import { createVirtualizer } from '@tanstack/svelte-virtual';
  import { normalizeSearchText } from '$lib/shared/search/search-core';
  import WorkspaceAction from '$lib/shared/workspace/WorkspaceAction.svelte';
  import { rememberWorkspaceEntry } from '$lib/shared/workspace/workspace';
  import {
    composeItemRelations,
    filterItems,
    itemDisplayDescription,
    itemMetricValue,
    itemRarityLabel,
    itemRouteLabels,
    rarityAccent,
    type ItemCatalogSort,
    type ItemRouteFilter,
  } from './catalog';
  import type { CatalogRecord, UnifiedCatalog } from './types';

  interface Props {
    catalog: UnifiedCatalog;
    title?: string;
    initialQuery?: string;
    initialSelectedId?: string;
  }

  type ItemDetailTab = 'info' | 'recipe' | 'drop' | 'shop' | 'technology';
  type ItemViewMode = 'grid' | 'list';

  let {
    catalog,
    title = '아이템 도감',
    initialQuery = '',
    initialSelectedId = '',
  }: Props = $props();
  let query = $state(untrack(() => initialQuery));
  let selectedCategories = $state<string[]>([]);
  let selectedRarities = $state<string[]>([]);
  let selectedRoutes = $state<ItemRouteFilter[]>([]);
  let sort = $state<ItemCatalogSort>('name');
  let viewMode = $state<ItemViewMode>('grid');
  let filtersOpen = $state(false);
  let selectedId = $state<string | null>(null);
  let activeTab = $state<ItemDetailTab>('info');
  let wide = $state(false);
  let mobileDetailOpen = $state(false);
  let narrowLimit = $state(24);
  let relationLimit = $state(18);
  let narrowFilterSignature = $state('');
  let appliedInitialState = $state<string | null>(null);
  let listScroller = $state<HTMLDivElement | null>(null);
  let lastListTrigger = $state<HTMLButtonElement | null>(null);
  let failedMedia = $state(new Set<string>());

  const items = $derived(
    catalog.records.items.filter(
      (item) => !item.localization_fallback && item.name_ko.trim().length > 0,
    ),
  );
  const itemRelations = $derived(catalog.item_relations ?? {});
  const categoryOptions = $derived.by(() => {
    const counts = new Map<string, number>();
    for (const item of items) counts.set(item.category, (counts.get(item.category) ?? 0) + 1);
    return [...counts.entries()].toSorted((left, right) => left[0].localeCompare(right[0], 'ko'));
  });
  const rarityOptions = $derived.by(() => {
    const order = ['일반', '비범', '희귀', '영웅', '전설'];
    const counts = new Map<string, number>();
    for (const item of items) {
      const rarity = itemRarityLabel(item);
      if (rarity) counts.set(rarity, (counts.get(rarity) ?? 0) + 1);
    }
    return order.flatMap((label) => {
      const count = counts.get(label);
      return count === undefined ? [] : [[label, count] as const];
    });
  });
  const routeOptions = $derived.by(() => {
    const options: ItemRouteFilter[] = ['recipe', 'drop', 'shop', 'technology'];
    return options.map(
      (route) =>
        [
          route,
          items.filter((item) => {
            const relations = itemRelations[item.id];
            if (!relations) return false;
            if (route === 'recipe') return relations.recipes.length > 0;
            if (route === 'drop') return relations.drops.length > 0;
            if (route === 'shop') return relations.shops.length > 0;
            return relations.technologies.length > 0;
          }).length,
        ] as const,
    );
  });
  const filteredItems = $derived(
    filterItems(items, itemRelations, {
      query,
      categories: selectedCategories,
      rarities: selectedRarities,
      routes: selectedRoutes,
      sort,
    }),
  );
  const narrowItems = $derived(filteredItems.slice(0, narrowLimit));
  const gridPageSize = $derived(wide ? 60 : 24);
  const visibleGridItems = $derived(filteredItems.slice(0, Math.max(narrowLimit, gridPageSize)));
  const selected = $derived(
    selectedId === null ? null : (items.find((item) => item.id === selectedId) ?? null),
  );
  const selectedRelations = $derived(selected ? composeItemRelations(catalog, selected.id) : null);
  const selectedMetrics = $derived(
    selected?.metrics.filter((metric) => ['기준가', '최대 묶음', '무게'].includes(metric.label)) ??
      [],
  );
  const visibleTabs = $derived.by(() => {
    if (!selected || !selectedRelations) return [];
    return [
      { id: 'info' as const, label: '정보' },
      ...(selectedRelations.recipes.length > 0
        ? [
            {
              id: 'recipe' as const,
              label: `제작 ${selectedRelations.recipes.length.toLocaleString('ko-KR')}`,
            },
          ]
        : []),
      ...(selectedRelations.drops.length > 0
        ? [
            {
              id: 'drop' as const,
              label: `드롭 ${selectedRelations.drops.length.toLocaleString('ko-KR')}`,
            },
          ]
        : []),
      ...(selectedRelations.shops.length > 0
        ? [
            {
              id: 'shop' as const,
              label: `상점 ${selectedRelations.shops.length.toLocaleString('ko-KR')}`,
            },
          ]
        : []),
      ...(selectedRelations.technologies.length > 0
        ? [
            {
              id: 'technology' as const,
              label: `기술 ${selectedRelations.technologies.length.toLocaleString('ko-KR')}`,
            },
          ]
        : []),
    ];
  });
  const activeFilterCount = $derived(
    selectedCategories.length + selectedRarities.length + selectedRoutes.length,
  );

  const virtualizer = createVirtualizer<HTMLDivElement, HTMLDivElement>({
    count: 0,
    getScrollElement: () => listScroller,
    estimateSize: () => 74,
    overscan: 8,
  });

  const exactRecordForEntry = (displayQuery: string, entityId: string) => {
    const normalizedId = normalizeSearchText(entityId);
    if (normalizedId.length > 0) {
      const idMatch = items.find((item) => normalizeSearchText(item.id) === normalizedId);
      if (idMatch) return idMatch;
    }
    const normalizedQuery = normalizeSearchText(displayQuery);
    if (normalizedQuery.length === 0) return null;
    const exactMatches = items.filter(
      (item) =>
        normalizeSearchText(item.name_ko) === normalizedQuery ||
        normalizeSearchText(item.id) === normalizedQuery,
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
    selectedCategories = [];
    selectedRarities = [];
    selectedRoutes = [];
    sort = 'name';
    const exact = exactRecordForEntry(displayQuery, entityId);
    selectedId = exact?.id ?? null;
    activeTab = 'info';
    mobileDetailOpen = exact !== null;
  });

  $effect(() => {
    const scroller = listScroller;
    const count = wide && viewMode === 'list' ? filteredItems.length : 0;
    untrack(() => {
      get(virtualizer).setOptions({
        count,
        getScrollElement: () => scroller,
        estimateSize: () => 82,
        overscan: 8,
      });
    });
  });

  $effect(() => {
    const results = filteredItems;
    if (selectedId !== null && !results.some((item) => item.id === selectedId)) {
      selectedId = null;
      activeTab = 'info';
      mobileDetailOpen = false;
    }
  });

  $effect(() => {
    if (!visibleTabs.some((tab) => tab.id === activeTab)) activeTab = 'info';
  });

  $effect(() => {
    const signature = [
      query,
      selectedCategories.join(','),
      selectedRarities.join(','),
      selectedRoutes.join(','),
      sort,
      viewMode,
    ].join('\u0000');
    if (signature === narrowFilterSignature) return;
    narrowFilterSignature = signature;
    narrowLimit = 24;
  });

  onMount(() => {
    const media = window.matchMedia('(min-width: 1024px)');
    const update = () => {
      const wasWide = wide;
      wide = media.matches;
      if (wasWide && !wide) mobileDetailOpen = false;
      if (wide) filtersOpen = false;
    };
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return;
      if (filtersOpen) {
        filtersOpen = false;
        return;
      }
      if (mobileDetailOpen) void closeMobileDetail();
    };
    update();
    media.addEventListener('change', update);
    window.addEventListener('keydown', closeOnEscape);
    return () => {
      media.removeEventListener('change', update);
      window.removeEventListener('keydown', closeOnEscape);
    };
  });

  const mediaAvailable = (path: string | null | undefined): path is string =>
    Boolean(path) && !failedMedia.has(path as string);

  const markMediaFailed = (path: string | null | undefined) => {
    if (!path || failedMedia.has(path)) return;
    failedMedia = new Set([...failedMedia, path]);
  };

  const toggleChoice = <T extends string>(values: T[], value: T): T[] =>
    values.includes(value) ? values.filter((entry) => entry !== value) : [...values, value];

  const resetFilters = () => {
    query = '';
    selectedCategories = [];
    selectedRarities = [];
    selectedRoutes = [];
    sort = 'name';
    filtersOpen = false;
    mobileDetailOpen = false;
  };

  const selectItem = (item: CatalogRecord, trigger: HTMLButtonElement) => {
    lastListTrigger = trigger;
    selectedId = item.id;
    activeTab = 'info';
    relationLimit = 18;
    mobileDetailOpen = !wide;
    rememberWorkspaceEntry({
      kind: 'item',
      id: item.id,
      name_ko: item.name_ko,
      href: queryHref('/items/', item.name_ko, item.id),
      image_path: item.image_path,
    });
  };

  const closeDetail = async () => {
    if (wide) selectedId = null;
    else mobileDetailOpen = false;
    await tick();
    lastListTrigger?.focus();
  };

  const closeMobileDetail = async () => {
    mobileDetailOpen = false;
    await tick();
    lastListTrigger?.focus();
  };

  const queryHref = (
    path: '/items/' | '/pals/' | '/technology/',
    displayQuery: string,
    entityId: string,
  ) => {
    const parameters = new URLSearchParams({ q: displayQuery, id: entityId });
    return `${resolve(path, {})}?${parameters.toString()}`;
  };

  const quantityLabel = (minimum: number, maximum: number) =>
    minimum === maximum
      ? `${minimum.toLocaleString('ko-KR')}개`
      : `${minimum.toLocaleString('ko-KR')}–${maximum.toLocaleString('ko-KR')}개`;

  const probabilityLabel = (probabilityPpm: number) => {
    if (probabilityPpm >= 1_000_000) return '확정';
    return `${new Intl.NumberFormat('ko-KR', { maximumFractionDigits: 2 }).format(
      probabilityPpm / 10_000,
    )}%`;
  };

  const routeBadges = (item: CatalogRecord) => {
    const relations = itemRelations[item.id];
    if (!relations) return [];
    const badges: string[] = [];
    if (relations.recipes.length > 0) badges.push('제작');
    const dropCount = new Set(relations.drops.map((drop) => drop.pal_id)).size;
    if (dropCount > 0) badges.push(`드롭 ${dropCount.toLocaleString('ko-KR')}종`);
    const shopCount = new Set(relations.shops.map((shop) => shop.shop_group_id)).size;
    if (shopCount > 0) badges.push(`상점 ${shopCount.toLocaleString('ko-KR')}곳`);
    const unlockLevel = relations.technologies.reduce<number | null>(
      (earliest, technology) =>
        earliest === null ? technology.level : Math.min(earliest, technology.level),
      null,
    );
    if (unlockLevel !== null) badges.push(`Lv. ${unlockLevel.toLocaleString('ko-KR')} 해금`);
    return badges;
  };

  const itemAriaLabel = (item: CatalogRecord) =>
    [item.name_ko, item.category, itemRarityLabel(item), ...routeBadges(item), '아이템']
      .filter(Boolean)
      .join(', ');

  const listMetric = (item: CatalogRecord, label: string) => itemMetricValue(item, label);

  const showMoreItems = () => {
    narrowLimit = Math.max(narrowLimit, gridPageSize) + gridPageSize;
  };
</script>

{#snippet filterControls(surface: 'rail' | 'sheet')}
  <div class="filter-group">
    <h2>분류</h2>
    <div class="filter-choices category-choices">
      {#each categoryOptions as [category, count] (category)}
        <label class:checked={selectedCategories.includes(category)}>
          <input
            type="checkbox"
            aria-label={category}
            checked={selectedCategories.includes(category)}
            onchange={() => (selectedCategories = toggleChoice(selectedCategories, category))}
          />
          <span>{category}</span><small>{count.toLocaleString('ko-KR')}</small>
        </label>
      {/each}
    </div>
  </div>
  <div class="filter-group">
    <h2>희귀도</h2>
    <div class="filter-choices rarity-choices">
      {#each rarityOptions as [rarity, count] (rarity)}
        <label class:checked={selectedRarities.includes(rarity)}>
          <input
            type="checkbox"
            aria-label={rarity}
            checked={selectedRarities.includes(rarity)}
            onchange={() => (selectedRarities = toggleChoice(selectedRarities, rarity))}
          />
          <i
            style:background={rarityAccent(
              items.find((item) => itemRarityLabel(item) === rarity)?.rarity,
            )}
          ></i>
          <span>{rarity}</span><small>{count.toLocaleString('ko-KR')}</small>
        </label>
      {/each}
    </div>
  </div>
  <div class="filter-group">
    <h2>얻는 법</h2>
    <div class="filter-choices route-choices">
      {#each routeOptions as [route, count] (route)}
        <label class:checked={selectedRoutes.includes(route)}>
          <input
            type="checkbox"
            aria-label={itemRouteLabels[route]}
            checked={selectedRoutes.includes(route)}
            onchange={() => (selectedRoutes = toggleChoice(selectedRoutes, route))}
          />
          <span>{itemRouteLabels[route]}</span><small>{count.toLocaleString('ko-KR')}</small>
        </label>
      {/each}
    </div>
  </div>
  {#if activeFilterCount > 0 || query.length > 0 || surface === 'sheet'}
    <div class="filter-footer">
      {#if activeFilterCount > 0 || query.length > 0}
        <button type="button" onclick={resetFilters}>전체 초기화</button>
      {/if}
      {#if surface === 'sheet'}
        <button
          class="apply-filter"
          type="button"
          aria-label="결과 보기"
          onclick={() => (filtersOpen = false)}
          >{filteredItems.length.toLocaleString('ko-KR')}개 보기</button
        >
      {/if}
    </div>
  {/if}
{/snippet}

{#snippet itemRow(item: CatalogRecord)}
  {@const rarity = itemRarityLabel(item)}
  {@const badges = routeBadges(item)}
  <button
    type="button"
    class="item-row"
    class:selected={selectedId === item.id}
    class:no-media={!mediaAvailable(item.image_path)}
    aria-label={itemAriaLabel(item)}
    aria-pressed={selectedId === item.id}
    onclick={(event) => selectItem(item, event.currentTarget)}
  >
    {#if mediaAvailable(item.image_path)}
      <span class="item-media" style:border-color={rarityAccent(item.rarity)}>
        <img src={item.image_path} alt="" onerror={() => markMediaFailed(item.image_path)} />
      </span>
    {/if}
    <span class="item-copy">
      <span class="item-eyebrow">
        <span>{item.category}</span>
        {#if rarity}<i style:background={rarityAccent(item.rarity)}></i><span>{rarity}</span>{/if}
      </span>
      <strong>{item.name_ko}</strong>
    </span>
    <span class="route-summary" aria-label="얻는 법">
      {#each badges.slice(0, 3) as badge (badge)}<span>{badge}</span>{/each}
    </span>
    <dl class="row-metrics" aria-label="아이템 수치">
      {#if listMetric(item, '기준가')}
        <div>
          <dt>기준가</dt>
          <dd>{listMetric(item, '기준가')}</dd>
        </div>
      {/if}
      {#if listMetric(item, '최대 묶음')}
        <div>
          <dt>묶음</dt>
          <dd>{listMetric(item, '최대 묶음')}</dd>
        </div>
      {/if}
      {#if listMetric(item, '무게')}
        <div>
          <dt>무게</dt>
          <dd>{listMetric(item, '무게')}</dd>
        </div>
      {/if}
    </dl>
  </button>
{/snippet}

{#snippet itemCard(item: CatalogRecord)}
  {@const rarity = itemRarityLabel(item)}
  {@const badges = routeBadges(item)}
  <article
    class="item-card"
    class:selected={selectedId === item.id}
    class:no-media={!mediaAvailable(item.image_path)}
    style:border-top-color={rarityAccent(item.rarity)}
  >
    <button
      type="button"
      class="item-card-main"
      aria-label={itemAriaLabel(item)}
      aria-pressed={selectedId === item.id}
      onclick={(event) => selectItem(item, event.currentTarget)}
    >
      {#if mediaAvailable(item.image_path)}
        <span class="card-media">
          <img
            src={item.image_path}
            alt=""
            loading="lazy"
            onerror={() => markMediaFailed(item.image_path)}
          />
        </span>
      {/if}
      <span class="card-copy">
        <span class="item-eyebrow">
          <span>{item.category}</span>
          {#if rarity}<i style:background={rarityAccent(item.rarity)}></i><span>{rarity}</span>{/if}
        </span>
        <strong>{item.name_ko}</strong>
      </span>
      {#if badges.length > 0}
        <span class="card-routes" aria-label="얻는 법">
          {#each badges.slice(0, 2) as badge (badge)}<span>{badge}</span>{/each}
          {#if badges.length > 2}<span>외 {badges.length - 2}</span>{/if}
        </span>
      {/if}
    </button>
  </article>
{/snippet}

<section class:detail-open={mobileDetailOpen && selected !== null} class="item-catalog-screen">
  <div class:with-detail={selected !== null} class="item-explorer">
    <aside class="filter-rail" aria-label="아이템 필터">
      <header><strong>필터</strong></header>
      {@render filterControls('rail')}
    </aside>

    <section class="item-index" aria-label="아이템 목록">
      <header class="index-heading">
        <div class="index-title">
          <h1>{title}</h1>
          <span aria-live="polite">{filteredItems.length.toLocaleString('ko-KR')}개</span>
        </div>
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
      </header>

      <div class="item-tools">
        <label class="search-field">
          <span class="sr-only">{title} 검색</span>
          <input
            type="search"
            bind:value={query}
            aria-label={`${title} 검색`}
            placeholder="검색"
            autocomplete="off"
          />
          {#if query.length > 0}
            <button type="button" aria-label="검색어 지우기" onclick={() => (query = '')}
              >지우기</button
            >
          {/if}
        </label>
        <label class="sort-field">
          <span class="sr-only">아이템 정렬</span>
          <select bind:value={sort} aria-label="아이템 정렬">
            <option value="name">이름순</option>
            <option value="rarity">희귀도 높은순</option>
            <option value="price_desc">기준가 높은순</option>
            <option value="price_asc">기준가 낮은순</option>
          </select>
        </label>
        <button
          class:active={activeFilterCount > 0}
          class="filter-trigger"
          type="button"
          aria-expanded={filtersOpen}
          aria-controls="item-filter-sheet"
          onclick={() => (filtersOpen = !filtersOpen)}
          >필터{activeFilterCount > 0 ? ` ${activeFilterCount.toString()}` : ''}</button
        >
      </div>

      {#if activeFilterCount > 0}
        <div class="active-filters" aria-label="적용 중인 필터">
          {#if selectedCategories.length > 0}
            <button type="button" onclick={() => (selectedCategories = [])}
              >분류 {selectedCategories.length} ×</button
            >
          {/if}
          {#if selectedRarities.length > 0}
            <button type="button" onclick={() => (selectedRarities = [])}
              >희귀도 {selectedRarities.length} ×</button
            >
          {/if}
          {#if selectedRoutes.length > 0}
            <button type="button" onclick={() => (selectedRoutes = [])}
              >얻는 법 {selectedRoutes.length} ×</button
            >
          {/if}
          <button class="reset-filter" type="button" onclick={resetFilters}>초기화</button>
        </div>
      {/if}

      {#if filtersOpen}
        <aside id="item-filter-sheet" class="filter-sheet" aria-label="아이템 필터 선택">
          <header>
            <strong>필터</strong>
            <button type="button" aria-label="필터 닫기" onclick={() => (filtersOpen = false)}
              >닫기</button
            >
          </header>
          <div class="filter-sheet-body">{@render filterControls('sheet')}</div>
        </aside>
      {/if}

      {#if filteredItems.length === 0}
        <div class="empty-state">
          <strong>조건에 맞는 아이템이 없습니다.</strong>
          <button type="button" onclick={resetFilters}>검색과 필터 초기화</button>
        </div>
      {:else}
        <div class="result-region" aria-label="아이템 도감 결과" bind:this={listScroller}>
          {#if viewMode === 'list'}
            <div class="column-head" aria-hidden="true">
              <span>아이템</span><span>얻는 법</span><span>기준가 · 묶음 · 무게</span>
            </div>
          {/if}
          {#if viewMode === 'grid'}
            <div class="item-grid">
              {#each visibleGridItems as item (item.id)}{@render itemCard(item)}{/each}
            </div>
            {#if visibleGridItems.length < filteredItems.length}
              <button class="load-more" type="button" onclick={showMoreItems}
                >다음 {Math.min(
                  gridPageSize,
                  filteredItems.length - visibleGridItems.length,
                ).toString()}개 보기</button
              >
            {/if}
          {:else if wide}
            <div
              class="virtual-canvas"
              style:height={`${$virtualizer.getTotalSize().toString()}px`}
            >
              {#each $virtualizer.getVirtualItems() as row (row.key)}
                {@const item = filteredItems[row.index]}
                {#if item}
                  <div
                    class="virtual-row"
                    data-index={row.index}
                    use:$virtualizer.measureElement
                    style:transform={`translateY(${row.start.toString()}px)`}
                  >
                    {@render itemRow(item)}
                  </div>
                {/if}
              {/each}
            </div>
          {:else}
            <div class="item-list">
              {#each narrowItems as item (item.id)}{@render itemRow(item)}{/each}
            </div>
            {#if narrowItems.length < filteredItems.length}
              <button class="load-more" type="button" onclick={() => (narrowLimit += 24)}
                >다음 {Math.min(24, filteredItems.length - narrowItems.length).toString()}개 보기</button
              >
            {/if}
          {/if}
        </div>
      {/if}
    </section>

    {#if selected && selectedRelations}
      <aside class="item-detail" aria-label="선택한 아이템 상세">
        <button
          class="mobile-back"
          type="button"
          aria-label="아이템 목록"
          onclick={closeMobileDetail}>← 목록</button
        >
        <header class:no-media={!mediaAvailable(selected.image_path)} class="detail-hero">
          {#if mediaAvailable(selected.image_path)}
            <span class="detail-media" style:border-color={rarityAccent(selected.rarity)}>
              <img
                src={selected.image_path}
                alt=""
                onerror={() => markMediaFailed(selected.image_path)}
              />
            </span>
          {/if}
          <div>
            <span class="detail-category">{selected.category}</span>
            <h2>{selected.name_ko}</h2>
            {#if itemRarityLabel(selected)}
              <span class="detail-rarity"
                ><i style:background={rarityAccent(selected.rarity)}></i>{itemRarityLabel(
                  selected,
                )}</span
              >
            {/if}
          </div>
          <button
            class="close-detail"
            type="button"
            aria-label="아이템 상세 닫기"
            onclick={closeDetail}>×</button
          >
        </header>

        <div class="workspace-detail-action">
          <WorkspaceAction
            entry={{
              kind: 'item',
              id: selected.id,
              name_ko: selected.name_ko,
              href: queryHref('/items/', selected.name_ko, selected.id),
              image_path: selected.image_path,
            }}
          />
        </div>

        <div class="detail-tabs" role="tablist" aria-label="아이템 상세 정보">
          {#each visibleTabs as tab (tab.id)}
            <button
              type="button"
              role="tab"
              aria-selected={activeTab === tab.id}
              class:active={activeTab === tab.id}
              onclick={() => {
                activeTab = tab.id;
                relationLimit = 18;
              }}>{tab.label}</button
            >
          {/each}
        </div>

        <div class="detail-body">
          {#if activeTab === 'info'}
            {#if itemDisplayDescription(selected)}
              <p class="detail-description">{itemDisplayDescription(selected)}</p>
            {/if}
            {#if selectedMetrics.length > 0}
              <dl class="detail-metrics">
                {#each selectedMetrics as metric (metric.label)}
                  <div>
                    <dt>{metric.label}</dt>
                    <dd>{metric.value}</dd>
                  </div>
                {/each}
              </dl>
            {/if}
          {:else if activeTab === 'recipe'}
            <section class="detail-section">
              <h3>만드는 법</h3>
              <ul class="recipe-list">
                {#each selectedRelations.recipes as recipe (recipe.key)}
                  <li>
                    <strong>{recipe.outputQuantity.toLocaleString('ko-KR')}개 제작</strong>
                    {#if recipe.unlockItem}
                      <a
                        class="recipe-requirement"
                        href={queryHref('/items/', recipe.unlockItem.name_ko, recipe.unlockItem.id)}
                      >
                        {#if mediaAvailable(recipe.unlockItem.image_path)}
                          <img
                            src={recipe.unlockItem.image_path}
                            alt=""
                            onerror={() => markMediaFailed(recipe.unlockItem?.image_path ?? null)}
                          />
                        {/if}
                        <span><small>필요 설계도</small>{recipe.unlockItem.name_ko}</span>
                      </a>
                    {/if}
                    <div class="ingredient-list">
                      {#each recipe.ingredients as ingredient (`${recipe.key}:${ingredient.id}`)}
                        <a href={queryHref('/items/', ingredient.name, ingredient.id)}>
                          {#if mediaAvailable(ingredient.imagePath)}
                            <img
                              src={ingredient.imagePath}
                              alt=""
                              onerror={() => markMediaFailed(ingredient.imagePath)}
                            />
                          {/if}
                          <span>{ingredient.name}</span><b
                            >× {ingredient.quantity.toLocaleString('ko-KR')}</b
                          >
                        </a>
                      {/each}
                    </div>
                  </li>
                {/each}
              </ul>
            </section>
          {:else if activeTab === 'drop'}
            <section class="detail-section">
              <h3>얻는 법</h3>
              <ul class="drop-list">
                {#each selectedRelations.drops.slice(0, relationLimit) as drop (drop.key)}
                  <li>
                    <a href={queryHref('/pals/', drop.pal.name_ko, drop.pal.id)}>
                      {#if mediaAvailable(drop.pal.image_path)}
                        <img
                          src={drop.pal.image_path}
                          alt=""
                          onerror={() => markMediaFailed(drop.pal.image_path)}
                        />
                      {/if}
                      <span
                        ><strong>{drop.pal.name_ko}</strong>{#if drop.variant === 'boss'}<small
                            >보스</small
                          >{/if}</span
                      >
                      <b
                        >{probabilityLabel(drop.probabilityPpm)} · {quantityLabel(
                          drop.minimumQuantity,
                          drop.maximumQuantity,
                        )}</b
                      >
                    </a>
                  </li>
                {/each}
              </ul>
              {#if relationLimit < selectedRelations.drops.length}
                <button class="load-relations" type="button" onclick={() => (relationLimit += 18)}
                  >다음 {Math.min(18, selectedRelations.drops.length - relationLimit).toString()}종
                  보기</button
                >
              {/if}
            </section>
          {:else if activeTab === 'shop'}
            <section class="detail-section">
              <h3>구매</h3>
              <ul class="shop-list">
                {#each selectedRelations.shops as offer (offer.key)}
                  <li>
                    <span
                      ><strong
                        >{offer.currencyName} 상점 {offer.shopCount.toLocaleString(
                          'ko-KR',
                        )}곳</strong
                      ><small>{offer.quantity.toLocaleString('ko-KR')}개</small></span
                    >
                    <b>{offer.price.toLocaleString('ko-KR')} {offer.currencyName}</b>
                    {#if offer.stock !== null}<small
                        >재고 {offer.stock.toLocaleString('ko-KR')}</small
                      >{/if}
                  </li>
                {/each}
              </ul>
            </section>
          {:else if activeTab === 'technology'}
            <section class="detail-section">
              <h3>해금 기술</h3>
              <ul class="technology-list">
                {#each selectedRelations.technologies as entry (entry.technology.id)}
                  <li>
                    <a
                      href={queryHref(
                        '/technology/',
                        entry.technology.name_ko,
                        entry.technology.id,
                      )}
                    >
                      {#if mediaAvailable(entry.technology.image_path)}
                        <img
                          src={entry.technology.image_path}
                          alt=""
                          onerror={() => markMediaFailed(entry.technology.image_path)}
                        />
                      {/if}
                      <span>{entry.technology.name_ko}</span>
                      <b
                        >Lv. {entry.level.toLocaleString('ko-KR')} · {entry.cost.toLocaleString(
                          'ko-KR',
                        )} PT</b
                      >
                    </a>
                  </li>
                {/each}
              </ul>
            </section>
          {/if}
        </div>
      </aside>
    {/if}
  </div>
</section>

<style>
  .item-catalog-screen {
    min-width: 0;
    padding: 18px clamp(12px, 1.8vw, 28px) 20px;
  }

  .item-explorer {
    display: grid;
    max-width: 1440px;
    min-width: 0;
    min-height: calc(100dvh - 168px);
    margin: 0 auto;
    grid-template-columns: 208px minmax(0, 1fr);
    gap: 12px;
  }

  .item-explorer.with-detail {
    grid-template-columns: 208px minmax(440px, 1fr) minmax(320px, 360px);
  }

  .filter-rail,
  .item-index,
  .item-detail,
  .filter-sheet {
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--ink);
  }

  .filter-rail {
    align-self: start;
    position: sticky;
    top: 12px;
    max-height: calc(100dvh - 150px);
    overflow: auto;
  }

  .filter-rail > header,
  .filter-sheet > header {
    display: flex;
    min-height: 48px;
    align-items: center;
    justify-content: space-between;
    padding: 0 12px;
    border-bottom: 1px solid var(--border);
  }

  .filter-rail > header strong,
  .filter-sheet > header strong {
    font-size: 0.92rem;
  }

  .filter-group {
    padding: 12px;
    border-bottom: 1px solid var(--border);
  }

  .filter-group h2 {
    margin: 0 0 8px;
    color: var(--muted-strong);
    font-size: 0.75rem;
    letter-spacing: 0.04em;
  }

  .filter-choices {
    display: grid;
    gap: 3px;
  }

  .filter-choices label {
    display: grid;
    min-height: 34px;
    align-items: center;
    grid-template-columns: 20px minmax(0, 1fr) auto;
    gap: 7px;
    padding: 4px 6px;
    border: 1px solid transparent;
    border-radius: 5px;
    color: var(--text-soft);
    cursor: pointer;
    font-size: 0.82rem;
  }

  .filter-choices label:has(i) {
    grid-template-columns: 20px 7px minmax(0, 1fr) auto;
  }

  .filter-choices label:hover,
  .filter-choices label.checked {
    border-color: var(--border-strong);
    background: var(--surface);
  }

  .filter-choices input {
    width: 18px;
    height: 18px;
    margin: 0;
    accent-color: var(--accent);
  }

  .filter-choices i,
  .detail-rarity i,
  .item-eyebrow i {
    display: inline-block;
    width: 7px;
    height: 7px;
    border-radius: 50%;
  }

  .filter-choices small {
    color: var(--muted);
    font-size: 0.72rem;
  }

  .filter-rail .filter-choices {
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 5px;
  }

  .filter-rail .filter-choices label,
  .filter-rail .filter-choices label:has(i) {
    position: relative;
    min-height: 36px;
    grid-template-columns: minmax(0, 1fr);
    justify-items: center;
    gap: 4px;
    padding: 5px;
    text-align: center;
  }

  .filter-rail .filter-choices label:has(i) {
    grid-template-columns: 7px minmax(0, auto);
  }

  .filter-rail .filter-choices input {
    position: absolute;
    z-index: 1;
    inset: 0;
    width: auto;
    height: auto;
    margin: 0;
    cursor: pointer;
    opacity: 0;
  }

  .filter-rail .filter-choices small {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
  }

  .filter-rail .filter-choices label:has(input:focus-visible) {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }

  .filter-rail .filter-choices label.checked {
    border-color: var(--accent);
    color: var(--text);
    background: color-mix(in srgb, var(--accent), var(--surface) 91%);
  }

  .filter-rail .filter-choices label.checked::after {
    position: absolute;
    top: 2px;
    right: 4px;
    color: var(--accent);
    content: '✓';
    font-size: 0.62rem;
    font-weight: 900;
  }

  .filter-footer {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding: 12px;
    color: var(--muted-strong);
    font-size: 0.78rem;
  }

  .filter-footer button,
  .filter-sheet > header button,
  .active-filters button {
    min-height: 32px;
    padding: 5px 8px;
    border: 1px solid var(--border);
    border-radius: 5px;
    color: var(--text-soft);
    background: var(--surface);
    cursor: pointer;
    font-size: 0.76rem;
  }

  .item-index {
    min-width: 0;
    overflow: hidden;
  }

  .index-heading {
    display: flex;
    min-height: 56px;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    padding: 0 14px;
    border-bottom: 1px solid var(--border);
  }

  .index-title {
    display: flex;
    min-width: 0;
    align-items: baseline;
    gap: 9px;
  }

  .index-heading h1 {
    margin: 0;
    font-size: 1.15rem;
  }

  .index-title > span {
    color: var(--muted-strong);
    font-size: 0.82rem;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }

  .view-switch {
    display: inline-flex;
    overflow: hidden;
    flex: 0 0 auto;
    border: 1px solid var(--border-strong);
    border-radius: 6px;
    background: var(--surface);
  }

  .view-switch button {
    min-height: 36px;
    padding: 0 11px;
    border: 0;
    border-right: 1px solid var(--border);
    color: var(--muted-strong);
    background: transparent;
    cursor: pointer;
    font-size: 0.76rem;
    font-weight: 750;
  }

  .view-switch button:last-child {
    border-right: 0;
  }

  .view-switch button.active {
    color: var(--void);
    background: var(--accent);
  }

  .item-tools {
    display: grid;
    grid-template-columns: minmax(220px, 1fr) 164px;
    gap: 8px;
    padding: 10px;
    border-bottom: 1px solid var(--border);
    background: var(--surface);
  }

  .search-field {
    position: relative;
    min-width: 0;
  }

  .search-field input,
  .sort-field select {
    width: 100%;
    min-height: 42px;
    border: 1px solid var(--border-strong);
    border-radius: 6px;
    background: var(--ink);
  }

  .search-field input {
    padding: 8px 58px 8px 11px;
  }

  .search-field button {
    position: absolute;
    top: 6px;
    right: 6px;
    min-height: 30px;
    padding: 0 7px;
    border: 0;
    color: var(--muted-strong);
    background: transparent;
    cursor: pointer;
  }

  .sort-field select {
    padding: 7px 28px 7px 9px;
  }

  .filter-trigger,
  .filter-sheet {
    display: none;
  }

  .active-filters {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    padding: 8px 10px;
    border-bottom: 1px solid var(--border);
  }

  .active-filters .reset-filter {
    margin-left: auto;
    color: var(--accent);
  }

  .column-head {
    display: grid;
    min-height: 34px;
    align-items: center;
    grid-template-columns: minmax(240px, 1fr) minmax(150px, 0.62fr) minmax(190px, 0.72fr);
    gap: 12px;
    padding: 0 12px;
    border-bottom: 1px solid var(--border);
    color: var(--muted);
    font-size: 0.7rem;
    font-weight: 800;
  }

  .result-region {
    max-height: calc(100dvh - 292px);
    overflow: auto;
    scrollbar-gutter: stable;
  }

  .item-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(144px, 1fr));
    align-content: start;
    gap: 9px;
    padding: 10px;
  }

  .item-card {
    min-width: 0;
    overflow: hidden;
    border: 1px solid var(--border);
    border-top: 3px solid var(--border-strong);
    border-radius: 7px;
    background: var(--ink);
    transition:
      border-color 120ms var(--ease-standard),
      background 120ms var(--ease-standard);
  }

  .item-card:hover {
    border-right-color: var(--border-strong);
    border-bottom-color: var(--border-strong);
    border-left-color: var(--border-strong);
    background: var(--surface);
  }

  .item-card.selected {
    border-right-color: var(--accent);
    border-bottom-color: var(--accent);
    border-left-color: var(--accent);
    box-shadow: inset 0 0 0 1px var(--accent);
  }

  .item-card-main {
    display: grid;
    width: 100%;
    height: 100%;
    min-height: 184px;
    grid-template-rows: auto auto 1fr;
    padding: 0;
    border: 0;
    color: inherit;
    text-align: left;
    background: transparent;
    cursor: pointer;
  }

  .item-card.no-media .item-card-main {
    min-height: 104px;
    grid-template-rows: auto 1fr;
  }

  .card-media {
    display: grid;
    width: 100%;
    min-height: 106px;
    place-items: center;
    overflow: hidden;
    border-bottom: 1px solid var(--border);
    background: var(--surface);
  }

  .card-media img {
    width: 82px;
    height: 82px;
    object-fit: contain;
  }

  .card-copy {
    display: grid;
    min-width: 0;
    gap: 5px;
    padding: 9px 10px 7px;
  }

  .card-copy > strong {
    display: -webkit-box;
    overflow: hidden;
    color: var(--text);
    font-size: 0.88rem;
    line-height: 1.32;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
  }

  .card-copy .item-eyebrow {
    flex-wrap: wrap;
    row-gap: 2px;
  }

  .card-routes {
    display: flex;
    align-self: end;
    flex-wrap: wrap;
    gap: 4px;
    padding: 0 9px 9px;
  }

  .card-routes span {
    padding: 3px 6px;
    border: 1px solid var(--border);
    border-radius: 999px;
    color: var(--text-soft);
    background: var(--surface-raised);
    font-size: 0.68rem;
    white-space: nowrap;
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

  .item-row {
    display: grid;
    width: 100%;
    min-height: 74px;
    align-items: center;
    grid-template-columns: 54px minmax(180px, 1fr) minmax(150px, 0.62fr) minmax(190px, 0.72fr);
    gap: 12px;
    padding: 9px 12px;
    border: 0;
    border-bottom: 1px solid var(--border);
    color: inherit;
    text-align: left;
    background: transparent;
    cursor: pointer;
  }

  .item-row.no-media {
    grid-template-columns: minmax(240px, 1fr) minmax(150px, 0.62fr) minmax(190px, 0.72fr);
  }

  .item-row:hover,
  .item-row.selected {
    background: var(--surface);
  }

  .item-row.selected {
    box-shadow: inset 3px 0 0 var(--accent);
  }

  .item-media,
  .detail-media {
    display: grid;
    overflow: hidden;
    place-items: center;
    border: 1px solid var(--border-strong);
    border-radius: 6px;
    background: var(--surface-raised);
  }

  .item-media {
    width: 50px;
    height: 50px;
  }

  .item-media img,
  .detail-media img,
  .recipe-requirement img,
  .ingredient-list img,
  .drop-list img,
  .technology-list img {
    width: 100%;
    height: 100%;
    object-fit: contain;
  }

  .item-copy {
    display: grid;
    min-width: 0;
    gap: 3px;
  }

  .item-copy > strong {
    overflow: hidden;
    font-size: 0.91rem;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .item-eyebrow {
    display: flex;
    align-items: center;
    gap: 5px;
    color: var(--muted);
    font-size: 0.69rem;
  }

  .route-summary {
    display: flex;
    min-width: 0;
    flex-wrap: wrap;
    gap: 4px;
  }

  .route-summary span {
    padding: 3px 6px;
    border: 1px solid var(--border);
    border-radius: 999px;
    color: var(--text-soft);
    background: var(--surface-raised);
    font-size: 0.7rem;
    white-space: nowrap;
  }

  .row-metrics {
    display: grid;
    min-width: 0;
    margin: 0;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: 4px;
  }

  .row-metrics div {
    min-width: 0;
  }

  .row-metrics dt {
    color: var(--muted);
    font-size: 0.65rem;
  }

  .row-metrics dd {
    overflow: hidden;
    margin: 2px 0 0;
    color: var(--text-soft);
    font-size: 0.76rem;
    font-variant-numeric: tabular-nums;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .item-detail {
    align-self: start;
    position: sticky;
    top: 12px;
    max-height: calc(100dvh - 150px);
    overflow: auto;
    border: 1px solid var(--border-strong);
    border-radius: 8px;
    background: var(--surface-raised);
  }

  .mobile-back {
    display: none;
  }

  .detail-hero {
    display: grid;
    min-height: 94px;
    align-items: center;
    grid-template-columns: 70px minmax(0, 1fr) 36px;
    gap: 11px;
    padding: 12px;
    border-bottom: 1px solid var(--border);
  }

  .detail-hero.no-media {
    grid-template-columns: minmax(0, 1fr) 36px;
  }

  .workspace-detail-action {
    display: flex;
    justify-content: flex-end;
    padding: 8px 12px;
    border-bottom: 1px solid var(--border);
    background: var(--ink);
  }

  .detail-media {
    width: 68px;
    height: 68px;
  }

  .detail-hero h2 {
    margin: 2px 0 4px;
    font-size: 1.05rem;
  }

  .detail-category,
  .detail-rarity {
    color: var(--muted-strong);
    font-size: 0.74rem;
  }

  .detail-rarity {
    display: inline-flex;
    align-items: center;
    gap: 5px;
  }

  .close-detail {
    width: 34px;
    height: 34px;
    padding: 0;
    border: 1px solid var(--border);
    border-radius: 5px;
    color: var(--muted-strong);
    background: var(--surface);
    cursor: pointer;
    font-size: 1.25rem;
  }

  .detail-tabs {
    display: flex;
    overflow-x: auto;
    border-bottom: 1px solid var(--border);
    scrollbar-width: thin;
  }

  .detail-tabs button {
    min-height: 44px;
    flex: 0 0 auto;
    padding: 0 11px;
    border: 0;
    border-bottom: 2px solid transparent;
    color: var(--muted-strong);
    background: transparent;
    cursor: pointer;
    font-size: 0.76rem;
  }

  .detail-tabs button.active {
    border-bottom-color: var(--accent);
    color: var(--text);
  }

  .detail-body {
    padding: 13px;
  }

  .detail-description {
    margin: 0 0 13px;
    color: var(--text-soft);
    font-size: 0.85rem;
    line-height: 1.55;
  }

  .detail-metrics {
    display: grid;
    margin: 0;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    border: 1px solid var(--border);
    border-radius: 6px;
  }

  .detail-metrics div {
    min-width: 0;
    padding: 9px;
    border-right: 1px solid var(--border);
  }

  .detail-metrics div:last-child {
    border-right: 0;
  }

  .detail-metrics dt {
    color: var(--muted);
    font-size: 0.68rem;
  }

  .detail-metrics dd {
    overflow: hidden;
    margin: 4px 0 0;
    color: var(--text);
    font-size: 0.82rem;
    font-weight: 750;
    text-overflow: ellipsis;
  }

  .detail-section {
    margin-top: 14px;
  }

  .detail-section:first-child {
    margin-top: 0;
  }

  .detail-section h3 {
    margin: 0 0 8px;
    font-size: 0.88rem;
  }

  .recipe-list,
  .drop-list,
  .shop-list,
  .technology-list {
    display: grid;
    gap: 7px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .recipe-list > li,
  .drop-list li,
  .shop-list li,
  .technology-list li {
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--surface);
  }

  .recipe-list > li > strong {
    display: block;
    padding: 8px 9px;
    border-bottom: 1px solid var(--border);
    font-size: 0.75rem;
  }

  .ingredient-list {
    display: grid;
  }

  .ingredient-list a,
  .recipe-requirement,
  .drop-list a,
  .technology-list a {
    display: grid;
    min-height: 48px;
    align-items: center;
    grid-template-columns: 34px minmax(0, 1fr) auto;
    gap: 8px;
    padding: 6px 8px;
    color: inherit;
    text-decoration: none;
  }

  .ingredient-list a + a {
    border-top: 1px solid var(--border);
  }

  .recipe-requirement {
    border-bottom: 1px solid var(--border);
    color: var(--brass);
  }

  .recipe-requirement span {
    display: grid;
    gap: 2px;
  }

  .recipe-requirement small {
    color: var(--muted);
    font-size: 0.68rem;
  }

  .ingredient-list a:not(:has(img)),
  .drop-list a:not(:has(img)),
  .technology-list a:not(:has(img)) {
    grid-template-columns: minmax(0, 1fr) auto;
  }

  .ingredient-list img,
  .recipe-requirement img,
  .drop-list img,
  .technology-list img {
    width: 32px;
    height: 32px;
  }

  .ingredient-list span,
  .technology-list span,
  .drop-list strong {
    min-width: 0;
    overflow: hidden;
    font-size: 0.78rem;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .ingredient-list b,
  .drop-list b,
  .technology-list b,
  .shop-list b {
    color: var(--text-soft);
    font-size: 0.73rem;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }

  .drop-list a > span {
    display: grid;
    min-width: 0;
    gap: 2px;
  }

  .drop-list small,
  .shop-list small {
    color: var(--muted);
    font-size: 0.68rem;
  }

  .shop-list li {
    display: grid;
    min-height: 54px;
    align-items: center;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: 8px;
    padding: 8px 9px;
  }

  .shop-list li > span {
    display: grid;
    gap: 2px;
  }

  .shop-list li > span strong {
    font-size: 0.78rem;
  }

  .shop-list li > small {
    grid-column: 1 / -1;
  }

  .load-more,
  .load-relations {
    display: block;
    width: calc(100% - 20px);
    min-height: 44px;
    margin: 10px;
    border: 1px solid var(--border-strong);
    border-radius: 6px;
    color: var(--text-soft);
    background: var(--surface);
    cursor: pointer;
  }

  .load-relations {
    width: 100%;
    margin: 10px 0 0;
  }

  .empty-state {
    display: grid;
    min-height: 260px;
    place-content: center;
    justify-items: center;
    gap: 12px;
    padding: 24px;
    color: var(--muted-strong);
    text-align: center;
  }

  .empty-state button {
    min-height: 44px;
    padding: 0 12px;
    border: 1px solid var(--border-strong);
    border-radius: 6px;
    color: var(--text);
    background: var(--surface);
    cursor: pointer;
  }

  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    padding: 0;
    overflow: hidden;
    clip: rect(0 0 0 0);
    white-space: nowrap;
  }

  @media (max-width: 1280px) and (min-width: 1024px) {
    .item-explorer.with-detail {
      grid-template-columns: 188px minmax(390px, 1fr) 320px;
    }

    .filter-group {
      padding: 10px;
    }

    .item-explorer.with-detail .item-grid {
      grid-template-columns: repeat(auto-fill, minmax(132px, 1fr));
    }

    .item-row {
      grid-template-columns: 50px minmax(170px, 1fr) minmax(125px, 0.55fr) minmax(132px, 0.56fr);
      gap: 9px;
    }

    .item-row.no-media {
      grid-template-columns: minmax(210px, 1fr) minmax(125px, 0.55fr) minmax(132px, 0.56fr);
    }

    .item-explorer.with-detail .row-metrics div:nth-child(3),
    .item-explorer.with-detail .column-head > span:last-child {
      display: none;
    }

    .item-explorer.with-detail .row-metrics {
      grid-template-columns: repeat(2, minmax(0, 1fr));
    }
  }

  @media (max-width: 1023px) {
    .item-catalog-screen {
      padding: 14px 12px 34px;
    }

    .item-explorer,
    .item-explorer.with-detail {
      display: block;
      min-height: 0;
    }

    .filter-rail {
      display: none;
    }

    .filter-trigger {
      display: block;
      min-height: 42px;
      padding: 0 11px;
      border: 1px solid var(--border-strong);
      border-radius: 6px;
      color: var(--text-soft);
      white-space: nowrap;
      background: var(--ink);
      cursor: pointer;
    }

    .filter-trigger.active {
      border-color: var(--accent);
      color: var(--text);
    }

    .item-tools {
      grid-template-columns: minmax(0, 1fr) 160px auto;
    }

    .filter-sheet {
      display: block;
      position: relative;
      margin: 8px 10px;
      border-color: var(--border-strong);
      background: var(--surface-raised);
    }

    .filter-sheet-body {
      display: grid;
      grid-template-columns: repeat(3, minmax(0, 1fr));
    }

    .filter-sheet .filter-group {
      border-right: 1px solid var(--border);
      border-bottom: 0;
    }

    .filter-sheet .filter-group:nth-child(3) {
      border-right: 0;
    }

    .filter-sheet .filter-footer {
      grid-column: 1 / -1;
      border-top: 1px solid var(--border);
    }

    .filter-sheet .apply-filter {
      color: var(--void);
      background: var(--accent);
      font-weight: 800;
    }

    .result-region {
      max-height: none;
      overflow: visible;
    }

    .item-row {
      grid-template-columns: 54px minmax(190px, 1fr) minmax(150px, 0.62fr) minmax(180px, 0.7fr);
    }

    .item-row.no-media {
      grid-template-columns: minmax(230px, 1fr) minmax(150px, 0.62fr) minmax(180px, 0.7fr);
    }

    .item-detail {
      display: none;
      position: static;
      max-height: none;
    }

    .detail-open .item-index {
      display: none;
    }

    .detail-open .item-detail {
      display: block;
    }

    .mobile-back {
      display: block;
      width: 100%;
      min-height: 46px;
      padding: 0 13px;
      border: 0;
      border-bottom: 1px solid var(--border);
      color: var(--text-soft);
      text-align: left;
      background: var(--ink);
      cursor: pointer;
    }

    .close-detail {
      display: none;
    }
  }

  @media (max-width: 719px) {
    .item-catalog-screen {
      padding: 10px 8px 82px;
    }

    .index-heading {
      min-height: 52px;
      flex-wrap: wrap;
      padding: 8px 11px;
    }

    .index-heading h1 {
      font-size: 1.04rem;
    }

    .item-tools {
      grid-template-columns: minmax(0, 1fr) auto;
      padding: 8px;
    }

    .item-grid {
      grid-template-columns: repeat(2, minmax(0, 1fr));
      gap: 8px;
      padding: 8px;
    }

    .item-card-main {
      min-height: 176px;
    }

    .card-media {
      min-height: 96px;
    }

    .card-media img {
      width: 74px;
      height: 74px;
    }

    .sort-field {
      grid-column: 1 / -1;
      grid-row: 2;
    }

    .filter-sheet {
      margin: 7px 8px;
    }

    .filter-sheet-body {
      display: block;
      max-height: min(66dvh, 560px);
      overflow: auto;
    }

    .filter-sheet .filter-group {
      border-right: 0;
      border-bottom: 1px solid var(--border);
    }

    .filter-sheet .filter-footer {
      position: sticky;
      bottom: 0;
      background: var(--surface-raised);
    }

    .category-choices {
      grid-template-columns: repeat(2, minmax(0, 1fr));
    }

    .filter-choices label {
      min-height: 44px;
    }

    .column-head {
      display: none;
    }

    .item-row,
    .item-row.no-media {
      min-height: 92px;
      grid-template-columns: 54px minmax(0, 1fr);
      gap: 9px;
      padding: 10px;
    }

    .item-row.no-media {
      grid-template-columns: minmax(0, 1fr);
    }

    .route-summary,
    .row-metrics {
      grid-column: 2;
    }

    .item-row.no-media .route-summary,
    .item-row.no-media .row-metrics {
      grid-column: 1;
    }

    .route-summary span:nth-child(n + 3) {
      display: none;
    }

    .row-metrics {
      display: flex;
      gap: 12px;
    }

    .row-metrics div:nth-child(n + 3) {
      display: none;
    }

    .row-metrics dt,
    .row-metrics dd {
      display: inline;
    }

    .row-metrics dd {
      margin-left: 4px;
    }

    .detail-body {
      padding: 12px;
    }

    .ingredient-list a,
    .recipe-requirement,
    .drop-list a,
    .technology-list a {
      grid-template-columns: 34px minmax(0, 1fr);
    }

    .ingredient-list a > b,
    .drop-list a > b,
    .technology-list a > b {
      grid-column: 2;
      justify-self: start;
    }

    .ingredient-list a:not(:has(img)),
    .recipe-requirement:not(:has(img)),
    .drop-list a:not(:has(img)),
    .technology-list a:not(:has(img)) {
      grid-template-columns: minmax(0, 1fr);
    }

    .ingredient-list a:not(:has(img)) > b,
    .drop-list a:not(:has(img)) > b,
    .technology-list a:not(:has(img)) > b {
      grid-column: 1;
    }
  }

  @media (max-width: 380px) {
    .row-metrics {
      display: none;
    }

    .detail-metrics {
      grid-template-columns: 1fr;
    }

    .detail-metrics div {
      display: flex;
      justify-content: space-between;
      gap: 10px;
      border-right: 0;
      border-bottom: 1px solid var(--border);
    }

    .detail-metrics div:last-child {
      border-bottom: 0;
    }
  }
</style>
