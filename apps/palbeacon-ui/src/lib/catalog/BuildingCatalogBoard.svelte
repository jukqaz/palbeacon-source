<script lang="ts">
  import { onMount, tick, untrack } from 'svelte';
  import { resolve } from '$app/paths';
  import {
    ArrowLeft02Icon,
    BoxesIcon,
    Cancel01Icon,
    CatalogueIcon,
    Chair01Icon,
    ConstructionIcon,
    CookingPotIcon,
    Factory01Icon,
    Lamp01Icon,
    MonsterIcon,
    More01Icon,
    Shield02Icon,
    Wrench01Icon,
  } from '@hugeicons/core-free-icons';
  import { HugeiconsIcon } from '@hugeicons/svelte';
  import { normalizeSearchText } from '$lib/shared/search/search-core';
  import WorkspaceAction from '$lib/shared/workspace/WorkspaceAction.svelte';
  import { rememberWorkspaceEntry } from '$lib/shared/workspace/workspace';
  import {
    buildingCategoryOrder,
    buildingMetric,
    buildingSubcategoriesFor,
    buildingSubcategoryLabel,
    buildingUnlock,
    filterBuildings,
    groupBuildings,
    type BuildingCategory,
  } from './building-catalog';
  import type { CatalogRecord, UnifiedCatalog } from './types';
  import { safeGameDescription } from '$lib/shared/data/display-text';

  interface Props {
    catalog: UnifiedCatalog;
    title: string;
    initialQuery?: string;
    initialSelectedId?: string;
  }

  let { catalog, title, initialQuery = '', initialSelectedId = '' }: Props = $props();
  let query = $state(untrack(() => initialQuery));
  let category = $state<BuildingCategory | 'all'>('생산');
  let subcategory = $state('all');
  let selectedId = $state<string | null>(null);
  let compactDetail = $state(false);
  let selectedButton = $state<HTMLButtonElement | null>(null);
  let failedMedia = $state(new Set<string>());
  let appliedInitialState = $state<string | null>(null);

  const records = $derived(
    catalog.records.buildings.filter(
      (record) => !record.localization_fallback && record.name_ko.trim().length > 0,
    ),
  );
  const subcategories = $derived(
    category === 'all' ? [] : buildingSubcategoriesFor(records, category),
  );
  const filtered = $derived(filterBuildings(catalog, query, category, subcategory));
  const groups = $derived(groupBuildings(filtered, category));
  const selected = $derived(
    selectedId === null ? null : (records.find((record) => record.id === selectedId) ?? null),
  );
  const selectedUnlock = $derived(selected ? buildingUnlock(catalog, selected) : null);

  const categoryOptions = [
    { id: 'all', label: '전체', icon: CatalogueIcon },
    { id: '생산', label: '생산', icon: Factory01Icon },
    { id: '팰 시설', label: '팰 시설', icon: MonsterIcon },
    { id: '보관', label: '보관', icon: BoxesIcon },
    { id: '식량', label: '식량', icon: CookingPotIcon },
    { id: '기반 시설', label: '기반 시설', icon: Wrench01Icon },
    { id: '조명', label: '조명', icon: Lamp01Icon },
    { id: '토대', label: '토대', icon: ConstructionIcon },
    { id: '방어', label: '방어', icon: Shield02Icon },
    { id: '가구', label: '가구', icon: Chair01Icon },
    { id: '기타', label: '기타', icon: More01Icon },
  ] as const;

  const mediaAvailable = (path: string | null | undefined): path is string =>
    Boolean(path) && !failedMedia.has(path as string);

  const markMediaFailed = (path: string | null | undefined) => {
    if (!path || failedMedia.has(path)) return;
    failedMedia = new Set([...failedMedia, path]);
  };

  const selectCategory = (next: BuildingCategory | 'all') => {
    category = next;
    subcategory = 'all';
    selectedId = null;
  };

  const selectBuilding = (record: CatalogRecord, button: HTMLButtonElement) => {
    selectedButton = button;
    selectedId = record.id;
    rememberWorkspaceEntry({
      kind: 'building',
      id: record.id,
      name_ko: record.name_ko,
      href: buildingHref(record),
      image_path: record.image_path,
    });
  };

  const closeInspector = async () => {
    selectedId = null;
    await tick();
    selectedButton?.focus();
  };

  const reset = () => {
    query = '';
    category = '생산';
    subcategory = 'all';
    selectedId = null;
  };

  const technologyHref = (record: CatalogRecord, technologyId: string) => {
    const parameters = new URLSearchParams({ q: record.name_ko, id: technologyId });
    return `${resolve('/technology/', {})}?${parameters.toString()}`;
  };

  const buildingHref = (record: CatalogRecord) => {
    const parameters = new URLSearchParams({ q: record.name_ko, id: record.id });
    return `${resolve('/buildings/', {})}?${parameters.toString()}`;
  };

  const itemHref = (name: string, id: string) => {
    const parameters = new URLSearchParams({ q: name, id });
    return `${resolve('/items/', {})}?${parameters.toString()}`;
  };

  const materialPlanHref = (record: CatalogRecord) => {
    const parameters = new URLSearchParams({ id: record.id });
    return `${resolve('/plan/materials/', {})}?${parameters.toString()}`;
  };

  const cardAriaLabel = (record: CatalogRecord) => {
    const unlock = buildingUnlock(catalog, record);
    return [
      record.name_ko,
      record.category,
      buildingSubcategoryLabel(record),
      unlock ? `레벨 ${unlock.level.toString()} 해금` : null,
      record.building_requires_power ? '전력 필요' : null,
    ]
      .filter(Boolean)
      .join(', ');
  };

  $effect(() => {
    const signature = `${initialQuery.trim()}\u0000${initialSelectedId.trim()}`;
    if (signature === appliedInitialState) return;
    appliedInitialState = signature;
    query = initialQuery.trim();
    const selectedById = records.find((record) => record.id === initialSelectedId.trim());
    const normalizedInitialQuery = normalizeSearchText(initialQuery);
    const exactNameMatches = normalizedInitialQuery
      ? records.filter((record) => normalizeSearchText(record.name_ko) === normalizedInitialQuery)
      : [];
    const exact = selectedById ?? (exactNameMatches.length === 1 ? exactNameMatches[0] : null);
    if (exact && buildingCategoryOrder.includes(exact.category as BuildingCategory)) {
      category = exact.category as BuildingCategory;
      subcategory = 'all';
      selectedId = exact.id;
    }
  });

  $effect(() => {
    if (selectedId && !filtered.some((record) => record.id === selectedId)) selectedId = null;
  });

  onMount(() => {
    const media = window.matchMedia('(max-width: 1023px)');
    const update = () => (compactDetail = media.matches);
    update();
    media.addEventListener('change', update);
    return () => media.removeEventListener('change', update);
  });
</script>

<section class="building-screen" class:detail-open={compactDetail && selected !== null}>
  <header class="page-head">
    <h1>{title}</h1>
  </header>

  <nav class="category-tabs" aria-label="건축물 대분류">
    {#each categoryOptions as option (option.id)}
      <button
        type="button"
        class:active={category === option.id}
        aria-pressed={category === option.id}
        onclick={() => selectCategory(option.id)}
      >
        <HugeiconsIcon icon={option.icon} size={22} strokeWidth={1.8} aria-hidden="true" />
        <span>{option.label}</span>
      </button>
    {/each}
  </nav>

  <div class="building-toolbar">
    <label class="search-field">
      <span>건축물 검색</span>
      <input
        type="search"
        bind:value={query}
        aria-label="건축물 검색"
        placeholder="건축물, 재료 또는 용도"
        autocomplete="off"
      />
      {#if query.length > 0}
        <button type="button" aria-label="검색어 지우기" onclick={() => (query = '')}>지우기</button
        >
      {/if}
    </label>
    <span class="result-summary" aria-live="polite"
      >{filtered.length.toLocaleString('ko-KR')}개</span
    >
  </div>

  {#if category !== 'all' && subcategories.length > 1}
    <nav class="subcategory-tabs" aria-label={`${category} 세부 분류`}>
      <button
        type="button"
        class:active={subcategory === 'all'}
        aria-pressed={subcategory === 'all'}
        onclick={() => (subcategory = 'all')}>전체</button
      >
      {#each subcategories as option (option.id)}
        <button
          type="button"
          class:active={subcategory === option.id}
          aria-pressed={subcategory === option.id}
          onclick={() => (subcategory = option.id)}>{option.label}</button
        >
      {/each}
    </nav>
  {/if}

  <div class="building-layout" class:with-inspector={selected !== null}>
    <div class="building-menu" aria-label="건축물 목록">
      {#if groups.length === 0}
        <div class="empty-state">
          <h2>조건에 맞는 건축물이 없습니다.</h2>
          <button type="button" onclick={reset}>검색과 분류 초기화</button>
        </div>
      {:else}
        {#each groups as group (group.id)}
          <section class="building-group" aria-labelledby={`building-group-${group.id}`}>
            <header>
              <h2 id={`building-group-${group.id}`}>{group.label}</h2>
              <span>{group.records.length.toLocaleString('ko-KR')}</span>
            </header>
            <div class="slot-grid">
              {#each group.records as record (record.id)}
                {@const unlock = buildingUnlock(catalog, record)}
                <button
                  type="button"
                  class="building-slot"
                  class:selected={selectedId === record.id}
                  class:no-media={!mediaAvailable(record.image_path)}
                  aria-pressed={selectedId === record.id}
                  aria-label={cardAriaLabel(record)}
                  onclick={(event) => selectBuilding(record, event.currentTarget)}
                >
                  {#if mediaAvailable(record.image_path)}
                    <span class="slot-media">
                      <img
                        src={record.image_path}
                        alt=""
                        loading="lazy"
                        decoding="async"
                        onerror={() => markMediaFailed(record.image_path)}
                      />
                      {#if unlock}<small>Lv. {unlock.level}</small>{/if}
                    </span>
                  {/if}
                  <strong>{record.name_ko}</strong>
                </button>
              {/each}
            </div>
          </section>
        {/each}
      {/if}
    </div>

    {#if selected}
      <aside class="inspector" aria-label="선택한 건축물 상세">
        {#if compactDetail}
          <button class="back-button" type="button" onclick={closeInspector}>
            <HugeiconsIcon icon={ArrowLeft02Icon} size={19} strokeWidth={2} aria-hidden="true" />
            건축물 목록
          </button>
        {/if}
        <header class:no-media={!mediaAvailable(selected.image_path)}>
          {#if mediaAvailable(selected.image_path)}
            <span class="inspector-media">
              <img
                src={selected.image_path}
                alt=""
                onerror={() => markMediaFailed(selected.image_path)}
              />
            </span>
          {/if}
          <div>
            <small>{selected.category} · {buildingSubcategoryLabel(selected)}</small>
            <h2>{selected.name_ko}</h2>
          </div>
          <button
            class="close-button"
            type="button"
            aria-label="건축물 상세 닫기"
            onclick={closeInspector}
          >
            <HugeiconsIcon icon={Cancel01Icon} size={19} strokeWidth={2} aria-hidden="true" />
          </button>
        </header>

        <div class="workspace-detail-action">
          {#if selected.materials?.length}
            <a class="plan-link" href={materialPlanHref(selected)}>재료 계산</a>
          {/if}
          <WorkspaceAction
            entry={{
              kind: 'building',
              id: selected.id,
              name_ko: selected.name_ko,
              href: buildingHref(selected),
              image_path: selected.image_path,
            }}
          />
        </div>

        {#if safeGameDescription(selected.description_ko)}<p class="description">
            {safeGameDescription(selected.description_ko)}
          </p>{/if}

        <dl class="decision-strip">
          {#if selectedUnlock}
            <div>
              <dt>해금</dt>
              <dd>레벨 {selectedUnlock.level}</dd>
              <small>{selectedUnlock.cost}포인트</small>
            </div>
          {/if}
          {#if buildingMetric(selected, 'HP')}
            <div>
              <dt>내구도</dt>
              <dd>{buildingMetric(selected, 'HP')}</dd>
            </div>
          {/if}
          {#if buildingMetric(selected, '방어')}
            <div>
              <dt>방어</dt>
              <dd>{buildingMetric(selected, '방어')}</dd>
            </div>
          {/if}
          {#if selected.building_requires_power}
            <div>
              <dt>가동</dt>
              <dd>전력 필요</dd>
            </div>
          {/if}
        </dl>

        {#if selected.materials?.length}
          <section class="material-section">
            <h3>필요 재료</h3>
            <ul>
              {#each selected.materials as material (material.item_id)}
                <li>
                  <a
                    href={itemHref(material.name_ko, material.item_id)}
                    aria-label={`${material.name_ko} 아이템 상세 열기`}
                  >
                    {#if mediaAvailable(material.image_path)}
                      <img
                        src={material.image_path}
                        alt=""
                        onerror={() => markMediaFailed(material.image_path)}
                      />
                    {/if}
                    <strong>{material.name_ko}</strong>
                    <b>× {material.quantity.toLocaleString('ko-KR')}</b>
                  </a>
                </li>
              {/each}
            </ul>
          </section>
        {/if}

        {#if selectedUnlock}
          <a
            class="technology-link"
            href={technologyHref(selectedUnlock.technology, selectedUnlock.technology.id)}
            >기술에서 보기</a
          >
        {/if}
      </aside>
    {/if}
  </div>
</section>

<style>
  .building-screen {
    width: min(100%, 1280px);
    margin: 0 auto;
    padding: 16px 18px 32px;
  }

  .page-head {
    display: flex;
    align-items: center;
    min-height: 42px;
  }

  .page-head h1,
  .building-group h2,
  .inspector h2,
  .material-section h3 {
    margin: 0;
  }

  .page-head h1 {
    font-size: 1.35rem;
  }

  .category-tabs {
    display: grid;
    grid-template-columns: repeat(11, minmax(78px, 1fr));
    overflow-x: auto;
    border: 1px solid var(--border);
    background: var(--sidebar);
    scrollbar-width: thin;
  }

  .category-tabs button {
    display: grid;
    min-width: 78px;
    min-height: 66px;
    place-items: center;
    gap: 4px;
    padding: 8px 6px;
    border: 0;
    border-right: 1px solid var(--border);
    border-radius: 0;
    color: var(--muted);
    background: transparent;
    font: inherit;
    font-size: 0.75rem;
    font-weight: 700;
    cursor: pointer;
    transition:
      color 120ms ease,
      background 120ms ease,
      box-shadow 120ms ease;
  }

  .category-tabs button:last-child {
    border-right: 0;
  }

  .category-tabs button:hover,
  .category-tabs button:focus-visible {
    color: var(--text);
    background: var(--surface);
  }

  .category-tabs button.active {
    color: var(--text);
    background: color-mix(in srgb, var(--accent) 13%, var(--surface));
    box-shadow: inset 0 -3px var(--accent);
  }

  .building-toolbar {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    align-items: end;
    gap: 12px;
    padding: 10px 12px;
    border: 1px solid var(--border);
    border-top: 0;
    background: var(--ink);
  }

  .search-field {
    display: grid;
    position: relative;
    gap: 5px;
    min-width: 0;
    color: var(--text-soft);
    font-size: 0.72rem;
    font-weight: 700;
  }

  .search-field input {
    min-width: 0;
    min-height: 40px;
    padding: 0 72px 0 12px;
    border: 1px solid var(--border);
    border-radius: 6px;
    color: var(--text);
    background: var(--void);
    font: inherit;
    font-size: 0.84rem;
  }

  .search-field button {
    position: absolute;
    right: 7px;
    bottom: 6px;
    min-height: 28px;
    padding: 0 9px;
    border: 1px solid var(--border);
    border-radius: 5px;
    color: var(--text-soft);
    background: var(--surface);
  }

  .result-summary {
    min-width: 54px;
    padding-bottom: 11px;
    color: var(--muted);
    text-align: right;
    font-size: 0.78rem;
  }

  .subcategory-tabs {
    display: flex;
    overflow-x: auto;
    min-height: 42px;
    border: 1px solid var(--border);
    border-top: 0;
    background: var(--surface);
    scrollbar-width: thin;
  }

  .subcategory-tabs button {
    flex: 0 0 auto;
    min-height: 41px;
    padding: 0 15px;
    border: 0;
    border-right: 1px solid var(--border);
    border-radius: 0;
    color: var(--muted);
    background: transparent;
    font: inherit;
    font-size: 0.78rem;
    font-weight: 700;
    cursor: pointer;
  }

  .subcategory-tabs button.active {
    color: var(--accent);
    background: var(--ink);
    box-shadow: inset 0 -2px var(--accent);
  }

  .building-layout {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    gap: 12px;
    margin-top: 12px;
    align-items: start;
  }

  .building-layout.with-inspector {
    grid-template-columns: minmax(0, 1fr) 340px;
  }

  .building-menu,
  .inspector {
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--ink);
  }

  .building-menu {
    min-width: 0;
    max-height: calc(100vh - 236px);
    overflow: auto;
    padding-bottom: 10px;
    scrollbar-gutter: stable;
  }

  .building-group > header {
    display: flex;
    position: sticky;
    z-index: 2;
    top: 0;
    align-items: center;
    justify-content: space-between;
    min-height: 34px;
    padding: 6px 10px;
    border-bottom: 1px solid color-mix(in srgb, var(--accent), var(--border) 58%);
    color: var(--text);
    background: color-mix(in srgb, var(--accent) 10%, var(--surface));
  }

  .building-group > header h2 {
    font-size: 0.8rem;
  }

  .building-group > header span {
    color: var(--text-soft);
    font-size: 0.7rem;
  }

  .slot-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(104px, 1fr));
    gap: 7px;
    padding: 9px;
  }

  .building-slot {
    display: grid;
    grid-template-rows: minmax(74px, 1fr) auto;
    min-width: 0;
    min-height: 112px;
    padding: 5px;
    border: 1px solid var(--border);
    border-radius: 3px;
    color: var(--text);
    background: var(--void);
    font: inherit;
    text-align: center;
    cursor: pointer;
    transition:
      border-color 120ms ease,
      background 120ms ease,
      transform 120ms ease;
  }

  .building-slot:hover,
  .building-slot:focus-visible,
  .building-slot.selected {
    border-color: var(--accent);
    background: color-mix(in srgb, var(--accent) 7%, var(--void));
  }

  .building-slot:hover {
    transform: translateY(-1px);
  }

  .building-slot.selected {
    box-shadow: inset 0 -3px var(--accent);
  }

  .building-slot.no-media {
    grid-template-rows: 1fr;
    align-items: center;
  }

  .slot-media {
    display: grid;
    position: relative;
    min-height: 72px;
    place-items: center;
  }

  .slot-media img {
    width: 68px;
    height: 68px;
    object-fit: contain;
  }

  .slot-media small {
    position: absolute;
    right: 1px;
    bottom: 1px;
    padding: 2px 4px;
    border: 1px solid color-mix(in srgb, var(--accent), var(--border) 55%);
    border-radius: 3px;
    color: var(--accent);
    background: var(--ink);
    font-size: 0.64rem;
    font-weight: 800;
  }

  .building-slot > strong {
    display: -webkit-box;
    overflow: hidden;
    min-height: 30px;
    padding-top: 4px;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    font-size: 0.74rem;
    line-height: 1.25;
  }

  .inspector {
    position: sticky;
    top: 12px;
    overflow: auto;
    max-height: calc(100vh - 196px);
    padding: 12px;
  }

  .inspector > header {
    display: grid;
    grid-template-columns: 78px minmax(0, 1fr) 34px;
    align-items: center;
    gap: 10px;
    padding-bottom: 12px;
    border-bottom: 1px solid var(--border);
  }

  .inspector > header.no-media {
    grid-template-columns: minmax(0, 1fr) 34px;
  }

  .workspace-detail-action {
    display: flex;
    gap: 8px;
    justify-content: flex-end;
    padding-block: 8px;
    border-bottom: 1px solid var(--border);
  }

  .inspector-media {
    display: grid;
    width: 78px;
    height: 78px;
    place-items: center;
    border: 1px solid var(--border-strong);
    background: var(--void);
  }

  .inspector-media img {
    width: 70px;
    height: 70px;
    object-fit: contain;
  }

  .inspector header small {
    display: block;
    margin-bottom: 4px;
    color: var(--accent);
    font-size: 0.7rem;
    font-weight: 800;
  }

  .inspector h2 {
    font-size: 1.04rem;
    line-height: 1.25;
  }

  .close-button,
  .back-button {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 34px;
    min-height: 34px;
    border: 1px solid var(--border);
    border-radius: 5px;
    color: var(--text-soft);
    background: var(--surface);
    cursor: pointer;
  }

  .back-button {
    display: none;
    gap: 6px;
    margin-bottom: 10px;
    padding: 0 11px;
    font: inherit;
    font-size: 0.78rem;
    font-weight: 700;
  }

  .description {
    margin: 12px 0;
    color: var(--text-soft);
    font-size: 0.82rem;
    line-height: 1.55;
  }

  .decision-strip {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 1px;
    margin: 0 0 14px;
    border: 1px solid var(--border);
    background: var(--border);
  }

  .decision-strip div {
    min-width: 0;
    padding: 8px;
    background: var(--surface);
  }

  .decision-strip dt,
  .decision-strip small {
    color: var(--muted);
    font-size: 0.68rem;
  }

  .decision-strip dd {
    margin: 3px 0 0;
    color: var(--text);
    font-size: 0.82rem;
    font-weight: 800;
  }

  .material-section {
    margin-top: 14px;
  }

  .material-section h3 {
    margin-bottom: 8px;
    font-size: 0.84rem;
  }

  .material-section ul {
    display: grid;
    gap: 5px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .material-section li {
    border: 1px solid var(--border);
    background: var(--surface);
  }

  .material-section li > a {
    display: grid;
    min-height: 44px;
    grid-template-columns: 34px minmax(0, 1fr) auto 12px;
    align-items: center;
    gap: 8px;
    padding: 4px 7px;
    color: inherit;
    text-decoration: none;
  }

  .material-section li > a::after {
    color: var(--muted-strong);
    content: '›';
    font-size: 1.1rem;
  }

  .material-section img {
    width: 32px;
    height: 32px;
    object-fit: contain;
  }

  .material-section strong,
  .material-section b {
    font-size: 0.78rem;
  }

  .material-section li > a:hover,
  .material-section li > a:focus-visible {
    color: var(--accent);
    background: color-mix(in srgb, var(--accent) 7%, var(--surface));
  }

  .material-section b {
    color: var(--accent);
  }

  .technology-link,
  .plan-link,
  .empty-state button {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-height: 40px;
    padding: 0 13px;
    border: 1px solid var(--accent);
    border-radius: 6px;
    color: var(--void);
    background: var(--accent);
    font-size: 0.8rem;
    font-weight: 800;
    text-decoration: none;
  }

  .technology-link {
    width: 100%;
    margin-top: 14px;
  }

  .plan-link {
    min-height: 36px;
    padding-inline: 11px;
  }

  .empty-state {
    display: grid;
    min-height: 260px;
    place-items: center;
    align-content: center;
    gap: 12px;
    padding: 24px;
    text-align: center;
  }

  .empty-state h2 {
    margin: 0;
    font-size: 1rem;
  }

  @media (max-width: 1023px) {
    .building-screen {
      padding-inline: 14px;
    }

    .building-layout.with-inspector {
      grid-template-columns: 1fr;
    }

    .building-menu {
      max-height: none;
      overflow: visible;
    }

    .building-group > header {
      position: static;
    }

    .inspector {
      position: static;
      max-height: none;
    }

    .back-button {
      display: inline-flex;
    }

    .detail-open .page-head,
    .detail-open .category-tabs,
    .detail-open .building-toolbar,
    .detail-open .subcategory-tabs,
    .detail-open .building-menu {
      display: none;
    }

    .detail-open .building-layout {
      margin-top: 0;
    }
  }

  @media (max-width: 719px) {
    .building-screen {
      padding: 12px 10px 82px;
    }

    .category-tabs {
      display: flex;
    }

    .category-tabs button {
      flex: 0 0 82px;
      min-height: 60px;
    }

    .building-toolbar {
      grid-template-columns: minmax(0, 1fr) auto;
      padding-inline: 8px;
    }

    .slot-grid {
      grid-template-columns: repeat(auto-fill, minmax(94px, 1fr));
      gap: 6px;
      padding: 7px;
    }

    .building-slot {
      min-height: 106px;
    }

    .inspector {
      padding: 10px;
    }

    .inspector > header {
      grid-template-columns: 70px minmax(0, 1fr) 34px;
    }

    .inspector-media {
      width: 70px;
      height: 70px;
    }

    .inspector-media img {
      width: 62px;
      height: 62px;
    }
  }

  @media (max-width: 380px) {
    .slot-grid {
      grid-template-columns: repeat(2, minmax(0, 1fr));
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .category-tabs button,
    .building-slot {
      transition: none;
    }
  }
</style>
