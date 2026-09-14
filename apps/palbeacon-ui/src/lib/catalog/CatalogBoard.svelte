<script lang="ts">
  import { onMount, tick, untrack } from 'svelte';
  import { get } from 'svelte/store';
  import { resolve } from '$app/paths';
  import { createVirtualizer } from '@tanstack/svelte-virtual';
  import {
    catalogKindLabels,
    categoriesForRecords,
    itemDisplayDescription,
    itemRarityLabel,
    type ItemAcquisitionSummary,
    rarityAccent,
    recordsForScope,
    searchCatalog,
    summarizeItemAcquisition,
  } from './catalog';
  import type { CatalogItemRelations, CatalogKind, CatalogRecord, UnifiedCatalog } from './types';
  import { safeGameDescription } from '$lib/shared/data/display-text';
  import { normalizeSearchText } from '$lib/shared/search/search-core';

  interface Props {
    catalog: UnifiedCatalog;
    scope: CatalogKind | 'all';
    title: string;
    initialQuery?: string;
    initialSelectedId?: string;
  }

  let { catalog, scope, title, initialQuery = '', initialSelectedId = '' }: Props = $props();
  let query = $state(untrack(() => initialQuery));
  let category = $state('all');
  let pageSize = $state(24);
  let displayLimit = $state(24);
  let compact = $state(true);
  let selectedKey = $state<string | null>(null);
  let selectionTrigger = $state<HTMLElement | null>(null);
  let selectedInspector = $state<HTMLElement | null>(null);
  let skillsExpanded = $state(false);
  let itemDropLimit = $state(4);
  let itemShopLimit = $state(4);
  let resultScroller = $state<HTMLDivElement | null>(null);
  let appliedInitialState = $state<string | null>(null);
  let failedMedia = $state(new Set<string>());

  const emptyItemRelations: CatalogItemRelations = {
    recipes: [],
    drops: [],
    shops: [],
    technologies: [],
  };
  const emptyItemAcquisition: ItemAcquisitionSummary = {
    routeCount: 0,
    compactLabel: null,
    routeSummary: null,
    unlockSummary: null,
    relationLabels: [],
  };

  const baseRecords = $derived(
    recordsForScope(catalog, scope).filter(
      (record) => !record.localization_fallback && record.name_ko.trim().length > 0,
    ),
  );
  const shopInventorySignature = (record: CatalogRecord): string =>
    [
      record.name_ko,
      record.category,
      ...(record.products ?? [])
        .map((product) =>
          [product.item_id, product.quantity, product.price, product.stock ?? ''].join(':'),
        )
        .toSorted(),
    ].join('|');
  const recordComposition = $derived.by(() => {
    const groups = new Map<string, CatalogRecord[]>();
    const passthrough: CatalogRecord[] = [];
    for (const record of baseRecords) {
      if (record.kind !== 'shop') {
        passthrough.push(record);
        continue;
      }
      const signature = shopInventorySignature(record);
      groups.set(signature, [...(groups.get(signature) ?? []), record]);
    }
    const shopLocationCounts = new Map<string, number>();
    const shops = [...groups.values()].flatMap((group) => {
      const representative = group[0];
      if (!representative) return [];
      shopLocationCounts.set(representative.id, group.length);
      return [representative];
    });
    return { records: [...passthrough, ...shops], shopLocationCounts };
  });
  const records = $derived(recordComposition.records);
  const itemAcquisitionById = $derived(
    new Map(
      catalog.records.items.map((record) => [
        record.id,
        summarizeItemAcquisition(catalog.item_relations?.[record.id] ?? emptyItemRelations),
      ]),
    ),
  );
  const categories = $derived(categoriesForRecords(records, scope));
  const searchResult = $derived(
    searchCatalog(records, query, category, scope, compact ? displayLimit : records.length),
  );
  const selected = $derived(
    selectedKey === null
      ? null
      : (records.find((record) => `${record.kind}:${record.id}` === selectedKey) ?? null),
  );
  const visiblePalSkills = $derived(
    (selected?.pal_skills ?? []).slice(
      0,
      compact && !skillsExpanded ? 4 : selected?.pal_skills?.length,
    ),
  );
  const selectedItemRelations = $derived(
    selected?.kind === 'item'
      ? (catalog.item_relations?.[selected.id] ?? emptyItemRelations)
      : emptyItemRelations,
  );
  const selectedItemAcquisition = $derived(
    selected?.kind === 'item'
      ? (itemAcquisitionById.get(selected.id) ?? emptyItemAcquisition)
      : emptyItemAcquisition,
  );
  const verifiedItemDrops = $derived(
    selectedItemRelations.drops.flatMap((drop) => {
      const pal = catalog.records.pals.find((record) => record.id === drop.pal_id);
      return pal ? [{ drop, pal }] : [];
    }),
  );
  const verifiedItemTechnologies = $derived(
    selectedItemRelations.technologies.flatMap((unlock) => {
      const technology = catalog.records.technologies.find(
        (record) => record.id === unlock.technology_id,
      );
      return technology ? [{ unlock, technology }] : [];
    }),
  );
  const visibleItemDrops = $derived(verifiedItemDrops.slice(0, itemDropLimit));
  const visibleItemShops = $derived(selectedItemRelations.shops.slice(0, itemShopLimit));
  const categoryLabel = $derived(scope === 'all' ? '종류' : '분류');
  const searchLabel = $derived(title.endsWith('검색') ? title : `${title} 검색`);
  const virtualizer = createVirtualizer<HTMLDivElement, HTMLDivElement>({
    count: 0,
    getScrollElement: () => resultScroller,
    estimateSize: () => 126,
    overscan: 6,
  });

  const exactRecordForEntry = (displayQuery: string, entityId: string) => {
    const normalizedId = normalizeSearchText(entityId);
    if (normalizedId.length > 0) {
      const idMatch = records.find((record) => normalizeSearchText(record.id) === normalizedId);
      if (idMatch) return idMatch;
    }

    const normalizedQuery = normalizeSearchText(displayQuery);
    if (normalizedQuery.length === 0) return null;
    const idMatch = records.find((record) => normalizeSearchText(record.id) === normalizedQuery);
    if (idMatch) return idMatch;
    const nameMatches = records.filter(
      (record) => normalizeSearchText(record.name_ko) === normalizedQuery,
    );
    return nameMatches.length === 1 ? nameMatches[0] : null;
  };

  $effect(() => {
    const displayQuery = initialQuery.trim();
    const entityId = initialSelectedId.trim();
    const signature = `${displayQuery}\u0000${entityId}`;
    if (signature === appliedInitialState) return;
    appliedInitialState = signature;
    query = displayQuery;
    category = 'all';
    displayLimit = pageSize;
    skillsExpanded = false;
    itemDropLimit = 4;
    itemShopLimit = 4;
    const exact = exactRecordForEntry(displayQuery, entityId);
    selectedKey = scope !== 'all' && exact ? `${exact.kind}:${exact.id}` : null;
    selectionTrigger = null;
  });

  $effect(() => {
    const count = compact ? 0 : searchResult.visible.length;
    untrack(() => {
      get(virtualizer).setOptions({
        count,
        getScrollElement: () => resultScroller,
        estimateSize: () => 126,
        overscan: 6,
      });
    });
  });

  $effect(() => {
    const current = selectedKey;
    if (current === null) return;
    const currentRecord = records.find((record) => `${record.kind}:${record.id}` === current);
    if (
      currentRecord === undefined ||
      searchCatalog([currentRecord], query, category, scope, 1).total === 0
    )
      selectedKey = null;
  });

  const selectRecord = async (record: CatalogRecord, trigger: HTMLElement) => {
    selectionTrigger = trigger;
    selectedKey = `${record.kind}:${record.id}`;
    skillsExpanded = false;
    itemDropLimit = 4;
    itemShopLimit = 4;
    await tick();
    selectedInspector?.focus({ preventScroll: true });
    selectedInspector?.scrollIntoView({ block: 'nearest' });
  };

  const closeDetail = async () => {
    const trigger = selectionTrigger;
    selectedKey = null;
    selectionTrigger = null;
    await tick();
    trigger?.focus({ preventScroll: true });
  };

  const reset = () => {
    query = '';
    category = 'all';
    displayLimit = pageSize;
    selectedKey = null;
    selectionTrigger = null;
    skillsExpanded = false;
    itemDropLimit = 4;
    itemShopLimit = 4;
  };

  const visibleTags = (record: CatalogRecord) => {
    const duplicated = new Set([record.category, ...itemRelationLabels(record)]);
    if (record.kind === 'item') {
      const rarityName = itemRarityLabel(record);
      if (rarityName) duplicated.add(rarityName);
    }
    return record.tags.filter((tag) => !duplicated.has(tag)).slice(0, 2);
  };

  const hiddenMetricLabels = new Set(['등급', '티어', '랭크', '효과 등급']);
  const visibleMetrics = (record: CatalogRecord) =>
    record.metrics.filter(
      (metric) =>
        !hiddenMetricLabels.has(metric.label) &&
        !(record.kind === 'shop' && ['판매품', '화폐'].includes(metric.label)),
    );
  const visibleDescription = (record: CatalogRecord) =>
    record.kind === 'shop'
      ? null
      : record.kind === 'item'
        ? itemDisplayDescription(record)
        : safeGameDescription(record.description_ko);

  const shopProductsForCard = (record: CatalogRecord) => {
    if (record.kind !== 'shop') return [];
    const normalizedQuery = normalizeSearchText(query);
    const products = record.products ?? [];
    const matching =
      normalizedQuery.length === 0
        ? []
        : products.filter((product) =>
            normalizeSearchText(product.item_name_ko).includes(normalizedQuery),
          );
    return (
      matching.length > 0
        ? [...matching, ...products.filter((product) => !matching.includes(product))]
        : products
    ).slice(0, 3);
  };

  const shopLocationCount = (record: CatalogRecord) =>
    record.kind === 'shop' ? (recordComposition.shopLocationCounts.get(record.id) ?? 1) : 0;

  const itemAcquisitionFor = (record: CatalogRecord): ItemAcquisitionSummary =>
    record.kind === 'item'
      ? (itemAcquisitionById.get(record.id) ?? emptyItemAcquisition)
      : emptyItemAcquisition;

  const itemRelationLabels = (record: CatalogRecord): string[] => {
    return itemAcquisitionFor(record).relationLabels;
  };

  const compactItemAcquisitionLabel = (record: CatalogRecord) =>
    itemAcquisitionFor(record).compactLabel;

  const itemHasTechnologyGate = (record: CatalogRecord) =>
    itemAcquisitionFor(record).unlockSummary !== null;

  const itemRecord = (id: string) => catalog.records.items.find((record) => record.id === id);
  type CatalogDetailPath =
    | '/items/'
    | '/pals/'
    | '/technology/'
    | '/buildings/'
    | '/skills/active/'
    | '/skills/passive/';

  const queryHref = (path: CatalogDetailPath, displayQuery: string, entityId = '') => {
    const parameters = new URLSearchParams({ q: displayQuery });
    if (entityId.length > 0) parameters.set('id', entityId);
    const resolvedPath =
      path === '/technology/'
        ? resolve('/technology/', {})
        : resolve('/[...path]', { path: path.replaceAll(/^\/+|\/+$/g, '') });
    return `${resolvedPath}?${parameters.toString()}`;
  };

  const detailPathForKind = (kind: CatalogKind): CatalogDetailPath | null => {
    switch (kind) {
      case 'pal':
        return '/pals/';
      case 'item':
        return '/items/';
      case 'technology':
        return '/technology/';
      case 'building':
        return '/buildings/';
      case 'active_skill':
        return '/skills/active/';
      case 'passive_skill':
        return '/skills/passive/';
      case 'shop':
        return null;
    }
  };

  const recordDetailHref = (record: CatalogRecord): string | null => {
    if (scope !== 'all') return null;
    const path = detailPathForKind(record.kind);
    return path === null ? null : queryHref(path, record.name_ko, record.id);
  };

  const quantityLabel = (minimum: number, maximum: number) =>
    minimum === maximum
      ? `${minimum.toLocaleString('ko-KR')}개`
      : `${minimum.toLocaleString('ko-KR')}–${maximum.toLocaleString('ko-KR')}개`;

  const probabilityLabel = (probabilityPpm: number) =>
    `${new Intl.NumberFormat('ko-KR', { maximumFractionDigits: 2 }).format(probabilityPpm / 10_000)}%`;

  const resultSummary = (total: number, visible: number) =>
    total === visible
      ? `${total.toLocaleString('ko-KR')}개 결과`
      : `${total.toLocaleString('ko-KR')}개 중 ${visible.toLocaleString('ko-KR')}개 표시`;

  const movementStanding = (
    value: number,
    field: 'run_speed' | 'ride_sprint_speed' | 'transport_speed' | 'stamina',
  ) => {
    if (value <= 0) return null;
    const values = catalog.records.pals
      .map((record) => record.pal_profile?.[field] ?? 0)
      .filter((candidate) => candidate > 0);
    const faster = values.filter((candidate) => candidate > value).length;
    const percentile = Math.max(1, Math.ceil(((faster + 1) / values.length) * 100));
    return `상위 ${percentile.toString()}%`;
  };

  const mediaAvailable = (path: string | null | undefined): path is string =>
    Boolean(path) && !failedMedia.has(path as string);

  const availablePreviewImages = (record: CatalogRecord) =>
    (record.preview_images ?? []).filter(mediaAvailable);

  const primaryImage = (record: CatalogRecord) =>
    mediaAvailable(record.image_path)
      ? record.image_path
      : (availablePreviewImages(record)[0] ?? null);

  const hasMedia = (record: CatalogRecord) => Boolean(primaryImage(record));
  const keepsMediaSlot = (record: CatalogRecord) => hasMedia(record);

  const markMediaFailed = (path: string | null | undefined) => {
    if (!path || failedMedia.has(path)) return;
    failedMedia = new Set([...failedMedia, path]);
  };

  const paldexLabel = (record: CatalogRecord) => {
    const number = record.pal_profile?.paldex_number;
    if (number === null || number === undefined) return null;
    const suffix = record.pal_profile?.paldex_suffix ?? '';
    return `No. ${number.toString().padStart(3, '0')}${suffix}`;
  };

  const recordContextLabel = (record: CatalogRecord) =>
    record.kind === 'pal'
      ? catalogKindLabels[record.kind]
      : record.kind === 'shop'
        ? catalogKindLabels[record.kind]
        : `${catalogKindLabels[record.kind]} · ${record.category}`;

  const recordAriaLabel = (record: CatalogRecord) => {
    const palDetails =
      record.kind === 'pal'
        ? [
            ...(paldexLabel(record) ? [paldexLabel(record)] : []),
            ...(record.elements ?? []).map((element) => element.name_ko),
            ...(record.work_suitability ?? []).map(
              (work) => `${work.name_ko} 레벨 ${work.level.toString()}`,
            ),
          ]
        : [];
    const itemDetails = record.kind === 'item' ? itemRelationLabels(record) : [];
    const shopDetails =
      record.kind === 'shop'
        ? [
            ...shopProductsForCard(record).map(
              (product) =>
                `${product.item_name_ko} ${product.price.toLocaleString('ko-KR')} ${record.category}`,
            ),
            `판매품 ${(record.products?.length ?? 0).toLocaleString('ko-KR')}종`,
            ...(shopLocationCount(record) > 1
              ? [`판매 위치 ${shopLocationCount(record).toLocaleString('ko-KR')}곳`]
              : []),
          ]
        : [];
    return [
      record.name_ko,
      ...palDetails,
      ...itemDetails,
      ...shopDetails,
      catalogKindLabels[record.kind],
    ].join(', ');
  };

  onMount(() => {
    const desktopMedia = window.matchMedia('(min-width: 720px)');
    const updatePageSize = () => {
      compact = !desktopMedia.matches;
      const previous = pageSize;
      pageSize = compact ? 12 : 24;
      if (displayLimit === previous || displayLimit < pageSize) displayLimit = pageSize;
    };
    updatePageSize();
    desktopMedia.addEventListener('change', updatePageSize);
    return () => desktopMedia.removeEventListener('change', updatePageSize);
  });
</script>

{#snippet recordCard(record: CatalogRecord)}
  {@const previewImages = availablePreviewImages(record)}
  {@const mainImage = primaryImage(record)}
  {@const detailHref = recordDetailHref(record)}
  <svelte:element
    this={detailHref === null ? 'button' : 'a'}
    role={detailHref === null ? 'button' : 'link'}
    type={detailHref === null ? 'button' : undefined}
    href={detailHref ?? undefined}
    class="record-card"
    class:detail-link={detailHref !== null}
    class:selected={selectedKey === `${record.kind}:${record.id}`}
    class:no-media={!keepsMediaSlot(record)}
    class:no-metrics={visibleMetrics(record).length === 0}
    class:description-priority={record.kind === 'passive_skill'}
    class:pal-record={record.kind === 'pal'}
    style={`--record-accent:${rarityAccent(record.rarity)}`}
    aria-pressed={detailHref === null ? selectedKey === `${record.kind}:${record.id}` : undefined}
    aria-label={`${recordAriaLabel(record)}${detailHref === null ? '' : ', 상세 보기'}`}
    onclick={detailHref === null
      ? (event: MouseEvent) => selectRecord(record, event.currentTarget as HTMLElement)
      : undefined}
  >
    {#if keepsMediaSlot(record)}
      <span class="card-media">
        {#if previewImages.length > 1}
          <span class="image-stack" aria-hidden="true">
            {#each previewImages as image, imageIndex (`${image}:${imageIndex.toString()}`)}
              <img
                src={image}
                alt=""
                loading="lazy"
                decoding="async"
                onerror={() => markMediaFailed(image)}
              />
            {/each}
          </span>
        {:else if mainImage}
          <img
            src={mainImage}
            alt=""
            loading="lazy"
            decoding="async"
            width="66"
            height="66"
            onerror={() => markMediaFailed(mainImage)}
          />
        {/if}
        {#if record.kind === 'pal' && record.elements?.length}
          <span class="media-elements" aria-hidden="true">
            {#each record.elements as element (element.id)}
              {#if mediaAvailable(element.icon_path)}
                <img
                  src={element.icon_path}
                  alt=""
                  onerror={() => markMediaFailed(element.icon_path)}
                />
              {/if}
            {/each}
          </span>
        {:else if mediaAvailable(record.element_icon_path)}
          <img
            class="element-icon"
            src={record.element_icon_path}
            alt=""
            aria-hidden="true"
            onerror={() => markMediaFailed(record.element_icon_path)}
          />
        {/if}
        {#if record.kind === 'pal' && paldexLabel(record)}
          <span class="paldex-number">{paldexLabel(record)}</span>
        {/if}
      </span>
    {/if}
    <span class="card-copy">
      <span class="kind-label">
        {recordContextLabel(record)}
      </span>
      <strong>{record.name_ko}</strong>
      {#if record.kind === 'pal'}
        <span class="pal-element-row">
          {#each record.elements ?? [] as element (element.id)}
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
          {#if record.pal_profile?.nocturnal}<span class="nocturnal">야행성</span>{/if}
        </span>
        <span class="work-row" aria-label="작업 적성">
          {#each (record.work_suitability ?? []).slice(0, compact ? 3 : 4) as work (work.id)}
            <span title={`${work.name_ko} Lv. ${work.level.toString()}`}>
              {#if mediaAvailable(work.icon_path)}
                <img src={work.icon_path} alt="" onerror={() => markMediaFailed(work.icon_path)} />
              {/if}
              <small>{work.name_ko}</small>
              <b>Lv.{work.level}</b>
            </span>
          {/each}
          {#if (record.work_suitability?.length ?? 0) > (compact ? 3 : 4)}
            <span class="work-more"
              >+{(record.work_suitability?.length ?? 0) - (compact ? 3 : 4)}</span
            >
          {/if}
        </span>
      {:else}
        {#if visibleDescription(record)}<span class="card-description"
            >{visibleDescription(record)}</span
          >{/if}
        {#if record.kind === 'shop'}
          <span class="shop-product-preview">
            {#each shopProductsForCard(record) as product (product.product_id)}
              <span>
                <strong>{product.item_name_ko}</strong>
                <small>{product.price.toLocaleString('ko-KR')} {record.category}</small>
              </span>
            {/each}
          </span>
        {/if}
        <span class="tag-row">
          {#if record.kind === 'item' && record.rarity !== undefined && itemRarityLabel(record)}
            <span class="rarity-tag" style={`--rarity-color:${rarityAccent(record.rarity)}`}
              >{itemRarityLabel(record)}</span
            >
            {#if compactItemAcquisitionLabel(record)}
              <span class="relation-tag">{compactItemAcquisitionLabel(record)}</span>
            {/if}
            {#if itemHasTechnologyGate(record)}
              <span class="relation-tag">기술 해금</span>
            {/if}
          {/if}
          {#each visibleTags(record) as tag, tagIndex (`${tag}:${tagIndex.toString()}`)}
            <span>{tag}</span>
          {/each}
          {#if record.kind === 'shop'}
            <span>판매품 {(record.products?.length ?? 0).toLocaleString('ko-KR')}종</span>
            {#if shopLocationCount(record) > 1}<span
                >판매 위치 {shopLocationCount(record).toLocaleString('ko-KR')}곳</span
              >{/if}
          {/if}
        </span>
      {/if}
    </span>
    {#if visibleMetrics(record).length > 0}
      <span class="card-metrics">
        {#each visibleMetrics(record).slice(0, 3) as metric (metric.label)}
          <span aria-label={`${metric.label} ${metric.value}`}
            ><small>{metric.label}</small><strong>{metric.value}</strong></span
          >
        {/each}
      </span>
    {/if}
  </svelte:element>
{/snippet}

<section class="catalog-screen">
  <header class="page-head">
    <h1>{title}</h1>
  </header>

  <div class="toolbar">
    <label class="search-field">
      <span>{searchLabel}</span>
      <input
        type="search"
        bind:value={query}
        aria-label={searchLabel}
        placeholder="한글 표시명, 분류 또는 특징"
        autocomplete="off"
      />
      {#if query.length > 0}
        <button type="button" aria-label="검색어 지우기" onclick={() => (query = '')}>지우기</button
        >
      {/if}
    </label>
    <label class="category-field">
      <span>{categoryLabel}</span>
      <select bind:value={category}>
        <option value="all">전체</option>
        {#each categories as option (option)}
          <option value={option}>{option}</option>
        {/each}
      </select>
    </label>
    <span class="result-summary" aria-live="polite">
      {resultSummary(searchResult.total, searchResult.visible.length)}
    </span>
  </div>

  <div class="catalog-layout" class:with-inspector={selected !== null}>
    <div class="result-region" aria-label={`${title} 결과`}>
      {#if searchResult.visible.length === 0}
        <div class="empty-state">
          <h2>조건에 맞는 항목이 없습니다.</h2>
          <p>검색어를 줄이거나 분류를 전체로 바꿔 보세요.</p>
          <button type="button" onclick={reset}>검색과 필터 초기화</button>
        </div>
      {:else}
        {#if compact}
          <div class="record-grid">
            {#each searchResult.visible as record (record.kind + record.id)}
              {@render recordCard(record)}
            {/each}
          </div>
        {:else}
          <div class="virtual-scroller" bind:this={resultScroller}>
            <div
              class="virtual-canvas"
              style:height={`${$virtualizer.getTotalSize().toString()}px`}
            >
              {#each $virtualizer.getVirtualItems() as row (row.key)}
                {@const record = searchResult.visible[row.index]}
                {#if record}
                  <div
                    class="virtual-row"
                    data-index={row.index}
                    use:$virtualizer.measureElement
                    style:transform={`translateY(${row.start.toString()}px)`}
                  >
                    {@render recordCard(record)}
                  </div>
                {/if}
              {/each}
            </div>
          </div>
        {/if}

        {#if compact && searchResult.visible.length < searchResult.total}
          <button class="load-more" type="button" onclick={() => (displayLimit += pageSize)}>
            다음 {pageSize}개 보기 · 남은 {(
              searchResult.total - searchResult.visible.length
            ).toLocaleString('ko-KR')}개
          </button>
        {/if}
      {/if}
    </div>

    {#if selected}
      {@const selectedPreviewImages = availablePreviewImages(selected)}
      {@const selectedMainImage = primaryImage(selected)}
      <aside
        class="inspector"
        aria-label="선택한 데이터 상세"
        tabindex="-1"
        bind:this={selectedInspector}
      >
        <header class:no-media={!keepsMediaSlot(selected)}>
          {#if keepsMediaSlot(selected)}
            <span
              class="inspector-media"
              style={`--record-accent:${rarityAccent(selected.rarity)}`}
            >
              {#if selectedPreviewImages.length > 1}
                <span class="image-stack" aria-hidden="true">
                  {#each selectedPreviewImages as image, imageIndex (`${image}:${imageIndex.toString()}`)}
                    <img src={image} alt="" onerror={() => markMediaFailed(image)} />
                  {/each}
                </span>
              {:else if selectedMainImage}
                <img
                  src={selectedMainImage}
                  alt=""
                  width="72"
                  height="72"
                  onerror={() => markMediaFailed(selectedMainImage)}
                />
              {/if}
              {#if selected.kind === 'pal' && selected.elements?.length}
                <span class="media-elements" aria-hidden="true">
                  {#each selected.elements as element (element.id)}
                    {#if mediaAvailable(element.icon_path)}
                      <img
                        src={element.icon_path}
                        alt=""
                        onerror={() => markMediaFailed(element.icon_path)}
                      />
                    {/if}
                  {/each}
                </span>
              {/if}
            </span>
          {/if}
          <div>
            <small>
              {recordContextLabel(selected)}
            </small>
            <h2>{selected.name_ko}</h2>
          </div>
          <button type="button" aria-label="상세 닫기" onclick={closeDetail}>닫기</button>
        </header>

        {#if visibleDescription(selected)}
          <p class="description">{visibleDescription(selected)}</p>
        {/if}

        {#if selected.kind === 'pal' && (selected.elements?.length || selected.pal_profile)}
          <div class="pal-status-strip">
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
            {#if selected.pal_profile}
              <strong>{selected.pal_profile.nocturnal ? '야행성' : '주행성'}</strong>
            {/if}
          </div>
        {:else if selected.kind === 'item' && selected.rarity !== undefined && itemRarityLabel(selected)}
          <dl class="item-decision-strip" style={`--rarity-color:${rarityAccent(selected.rarity)}`}>
            <div class="rarity-decision">
              <dt>희귀도</dt>
              <dd>{itemRarityLabel(selected)}</dd>
            </div>
            {#if selectedItemAcquisition.routeSummary}
              <div>
                <dt>획득 경로</dt>
                <dd>{selectedItemAcquisition.routeSummary}</dd>
              </div>
            {/if}
            {#if selectedItemAcquisition.unlockSummary}
              <div>
                <dt>해금 조건</dt>
                <dd>{selectedItemAcquisition.unlockSummary}</dd>
              </div>
            {/if}
          </dl>
        {/if}

        {#if visibleMetrics(selected).length > 0}
          <dl class="metric-grid">
            {#each visibleMetrics(selected) as metric (metric.label)}
              <div>
                <dt>{metric.label}</dt>
                <dd>{metric.value}</dd>
              </div>
            {/each}
          </dl>
        {/if}

        {#if selected.kind === 'item' && selectedItemRelations.recipes.length > 0}
          <section class="detail-section item-relation-section recipe-section">
            <h3>만드는 법</h3>
            <div class="recipe-list">
              {#each selectedItemRelations.recipes as recipe, recipeIndex (recipe.recipe_id)}
                <article class="recipe-card">
                  <header>
                    <span>제작식 {(recipeIndex + 1).toLocaleString('ko-KR')}</span>
                    <strong>완성 × {recipe.output_quantity.toLocaleString('ko-KR')}</strong>
                  </header>
                  <ul class="ingredient-list">
                    {#each recipe.ingredients as ingredient (`${recipe.recipe_id}:${ingredient.item_id}`)}
                      {@const ingredientRecord = itemRecord(ingredient.item_id)}
                      <li style={`--record-accent:${rarityAccent(ingredientRecord?.rarity)}`}>
                        {#if ingredientRecord}
                          <a
                            href={queryHref(
                              '/items/',
                              ingredientRecord.name_ko,
                              ingredientRecord.id,
                            )}
                          >
                            {#if mediaAvailable(ingredientRecord.image_path)}
                              <img
                                src={ingredientRecord.image_path}
                                alt=""
                                onerror={() => markMediaFailed(ingredientRecord.image_path)}
                              />
                            {/if}
                            <span>{ingredientRecord.name_ko}</span>
                          </a>
                        {:else}
                          <span class="relation-copy">{ingredient.name_ko}</span>
                        {/if}
                        <b>× {ingredient.quantity.toLocaleString('ko-KR')}</b>
                      </li>
                    {/each}
                  </ul>
                </article>
              {/each}
            </div>
          </section>
        {/if}

        {#if selected.kind === 'item' && verifiedItemDrops.length > 0}
          <section class="detail-section item-relation-section drop-section">
            <h3>얻는 법</h3>
            <ol class="drop-list">
              {#each visibleItemDrops as entry (entry.drop.method_id)}
                {@const drop = entry.drop}
                {@const pal = entry.pal}
                <li>
                  <a class="relation-identity" href={queryHref('/pals/', pal.name_ko, pal.id)}>
                    {#if mediaAvailable(pal.image_path)}
                      <img
                        src={pal.image_path}
                        alt=""
                        onerror={() => markMediaFailed(pal.image_path)}
                      />
                    {/if}
                    <span>
                      <strong>{pal.name_ko}</strong>
                      <small>
                        {drop.variant === 'boss' ? '보스' : '일반'}{drop.level > 0
                          ? ` · Lv. ${drop.level.toString()}`
                          : ''}
                      </small>
                    </span>
                  </a>
                  <span class="drop-values">
                    <strong>{probabilityLabel(drop.probability_ppm)}</strong>
                    <small>{quantityLabel(drop.minimum_quantity, drop.maximum_quantity)}</small>
                  </span>
                </li>
              {/each}
            </ol>
            {#if verifiedItemDrops.length > 4}
              <button
                class="detail-more"
                type="button"
                onclick={() =>
                  (itemDropLimit =
                    itemDropLimit >= verifiedItemDrops.length
                      ? 4
                      : Math.min(itemDropLimit + 8, verifiedItemDrops.length))}
              >
                {itemDropLimit >= verifiedItemDrops.length
                  ? '드롭 접기'
                  : `다음 ${Math.min(8, verifiedItemDrops.length - itemDropLimit).toString()}개 보기`}
              </button>
            {/if}
          </section>
        {/if}

        {#if selected.kind === 'item' && selectedItemRelations.shops.length > 0}
          <section class="detail-section item-relation-section shop-relation-section">
            <h3>구매</h3>
            <ul class="shop-offer-list">
              {#each visibleItemShops as offer (offer.product_id)}
                <li>
                  <div class="shop-offer">
                    <span>
                      <strong>{offer.currency_name_ko} 상점</strong>
                      <small>
                        상품 × {offer.quantity.toLocaleString('ko-KR')}{offer.stock === null
                          ? ''
                          : ` · 재고 ${offer.stock.toLocaleString('ko-KR')}`}
                      </small>
                    </span>
                    <b>{offer.price.toLocaleString('ko-KR')} {offer.currency_name_ko}</b>
                  </div>
                </li>
              {/each}
            </ul>
            {#if selectedItemRelations.shops.length > 4}
              <button
                class="detail-more"
                type="button"
                onclick={() =>
                  (itemShopLimit =
                    itemShopLimit >= selectedItemRelations.shops.length
                      ? 4
                      : Math.min(itemShopLimit + 8, selectedItemRelations.shops.length))}
              >
                {itemShopLimit >= selectedItemRelations.shops.length
                  ? '상점 접기'
                  : `다음 ${Math.min(8, selectedItemRelations.shops.length - itemShopLimit).toString()}곳 보기`}
              </button>
            {/if}
          </section>
        {/if}

        {#if selected.kind === 'item' && verifiedItemTechnologies.length > 0}
          <section class="detail-section item-relation-section technology-relation-section">
            <h3>해금 기술</h3>
            <ul class="technology-relation-list">
              {#each verifiedItemTechnologies as entry (entry.unlock.technology_id)}
                {@const unlock = entry.unlock}
                {@const technology = entry.technology}
                <li>
                  <a href={queryHref('/technology/', technology.name_ko, unlock.technology_id)}>
                    {#if mediaAvailable(technology.image_path)}
                      <img
                        src={technology.image_path}
                        alt=""
                        onerror={() => markMediaFailed(technology.image_path)}
                      />
                    {/if}
                    <span>
                      <strong>{technology.name_ko}</strong>
                      <small>Lv. {unlock.level} · {unlock.cost} PT</small>
                    </span>
                    <b>기술 보기</b>
                  </a>
                </li>
              {/each}
            </ul>
          </section>
        {/if}

        {#if selected.kind === 'pal' && selected.work_suitability?.length}
          <section class="detail-section pal-work-section">
            <h3>작업 적성 <small>게임 기본 레벨</small></h3>
            <ul class="work-grid">
              {#each selected.work_suitability as work (work.id)}
                <li>
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
        {/if}

        {#if selected.kind === 'pal' && selected.pal_profile}
          <section class="detail-section pal-profile-section">
            <h3>이동·생활 <small>전체 팰 비교</small></h3>
            <dl class="profile-grid">
              {#if movementStanding(selected.pal_profile.run_speed, 'run_speed')}
                <div>
                  <dt>달리기</dt>
                  <dd>
                    <strong>{movementStanding(selected.pal_profile.run_speed, 'run_speed')}</strong>
                    <small>지수 {selected.pal_profile.run_speed.toLocaleString('ko-KR')}</small>
                  </dd>
                </div>
              {/if}
              {#if movementStanding(selected.pal_profile.ride_sprint_speed, 'ride_sprint_speed')}
                <div>
                  <dt>탑승 질주</dt>
                  <dd>
                    <strong
                      >{movementStanding(
                        selected.pal_profile.ride_sprint_speed,
                        'ride_sprint_speed',
                      )}</strong
                    >
                    <small
                      >지수 {selected.pal_profile.ride_sprint_speed.toLocaleString('ko-KR')}</small
                    >
                  </dd>
                </div>
              {/if}
              {#if movementStanding(selected.pal_profile.transport_speed, 'transport_speed')}
                <div>
                  <dt>운반 속도</dt>
                  <dd>
                    <strong
                      >{movementStanding(
                        selected.pal_profile.transport_speed,
                        'transport_speed',
                      )}</strong
                    >
                    <small
                      >지수 {selected.pal_profile.transport_speed.toLocaleString('ko-KR')}</small
                    >
                  </dd>
                </div>
              {/if}
              {#if movementStanding(selected.pal_profile.stamina, 'stamina') && selected.pal_profile.ride_sprint_speed > 0}
                <div>
                  <dt>탑승 스태미나</dt>
                  <dd>
                    <strong>{movementStanding(selected.pal_profile.stamina, 'stamina')}</strong>
                    <small>지수 {selected.pal_profile.stamina.toLocaleString('ko-KR')}</small>
                  </dd>
                </div>
              {/if}
              <div>
                <dt>식사량</dt>
                <dd>{selected.pal_profile.food_amount.toLocaleString('ko-KR')}단계</dd>
              </div>
            </dl>
          </section>
        {/if}

        {#if selected.kind === 'pal' && selected.pal_skills?.length}
          <section class="detail-section pal-skill-section">
            <h3>습득 액티브 스킬 <small>{selected.pal_skills.length}개</small></h3>
            <ol class="skill-list">
              {#each visiblePalSkills as skill (skill.id)}
                <li>
                  {#if mediaAvailable(skill.icon_path)}
                    <img
                      src={skill.icon_path}
                      alt=""
                      onerror={() => markMediaFailed(skill.icon_path)}
                    />
                  {/if}
                  <span>
                    <small>Lv. {skill.level} · {skill.element_name_ko}</small>
                    <strong>{skill.name_ko}</strong>
                  </span>
                  <span class="skill-values">
                    <small>위력 <b>{skill.power}</b></small>
                    <small>재사용 <b>{skill.cooldown_seconds}초</b></small>
                  </span>
                </li>
              {/each}
            </ol>
            {#if compact && selected.pal_skills.length > 4}
              <button
                class="detail-more"
                type="button"
                aria-expanded={skillsExpanded}
                onclick={() => (skillsExpanded = !skillsExpanded)}
              >
                {skillsExpanded
                  ? '스킬 접기'
                  : `나머지 ${(selected.pal_skills.length - 4).toString()}개 보기`}
              </button>
            {/if}
          </section>
        {/if}

        {#if selected.kind === 'pal' && selected.pal_drops?.length}
          <section class="detail-section pal-drop-section">
            <h3>드롭 아이템 <small>{selected.pal_drops.length}개 경로</small></h3>
            <ul class="drop-list">
              {#each selected.pal_drops as drop (`${drop.item_id}:${drop.variant}`)}
                <li style={`--record-accent:${rarityAccent(drop.rarity)}`}>
                  <a
                    class="relation-identity"
                    href={queryHref('/items/', drop.name_ko, drop.item_id)}
                  >
                    {#if mediaAvailable(drop.image_path)}
                      <img
                        src={drop.image_path}
                        alt=""
                        onerror={() => markMediaFailed(drop.image_path)}
                      />
                    {/if}
                    <span>
                      <strong>{drop.name_ko}</strong>
                      <small>{drop.variant === 'boss' ? '보스 개체' : '일반 개체'}</small>
                    </span>
                  </a>
                  <span class="drop-values">
                    <strong>{probabilityLabel(drop.probability_ppm)}</strong>
                    <small>{quantityLabel(drop.minimum_quantity, drop.maximum_quantity)}</small>
                  </span>
                </li>
              {/each}
            </ul>
          </section>
        {/if}

        {#if selected.kind === 'active_skill' && selected.learned_by_pals?.length}
          <section class="detail-section learned-by-section">
            <h3>습득 팰 <small>{selected.learned_by_pals.length}종</small></h3>
            <ul class="related-pal-grid">
              {#each selected.learned_by_pals as pal (pal.id)}
                <li>
                  <a href={queryHref('/pals/', pal.name_ko, pal.id)}>
                    {#if mediaAvailable(pal.image_path)}
                      <img
                        src={pal.image_path}
                        alt=""
                        onerror={() => markMediaFailed(pal.image_path)}
                      />
                    {/if}
                    <span>
                      <strong>{pal.name_ko}</strong>
                      {#if pal.paldex_number !== null}
                        <small>No. {pal.paldex_number.toString().padStart(3, '0')}</small>
                      {/if}
                    </span>
                  </a>
                </li>
              {/each}
            </ul>
          </section>
        {/if}

        {#if selected.kind !== 'pal' && selected.kind !== 'item' && visibleTags(selected).length > 0}
          <section class="detail-section">
            <h3>추가 분류</h3>
            <div class="detail-tags">
              {#each visibleTags(selected) as tag, tagIndex (`${tag}:${tagIndex.toString()}`)}<span
                  >{tag}</span
                >{/each}
            </div>
          </section>
        {/if}

        {#if selected.materials && selected.materials.length > 0}
          <section class="detail-section">
            <h3>건축 재료 <small>{selected.materials.length}종</small></h3>
            <ul class="product-list material-list">
              {#each selected.materials as material (material.item_id)}
                <li style={`--record-accent:${rarityAccent(material.rarity)}`}>
                  {#if mediaAvailable(material.image_path)}
                    <img
                      src={material.image_path}
                      alt=""
                      onerror={() => markMediaFailed(material.image_path)}
                    />
                  {/if}
                  <span><strong>{material.name_ko}</strong></span>
                  <b>× {material.quantity.toLocaleString('ko-KR')}</b>
                </li>
              {/each}
            </ul>
          </section>
        {/if}

        {#if selected.products && selected.products.length > 0}
          <section class="detail-section">
            <h3>판매품 <small>상위 24개</small></h3>
            <ul class="product-list">
              {#each selected.products.slice(0, 24) as product (product.product_id)}
                <li style={`--record-accent:${rarityAccent(product.rarity)}`}>
                  {#if mediaAvailable(product.image_path)}
                    <img
                      src={product.image_path}
                      alt=""
                      onerror={() => markMediaFailed(product.image_path)}
                    />
                  {/if}
                  <span><strong>{product.item_name_ko}</strong></span>
                  <b>{product.price.toLocaleString('ko-KR')}</b>
                </li>
              {/each}
            </ul>
          </section>
        {/if}

        {#if selected.target_path}
          <a class="open-source" href={resolve('/technology/', {})}>기술 트리에서 열기</a>
        {/if}
      </aside>
    {/if}
  </div>
</section>

<style>
  .catalog-screen {
    container-name: catalog;
    container-type: inline-size;
    padding: 14px clamp(12px, 1.8vw, 26px) 30px;
  }

  .page-head {
    display: flex;
    min-height: 58px;
    align-items: center;
    padding: 0 16px;
    border: 1px solid var(--border);
    border-radius: 7px 7px 0 0;
    background: var(--ink);
  }

  .kind-label {
    color: var(--tech-normal);
    font-size: 0.75rem;
    font-weight: 850;
    letter-spacing: 0.13em;
  }

  h1 {
    margin: 0;
    font-size: 1.2rem;
    letter-spacing: -0.02em;
  }

  dt {
    color: var(--muted);
    font-size: 0.75rem;
  }

  dd {
    margin: 5px 0 0;
    font-size: 0.75rem;
    font-weight: 800;
  }

  .toolbar {
    display: grid;
    grid-template-columns: minmax(260px, 1fr) minmax(180px, 240px) auto;
    gap: 12px;
    align-items: end;
    padding: 11px 14px;
    border: 1px solid var(--border);
    border-top: 0;
    background: var(--sidebar);
  }

  .search-field,
  .category-field {
    position: relative;
    display: grid;
    gap: 7px;
    color: var(--text-soft);
    font-size: 0.75rem;
    font-weight: 730;
  }

  input,
  select {
    min-width: 0;
    min-height: 42px;
    padding: 0 13px;
    border: 1px solid var(--border-strong);
    border-radius: 5px;
    background: var(--void);
    font-size: 0.82rem;
  }

  .search-field input {
    padding-right: 62px;
  }

  .search-field button {
    position: absolute;
    right: 7px;
    bottom: 7px;
    min-height: 32px;
    border: 0;
    border-radius: 4px;
    color: var(--muted-strong);
    background: var(--surface-raised);
    cursor: pointer;
    font-size: 0.75rem;
  }

  .result-summary {
    padding-bottom: 13px;
    color: var(--muted-strong);
    font-size: 0.75rem;
    white-space: nowrap;
  }

  .catalog-layout {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    gap: 16px;
    margin-top: 10px;
  }

  .catalog-layout.with-inspector {
    grid-template-columns: minmax(0, 1fr) minmax(320px, 360px);
  }

  .record-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(300px, 1fr));
    gap: 8px;
  }

  .virtual-scroller {
    height: min(780px, calc(100dvh - 290px));
    min-height: 420px;
    overflow: auto;
    padding-right: 6px;
    scrollbar-color: var(--border-strong) var(--void);
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
    padding-bottom: 8px;
  }

  .virtual-row .record-card {
    width: 100%;
  }

  .record-card {
    position: relative;
    display: grid;
    min-width: 0;
    min-height: 112px;
    grid-template-columns: 76px minmax(180px, 1fr) minmax(210px, auto);
    gap: 12px;
    align-items: center;
    padding: 12px;
    overflow: hidden;
    border: 1px solid var(--border);
    border-radius: 6px;
    color: inherit;
    background: color-mix(in srgb, var(--record-accent) 5%, var(--ink));
    cursor: pointer;
    text-align: left;
    transition:
      translate var(--motion-fast) var(--ease-standard),
      border-color var(--motion-base) var(--ease-standard),
      box-shadow var(--motion-base) var(--ease-standard),
      background-color var(--motion-base) var(--ease-standard);
    text-decoration: none;
  }

  .record-card.detail-link::after {
    position: absolute;
    right: 10px;
    bottom: 7px;
    color: var(--tech-normal);
    content: '상세 보기 ›';
    font-size: 0.68rem;
    font-weight: 780;
  }

  .record-card.no-media {
    grid-template-columns: minmax(180px, 1fr) minmax(210px, auto);
  }

  .record-card.no-metrics,
  .record-card.no-media.no-metrics {
    grid-template-columns: 76px minmax(0, 1fr);
  }

  .record-card.no-media.no-metrics {
    grid-template-columns: minmax(0, 1fr);
  }

  .record-card::before {
    position: absolute;
    inset: 0 auto 0 0;
    width: 3px;
    background: var(--record-accent);
    content: '';
    transition:
      width var(--motion-base) var(--ease-emphasized),
      filter var(--motion-base) var(--ease-standard);
  }

  .record-card:hover,
  .record-card.selected {
    border-color: color-mix(in srgb, var(--record-accent), white 20%);
    translate: 0 -1px;
  }

  .record-card.selected {
    box-shadow: inset 0 0 0 1px var(--focus);
  }

  .record-card:hover::before,
  .record-card.selected::before {
    width: 5px;
    filter: saturate(1.2) brightness(1.1);
  }

  .card-media,
  .inspector-media {
    display: grid;
    position: relative;
    width: 70px;
    height: 70px;
    place-items: center;
    overflow: hidden;
    border: 1px solid color-mix(in srgb, var(--record-accent), var(--border) 45%);
    border-radius: 5px;
    background: color-mix(in srgb, var(--record-accent) 8%, var(--void));
  }

  img {
    width: 100%;
    height: 100%;
    object-fit: contain;
  }

  .element-icon {
    position: absolute;
    right: 3px;
    bottom: 3px;
    width: 22px;
    height: 22px;
    padding: 2px;
    border: 1px solid rgb(255 255 255 / 34%);
    border-radius: 50%;
    background: rgb(3 10 14 / 82%);
  }

  .media-elements {
    position: absolute;
    right: 3px;
    bottom: 3px;
    display: flex;
    gap: 2px;
  }

  .media-elements img {
    width: 22px;
    height: 22px;
    padding: 2px;
    border: 1px solid rgb(255 255 255 / 34%);
    border-radius: 50%;
    background: rgb(3 10 14 / 88%);
  }

  .paldex-number {
    position: absolute;
    top: 3px;
    left: 3px;
    padding: 2px 4px;
    border-radius: 2px;
    color: var(--text);
    background: rgb(3 10 14 / 82%);
    font-family: 'Cascadia Mono', 'Consolas', monospace;
    font-size: 0.75rem;
    font-weight: 850;
  }

  .image-stack {
    position: relative;
    display: block;
    width: 100%;
    height: 100%;
  }

  .image-stack img {
    position: absolute;
    width: 48px;
    height: 48px;
    padding: 3px;
    border: 1px solid var(--border-strong);
    border-radius: 4px;
    background: var(--void);
  }

  .image-stack img:nth-child(1) {
    top: 3px;
    left: 3px;
  }

  .image-stack img:nth-child(2) {
    right: 3px;
    bottom: 3px;
  }

  .image-stack img:nth-child(3) {
    right: 1px;
    top: 1px;
    width: 34px;
    height: 34px;
  }

  .card-copy {
    display: grid;
    min-width: 0;
  }

  .card-copy strong {
    margin-top: 4px;
    overflow: hidden;
    font-size: 0.88rem;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .card-description {
    display: -webkit-box;
    max-width: 760px;
    margin-top: 5px;
    overflow: hidden;
    color: var(--muted-strong);
    font-size: 0.75rem;
    line-height: 1.45;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
  }

  .shop-product-preview {
    display: grid;
    min-width: 0;
    gap: 4px;
    margin-top: 7px;
  }

  .shop-product-preview > span {
    display: grid;
    min-width: 0;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: 8px;
    color: var(--text-soft);
    font-size: 0.75rem;
  }

  .shop-product-preview strong {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .shop-product-preview small {
    color: var(--text-soft);
    white-space: nowrap;
  }

  .pal-element-row {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    margin-top: 5px;
  }

  .pal-element-row > span,
  .element-list > span {
    display: inline-flex;
    min-height: 22px;
    align-items: center;
    gap: 4px;
    padding: 2px 6px 2px 3px;
    border: 1px solid var(--border);
    border-radius: 999px;
    color: var(--text-soft);
    background: rgb(255 255 255 / 3%);
    font-size: 0.75rem;
    font-weight: 760;
  }

  .pal-element-row img,
  .element-list img {
    width: 16px;
    height: 16px;
  }

  .pal-element-row .nocturnal {
    padding-left: 6px;
    color: #c8b9ff;
    background: rgb(132 105 217 / 12%);
  }

  .work-row {
    display: flex;
    min-width: 0;
    flex-wrap: wrap;
    gap: 4px;
    margin-top: 7px;
  }

  .work-row > span {
    display: inline-grid;
    min-width: 0;
    min-height: 26px;
    grid-template-columns: 18px minmax(0, auto) auto;
    align-items: center;
    gap: 4px;
    padding: 3px 6px 3px 4px;
    border: 1px solid color-mix(in srgb, var(--tech-normal), var(--border) 74%);
    border-radius: 3px;
    background: color-mix(in srgb, var(--tech-normal) 5%, var(--void));
  }

  .work-row img {
    width: 18px;
    height: 18px;
  }

  .work-row small {
    overflow: hidden;
    color: var(--muted-strong);
    font-size: 0.75rem;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .work-row b {
    color: var(--text);
    font-size: 0.75rem;
  }

  .work-row .work-more {
    display: inline-flex;
    min-width: 28px;
    justify-content: center;
    color: var(--muted-strong);
    font-size: 0.75rem;
    font-weight: 850;
  }

  .tag-row,
  .detail-tags {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    margin-top: 8px;
  }

  .tag-row span,
  .detail-tags span {
    padding: 3px 6px;
    border: 1px solid var(--border);
    border-radius: 3px;
    color: var(--muted-strong);
    background: rgb(255 255 255 / 2%);
    font-size: 0.75rem;
  }

  .tag-row .rarity-tag {
    border-color: color-mix(in srgb, var(--rarity-color), var(--border) 35%);
    color: var(--rarity-color);
    background: color-mix(in srgb, var(--rarity-color) 10%, var(--void));
  }

  .tag-row .relation-tag {
    border-color: color-mix(in srgb, var(--tech-normal), var(--border) 55%);
    color: var(--tech-normal);
    background: color-mix(in srgb, var(--tech-normal) 7%, var(--void));
  }

  .card-metrics {
    display: grid;
    grid-template-columns: repeat(3, minmax(66px, 1fr));
    gap: 5px;
    min-width: 210px;
  }

  .card-metrics span {
    display: grid;
    min-height: 54px;
    align-content: center;
    padding: 7px 9px;
    border: 1px solid var(--border);
    border-radius: 4px;
    background: rgb(255 255 255 / 2%);
    text-align: center;
  }

  .card-metrics small {
    color: var(--muted);
    font-size: 0.75rem;
  }

  .card-metrics strong {
    margin-top: 2px;
    color: var(--text-soft);
    font-size: 0.75rem;
  }

  .load-more,
  .empty-state button,
  .open-source,
  .detail-more {
    display: flex;
    min-height: 44px;
    align-items: center;
    justify-content: center;
    margin-top: 14px;
    padding-inline: 16px;
    border: 1px solid var(--border-strong);
    border-radius: 5px;
    color: var(--text);
    background: var(--surface-raised);
    cursor: pointer;
    text-decoration: none;
    font-size: 0.75rem;
    font-weight: 750;
  }

  .load-more {
    width: 100%;
  }

  .detail-more {
    width: 100%;
    margin-top: 8px;
  }

  .empty-state {
    display: grid;
    min-height: 320px;
    place-items: center;
    align-content: center;
    border: 1px solid var(--border);
    text-align: center;
  }

  .empty-state h2 {
    margin: 0 0 4px;
    font-size: 1rem;
  }

  .empty-state p {
    margin: 0;
    color: var(--muted-strong);
    font-size: 0.76rem;
  }

  .inspector {
    position: sticky;
    top: 82px;
    align-self: start;
    max-height: calc(100vh - 104px);
    overflow: auto;
    border: 1px solid var(--border-strong);
    border-radius: 7px;
    background: var(--ink);
  }

  .inspector > header {
    display: grid;
    grid-template-columns: 74px minmax(0, 1fr) auto;
    gap: 12px;
    align-items: center;
    padding: 16px;
    border-bottom: 1px solid var(--border);
  }

  .inspector > header.no-media {
    grid-template-columns: minmax(0, 1fr) auto;
  }

  .inspector-media {
    width: 74px;
    height: 74px;
  }

  .inspector header small {
    color: var(--tech-normal);
    font-size: 0.75rem;
    font-weight: 800;
  }

  .inspector h2 {
    margin: 5px 0;
    font-size: 1rem;
  }

  .inspector header button {
    min-width: 44px;
    min-height: 40px;
    border: 1px solid var(--border);
    border-radius: 4px;
    background: var(--surface-raised);
    cursor: pointer;
    font-size: 0.75rem;
  }

  .description {
    margin: 0;
    padding: 18px;
    color: var(--text-soft);
    font-size: 0.77rem;
    line-height: 1.7;
    white-space: pre-line;
  }

  .pal-status-strip {
    display: flex;
    min-height: 44px;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    padding: 8px 14px;
    border-top: 1px solid var(--border);
    background: color-mix(in srgb, var(--accent), var(--ink) 94%);
  }

  .item-decision-strip {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(88px, 1fr));
    margin: 0;
    border-top: 1px solid var(--border);
    border-bottom: 1px solid var(--border);
    background: var(--surface);
  }

  .item-decision-strip div {
    display: grid;
    min-height: 58px;
    align-content: center;
    gap: 4px;
    padding: 9px 12px;
    border-right: 1px solid var(--border);
  }

  .item-decision-strip div:last-child {
    border-right: 0;
  }

  .item-decision-strip dt {
    color: var(--muted);
    font-size: 0.75rem;
  }

  .item-decision-strip dd {
    margin: 0;
    color: var(--text-soft);
    font-size: 0.75rem;
    font-weight: 820;
  }

  .item-decision-strip .rarity-decision {
    background: color-mix(in srgb, var(--rarity-color) 9%, var(--surface));
  }

  .item-decision-strip .rarity-decision dd {
    color: var(--rarity-color);
  }

  .element-list {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }

  .pal-status-strip > strong {
    flex: none;
    color: #c8b9ff;
    font-size: 0.75rem;
  }

  .metric-grid {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    margin: 0;
    border-top: 1px solid var(--border);
    border-bottom: 1px solid var(--border);
  }

  .metric-grid div {
    padding: 12px 14px;
    border-right: 1px solid var(--border);
    border-bottom: 1px solid var(--border);
  }

  .metric-grid div:nth-child(2n) {
    border-right: 0;
  }

  .detail-section {
    padding: 16px;
    border-bottom: 1px solid var(--border);
  }

  .detail-section h3 {
    margin: 0;
    font-size: 0.75rem;
  }

  .detail-section h3 small {
    color: var(--muted);
    font-weight: 500;
  }

  .recipe-list {
    display: grid;
    gap: 8px;
    margin-top: 12px;
  }

  .recipe-card {
    overflow: hidden;
    border: 1px solid var(--border);
    border-radius: 5px;
    background: var(--surface);
  }

  .recipe-card > header {
    display: flex;
    min-height: 38px;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    padding: 7px 10px;
    border-bottom: 1px solid var(--border);
    color: var(--muted-strong);
    font-size: 0.75rem;
  }

  .recipe-card > header strong {
    color: var(--tech-normal);
    font-size: 0.75rem;
  }

  .ingredient-list,
  .drop-list,
  .shop-offer-list,
  .technology-relation-list,
  .related-pal-grid {
    display: grid;
    gap: 1px;
    margin: 0;
    padding: 0;
    background: var(--border);
    list-style: none;
  }

  .ingredient-list li,
  .drop-list li,
  .shop-offer-list li,
  .technology-relation-list li,
  .related-pal-grid li {
    position: relative;
    min-width: 0;
    background: var(--surface);
  }

  .ingredient-list li {
    display: grid;
    min-height: 50px;
    grid-template-columns: minmax(0, 1fr) auto;
    align-items: center;
    gap: 10px;
    padding: 6px 9px;
  }

  .ingredient-list li::before {
    position: absolute;
    inset: 0 auto 0 0;
    width: 2px;
    background: var(--record-accent);
    content: '';
  }

  .ingredient-list a,
  .relation-identity,
  .shop-offer,
  .technology-relation-list a,
  .related-pal-grid a {
    color: inherit;
    text-decoration: none;
  }

  .ingredient-list a,
  .relation-identity {
    display: grid;
    min-width: 0;
    grid-template-columns: 36px minmax(0, 1fr);
    align-items: center;
    gap: 8px;
  }

  .ingredient-list img,
  .relation-identity img,
  .technology-relation-list img,
  .related-pal-grid img {
    width: 36px;
    height: 36px;
    object-fit: contain;
  }

  .ingredient-list a span,
  .relation-copy {
    overflow: hidden;
    color: var(--text-soft);
    font-size: 0.75rem;
    font-weight: 720;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .ingredient-list b {
    color: var(--text);
    font-size: 0.75rem;
  }

  .drop-list,
  .shop-offer-list,
  .technology-relation-list,
  .related-pal-grid {
    margin-top: 12px;
  }

  .drop-list li {
    display: grid;
    min-height: 58px;
    grid-template-columns: minmax(0, 1fr) auto;
    align-items: center;
    gap: 10px;
    padding: 8px 10px;
  }

  .relation-identity span,
  .shop-offer span,
  .technology-relation-list a span,
  .related-pal-grid a span {
    display: grid;
    min-width: 0;
    gap: 2px;
  }

  .relation-identity strong,
  .shop-offer-list strong,
  .technology-relation-list strong,
  .related-pal-grid strong {
    overflow: hidden;
    color: var(--text-soft);
    font-size: 0.75rem;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .relation-identity small,
  .shop-offer-list small,
  .technology-relation-list small,
  .related-pal-grid small {
    color: var(--muted);
    font-size: 0.75rem;
  }

  .drop-values {
    display: grid;
    gap: 2px;
    text-align: right;
  }

  .drop-values strong {
    color: var(--brass);
    font-size: 0.75rem;
  }

  .drop-values small {
    color: var(--muted-strong);
    font-size: 0.75rem;
  }

  .shop-offer,
  .technology-relation-list a {
    display: grid;
    min-height: 54px;
    grid-template-columns: minmax(0, 1fr) auto;
    align-items: center;
    gap: 10px;
    padding: 8px 10px;
  }

  .shop-offer > b,
  .technology-relation-list a > b {
    color: var(--text-soft);
    font-size: 0.75rem;
    text-align: right;
  }

  .technology-relation-list a {
    grid-template-columns: 38px minmax(0, 1fr) auto;
  }

  .related-pal-grid {
    max-height: 360px;
    grid-template-columns: repeat(auto-fill, minmax(150px, 1fr));
    overflow-y: auto;
  }

  .related-pal-grid a {
    display: grid;
    min-height: 54px;
    grid-template-columns: 38px minmax(0, 1fr);
    align-items: center;
    gap: 8px;
    padding: 8px 10px;
  }

  .ingredient-list a:hover,
  .relation-identity:hover,
  .technology-relation-list a:hover,
  .related-pal-grid a:hover {
    color: var(--tech-normal);
  }

  .work-grid,
  .skill-list {
    display: grid;
    gap: 5px;
    margin: 12px 0 0;
    padding: 0;
    list-style: none;
  }

  .work-grid {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }

  .work-grid li {
    display: grid;
    min-width: 0;
    min-height: 46px;
    grid-template-columns: 28px minmax(0, 1fr) auto;
    align-items: center;
    gap: 7px;
    padding: 7px;
    border: 1px solid var(--border);
    border-radius: 4px;
    background: var(--surface);
  }

  .work-grid img {
    width: 28px;
    height: 28px;
  }

  .work-grid strong,
  .work-grid b {
    font-size: 0.75rem;
  }

  .work-grid b {
    color: var(--tech-normal);
  }

  .profile-grid {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    margin: 12px 0 0;
    border: 1px solid var(--border);
  }

  .profile-grid div {
    padding: 9px 10px;
    border-right: 1px solid var(--border);
    border-bottom: 1px solid var(--border);
  }

  .profile-grid div:nth-child(2n) {
    border-right: 0;
  }

  .profile-grid dt {
    font-size: 0.75rem;
  }

  .profile-grid dd {
    display: grid;
    gap: 2px;
    margin-top: 3px;
    color: var(--text-soft);
    font-size: 0.75rem;
  }

  .profile-grid dd strong {
    color: var(--tech-normal);
  }

  .profile-grid dd small {
    color: var(--muted);
    font-size: 0.75rem;
  }

  .skill-list li {
    display: grid;
    min-width: 0;
    grid-template-columns: 38px minmax(0, 1fr) auto;
    align-items: center;
    gap: 9px;
    padding: 8px;
    border: 1px solid var(--border);
    border-radius: 4px;
    background: var(--surface);
  }

  .skill-list img {
    width: 38px;
    height: 38px;
  }

  .skill-list li > span:not(.skill-values) {
    display: grid;
    min-width: 0;
  }

  .skill-list strong {
    overflow: hidden;
    font-size: 0.75rem;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .skill-list small {
    color: var(--muted);
    font-size: 0.75rem;
  }

  .skill-values {
    display: grid;
    gap: 3px;
    text-align: right;
  }

  .skill-values b {
    color: var(--text-soft);
  }

  .product-list {
    display: grid;
    gap: 1px;
    margin: 12px 0 0;
    padding: 0;
    background: var(--border);
    list-style: none;
  }

  .product-list li {
    display: grid;
    min-height: 52px;
    grid-template-columns: 38px minmax(0, 1fr) auto;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    padding: 8px 10px;
    background: var(--surface);
    font-size: 0.75rem;
  }

  .product-list li::before {
    position: absolute;
    inset: 0 auto 0 0;
    width: 2px;
    background: var(--record-accent);
    content: '';
  }

  .product-list li {
    position: relative;
  }

  .product-list li:not(:has(img)) {
    grid-template-columns: minmax(0, 1fr) auto;
  }

  .product-list img {
    width: 36px;
    height: 36px;
    object-fit: contain;
  }

  .product-list b {
    color: var(--text-soft);
    font-size: 0.75rem;
  }

  .product-list li > span:first-child {
    display: grid;
    min-width: 0;
  }

  .open-source {
    margin: 16px;
  }

  @media (max-width: 1120px) {
    .page-head {
      grid-template-columns: 1fr;
    }

    .catalog-layout.with-inspector {
      grid-template-columns: minmax(0, 1fr) 320px;
    }

    .record-grid {
      grid-template-columns: 1fr;
    }

    .record-card,
    .record-card.no-media {
      grid-template-columns: 70px minmax(0, 1fr) minmax(200px, 210px);
    }

    .record-card.no-media {
      grid-template-columns: minmax(0, 1fr) minmax(200px, 210px);
    }

    .card-metrics {
      min-width: 200px;
      grid-template-columns: repeat(3, minmax(60px, 1fr));
    }

    .catalog-layout.with-inspector .record-card {
      grid-template-columns: 70px minmax(0, 1fr) minmax(150px, 170px);
    }

    .catalog-layout.with-inspector .record-card.no-media {
      grid-template-columns: minmax(0, 1fr) minmax(150px, 170px);
    }

    .catalog-layout.with-inspector .card-metrics {
      min-width: 150px;
      grid-template-columns: repeat(2, minmax(68px, 1fr));
    }

    .catalog-layout.with-inspector .card-metrics > span:nth-child(3) {
      display: none;
    }
  }

  @container catalog (max-width: 820px) {
    .toolbar {
      grid-template-columns: minmax(0, 1fr) minmax(160px, 200px);
    }

    .result-summary {
      grid-column: 1 / -1;
      padding-bottom: 0;
    }

    .catalog-layout.with-inspector {
      grid-template-columns: 1fr;
    }

    .inspector {
      position: static;
      max-height: none;
      grid-row: 1;
    }

    .catalog-layout.with-inspector .record-card {
      grid-template-columns: 70px minmax(0, 1fr) minmax(200px, 210px);
    }

    .catalog-layout.with-inspector .record-card.no-media {
      grid-template-columns: minmax(0, 1fr) minmax(200px, 210px);
    }

    .catalog-layout.with-inspector .card-metrics {
      min-width: 200px;
      grid-template-columns: repeat(3, minmax(60px, 1fr));
    }

    .catalog-layout.with-inspector .card-metrics > span:nth-child(3) {
      display: grid;
    }
  }

  @container catalog (max-width: 620px) {
    .page-head {
      gap: 10px;
      padding: 13px;
    }

    .search-field > span,
    .category-field > span {
      position: absolute;
      width: 1px;
      height: 1px;
      padding: 0;
      overflow: hidden;
      clip: rect(0 0 0 0);
      white-space: nowrap;
    }

    .toolbar {
      grid-template-columns: 1fr;
    }

    .result-summary {
      grid-column: auto;
    }

    .record-card,
    .record-card.no-media,
    .catalog-layout.with-inspector .record-card,
    .catalog-layout.with-inspector .record-card.no-media {
      min-height: 104px;
      grid-template-columns: 62px minmax(0, 1fr) 72px;
      gap: 9px;
      padding: 10px;
    }

    .record-card.no-media,
    .catalog-layout.with-inspector .record-card.no-media {
      grid-template-columns: minmax(0, 1fr) 72px;
    }

    .record-card.no-metrics,
    .record-card.no-media.no-metrics,
    .catalog-layout.with-inspector .record-card.no-metrics,
    .catalog-layout.with-inspector .record-card.no-media.no-metrics {
      grid-template-columns: minmax(0, 1fr);
    }

    .card-description,
    .card-metrics > span:nth-child(n + 2) {
      display: none;
    }

    .description-priority .card-description {
      display: -webkit-box;
    }

    .pal-record .card-metrics,
    .catalog-layout.with-inspector .pal-record .card-metrics {
      display: grid;
      min-width: 0;
      grid-column: 2 / -1;
      grid-template-columns: repeat(3, minmax(0, 1fr));
      gap: 3px;
    }

    .pal-record .card-metrics > span:nth-child(n + 2) {
      display: grid;
    }

    .pal-record .card-metrics span {
      min-height: 30px;
      padding: 3px 5px;
    }

    .card-metrics,
    .catalog-layout.with-inspector .card-metrics {
      min-width: 72px;
      grid-template-columns: 1fr;
    }

    .item-decision-strip {
      grid-template-columns: repeat(2, minmax(0, 1fr));
    }

    .item-decision-strip div:nth-child(2n) {
      border-right: 0;
    }

    .drop-list li,
    .shop-offer,
    .technology-relation-list a {
      align-items: start;
    }

    .shop-offer,
    .technology-relation-list a {
      grid-template-columns: minmax(0, 1fr);
    }

    .technology-relation-list a:has(img) {
      grid-template-columns: 38px minmax(0, 1fr);
    }

    .shop-offer > b,
    .technology-relation-list a > b {
      grid-column: 1 / -1;
      text-align: left;
    }
  }

  @media (max-width: 719px) {
    .catalog-screen {
      padding: 12px 10px 82px;
    }

    .page-head {
      min-height: 54px;
      padding: 0 14px;
    }

    h1 {
      font-size: 1.1rem;
    }

    .toolbar {
      grid-template-columns: 1fr;
    }

    .result-summary {
      padding-bottom: 0;
    }

    .record-grid {
      grid-template-columns: minmax(0, 1fr);
    }

    .record-card,
    .record-card.no-media {
      min-height: 104px;
      grid-template-columns: 62px minmax(0, 1fr) 72px;
      gap: 9px;
      padding: 10px;
    }

    .record-card.no-media {
      grid-template-columns: minmax(0, 1fr) 72px;
    }

    .card-media {
      width: 58px;
      height: 58px;
    }

    .card-description,
    .card-metrics > span:nth-child(n + 2) {
      display: none;
    }

    .work-row > span {
      grid-template-columns: 18px auto;
    }

    .work-row small {
      display: none;
    }

    .card-metrics {
      display: grid;
      min-width: 72px;
      grid-template-columns: 1fr;
    }

    .card-metrics span {
      min-height: 48px;
      padding: 5px;
    }

    .kind-label {
      letter-spacing: 0.06em;
    }

    .inspector > header {
      grid-template-columns: 62px minmax(0, 1fr) auto;
    }

    .inspector-media {
      width: 62px;
      height: 62px;
    }

    .work-grid {
      grid-template-columns: 1fr;
    }
  }

  @media (max-width: 380px) {
    .record-card,
    .record-card.no-media {
      grid-template-columns: 54px minmax(0, 1fr);
    }

    .record-card.no-media {
      grid-template-columns: minmax(0, 1fr);
    }

    .card-metrics {
      display: none;
    }

    .pal-record .card-metrics {
      display: grid;
    }
  }
</style>
