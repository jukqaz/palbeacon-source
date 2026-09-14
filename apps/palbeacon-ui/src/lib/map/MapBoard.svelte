<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { Dialog } from 'bits-ui';
  import Panzoom from '@panzoom/panzoom/dist/panzoom.es.js';
  import type { PanzoomEventDetail, PanzoomObject } from '@panzoom/panzoom';
  import {
    declutterMapPois,
    defaultMapFilterIds,
    filterMapPois,
    levelZeroTiles,
    mapFilterColor,
    mapFilterOptions,
    mapAreaLabel,
    mapIndexForRegion,
    mapPoisForRegion,
    mapRegions,
    poiIconPath,
    publicSupplementalFilterIds,
    termById,
    tilePublicPath,
  } from './map';
  import { SvelteSet } from 'svelte/reactivity';
  import { exactPoiKinds } from './overlay-filters';
  import type { MapBundle, MapPoi } from './types';

  const MIN_RELATIVE_ZOOM = 1;
  const MAX_RELATIVE_ZOOM = 8;
  const ZOOM_FACTOR = 1.35;

  interface Props {
    bundle: MapBundle;
    initialQuery?: string;
    initialEnabledFilterIds?: readonly string[] | undefined;
    onEnabledFilterIdsChange?: (enabledFilterIds: readonly string[]) => void;
  }

  let {
    bundle,
    initialQuery = '',
    initialEnabledFilterIds,
    onEnabledFilterIdsChange,
  }: Props = $props();
  let regionId = $state('main');
  let query = $state((() => initialQuery)());
  const enabledFilterIds = new SvelteSet<string>();
  let selectedPoiId = $state<string | null>(null);
  let zoom = $state(1);
  let baseZoom = $state(1);
  let coarsePointer = $state(false);
  let touchInteractionEnabled = $state(false);
  let filtersOpen = $state(false);
  let mobileSheet = $state(false);
  let activeMarkerId = $state<string | null>(null);
  let focusSelectedAfterClose = false;
  // oxlint-disable-next-line no-unassigned-vars -- Svelte assigns bind:this during mount.
  let filterTriggerButton: HTMLButtonElement;
  let filterCloseButton = $state<HTMLButtonElement>();
  // oxlint-disable-next-line no-unassigned-vars -- Svelte assigns bind:this during mount.
  let mapSvg: SVGSVGElement;
  // oxlint-disable-next-line no-unassigned-vars -- Svelte assigns bind:this during mount.
  let mapViewport: SVGGElement;
  let selectedInspector = $state<HTMLElement>();
  let panzoom: PanzoomObject | null = null;
  let focusFrame: number | null = null;

  const region = $derived(
    mapRegions.find((candidate) => candidate.id === regionId) ?? mapRegions[0],
  );
  const terms = $derived(termById(bundle));
  const regionPois = $derived(mapPoisForRegion(bundle, region));
  const filterOptions = $derived(mapFilterOptions(bundle, region, regionPois));
  const filterGroups = $derived(
    bundle.terminology.groups
      .map((group) => ({
        ...group,
        options: filterOptions.filter((option) => option.groupId === group.id),
      }))
      .filter((group) => group.options.length > 0),
  );
  const allFilterIds = $derived([...exactPoiKinds, ...publicSupplementalFilterIds(bundle)]);
  const activeRegionFilterCount = $derived(
    filterOptions.filter((option) => enabledFilterIds.has(option.id)).length,
  );
  const filteredPois = $derived(filterMapPois(regionPois, terms, enabledFilterIds, query));
  const relativeZoom = $derived(baseZoom > 0 ? zoom / baseZoom : 1);
  const visiblePois = $derived(declutterMapPois(filteredPois, relativeZoom, selectedPoiId));
  const resultPois = $derived.by(() => {
    const limit = query.trim().length > 0 ? 36 : 12;
    const seenSupplementalKinds = new Set<string>();
    return filteredPois
      .filter((poi) => {
        if (poi.source === 'exact') return true;
        if (seenSupplementalKinds.has(poi.kind)) return false;
        seenSupplementalKinds.add(poi.kind);
        return true;
      })
      .slice(0, limit);
  });
  const index = $derived(mapIndexForRegion(bundle, region));
  const tiles = $derived(levelZeroTiles(index));
  const selectedPoi = $derived(
    selectedPoiId === null ? null : (regionPois.find((poi) => poi.id === selectedPoiId) ?? null),
  );
  const markerTabStopId = $derived.by(() => {
    if (activeMarkerId && visiblePois.some((poi) => poi.id === activeMarkerId)) {
      return activeMarkerId;
    }
    if (selectedPoiId && visiblePois.some((poi) => poi.id === selectedPoiId)) {
      return selectedPoiId;
    }
    return visiblePois[0]?.id ?? null;
  });
  const markerSize = $derived(78 / zoom);

  const kindColor = (kind: string): string => mapFilterColor(kind, terms);

  const publishEnabledFilters = () => {
    onEnabledFilterIdsChange?.(allFilterIds.filter((candidate) => enabledFilterIds.has(candidate)));
  };

  const toggleFilter = (filterId: string) => {
    if (enabledFilterIds.has(filterId)) {
      enabledFilterIds.delete(filterId);
      if (selectedPoi?.kind === filterId) selectedPoiId = null;
    } else {
      enabledFilterIds.add(filterId);
    }
    publishEnabledFilters();
  };

  const replaceEnabledFilters = (nextIds: readonly string[]) => {
    enabledFilterIds.clear();
    for (const id of nextIds) enabledFilterIds.add(id);
    if (selectedPoi && !enabledFilterIds.has(selectedPoi.kind)) selectedPoiId = null;
    publishEnabledFilters();
  };

  const openFilters = async () => {
    filtersOpen = true;
    await tick();
    if (mobileSheet) return;
    const closeButton = filterCloseButton;
    requestAnimationFrame(() => {
      if (closeButton?.isConnected) closeButton.focus({ preventScroll: true });
    });
  };

  const closeFilters = async () => {
    filtersOpen = false;
    await tick();
    if (!mobileSheet) filterTriggerButton.focus();
  };

  const dialogCloseAutoFocus = (event: Event) => {
    event.preventDefault();
    const shouldRevealSelection = focusSelectedAfterClose;
    const focusTarget = shouldRevealSelection ? selectedInspector : filterTriggerButton;
    focusSelectedAfterClose = false;
    requestAnimationFrame(() => {
      focusTarget?.focus({ preventScroll: !shouldRevealSelection });
      if (shouldRevealSelection) focusTarget?.scrollIntoView?.({ block: 'start' });
    });
  };

  const filterPanelKeydown = (event: KeyboardEvent) => {
    if (event.key !== 'Escape' || !filtersOpen || mobileSheet) return;
    event.preventDefault();
    void closeFilters();
  };

  const applyPurpose = (purpose: 'location' | 'resource' | 'enemy' | 'collectible') => {
    const groupIds = purpose === 'location' ? new Set(['location', 'npc']) : new Set([purpose]);
    replaceEnabledFilters(
      filterOptions.filter((option) => groupIds.has(option.groupId)).map((option) => option.id),
    );
  };

  const purposeSelected = (purpose: 'location' | 'resource' | 'enemy' | 'collectible') => {
    const groupIds = purpose === 'location' ? new Set(['location', 'npc']) : new Set([purpose]);
    const purposeIds = filterOptions
      .filter((option) => groupIds.has(option.groupId))
      .map((option) => option.id);
    return (
      purposeIds.length > 0 &&
      purposeIds.every((id) => enabledFilterIds.has(id)) &&
      filterOptions
        .filter((option) => !groupIds.has(option.groupId))
        .every((option) => !enabledFilterIds.has(option.id))
    );
  };

  $effect(() => {
    const requestedIds = initialEnabledFilterIds ?? defaultMapFilterIds(bundle);
    const availableIds = new Set(allFilterIds);
    enabledFilterIds.clear();
    for (const id of requestedIds) {
      if (availableIds.has(id)) enabledFilterIds.add(id);
    }
  });

  const motionAllowed = (): boolean =>
    !window.matchMedia('(prefers-reduced-motion: reduce)').matches;

  const resetView = () => {
    const reset = panzoom?.reset({ animate: motionAllowed() });
    if (reset) zoom = reset.scale;
  };

  const changeRegion = (next: string) => {
    regionId = next;
    selectedPoiId = null;
    resetView();
  };

  const setZoom = (next: number) => {
    const constrained = Math.max(MIN_RELATIVE_ZOOM, Math.min(MAX_RELATIVE_ZOOM, next));
    const absoluteScale = baseZoom * constrained;
    if (panzoom) {
      if (selectedPoi) {
        focusSelectedPoint(constrained);
        zoom = panzoom.getScale();
        return;
      }
      const bounds = mapSvg.getBoundingClientRect();
      const applied = panzoom.zoomToPoint(
        absoluteScale,
        {
          clientX: bounds.left + bounds.width / 2,
          clientY: bounds.top + bounds.height / 2,
        },
        { animate: motionAllowed() },
      );
      zoom = applied.scale;
    } else {
      zoom = absoluteScale;
    }
  };

  const focusSelectedPoint = (nextZoom: number) => {
    if (!panzoom) return;
    const absoluteScale =
      baseZoom * Math.max(MIN_RELATIVE_ZOOM, Math.min(MAX_RELATIVE_ZOOM, nextZoom));
    panzoom.zoom(absoluteScale, { animate: false });
    if (focusFrame !== null) cancelAnimationFrame(focusFrame);
    focusFrame = requestAnimationFrame(() => {
      focusFrame = null;
      if (!panzoom) return;
      const marker = [...mapSvg.querySelectorAll<SVGGElement>('[data-poi-marker]')].find(
        (candidate) => candidate.dataset['poiId'] === selectedPoiId,
      );
      if (!marker) return;
      const markerBounds = marker.getBoundingClientRect();
      const bounds = mapSvg.getBoundingClientRect();
      const viewBox = mapSvg.viewBox.baseVal;
      const viewportScale = Math.min(bounds.width / viewBox.width, bounds.height / viewBox.height);
      const current = panzoom.getPan();
      const appliedScale = panzoom.getScale();
      panzoom.pan(
        current.x +
          (bounds.left + bounds.width / 2 - (markerBounds.left + markerBounds.width / 2)) /
            appliedScale /
            viewportScale,
        current.y +
          (bounds.top + bounds.height / 2 - (markerBounds.top + markerBounds.height / 2)) /
            appliedScale /
            viewportScale,
        { force: true, animate: false },
      );
    });
  };

  const focusPoi = async (poi: MapPoi) => {
    activeMarkerId = poi.id;
    selectedPoiId = poi.id;
    const closeMobileSheet = mobileSheet && filtersOpen;
    if (closeMobileSheet) {
      focusSelectedAfterClose = true;
      filtersOpen = false;
    }
    await tick();
    if (selectedPoiId !== poi.id) return;
    const nextZoom = Math.max(relativeZoom, 2.4);
    requestAnimationFrame(() => {
      if (selectedPoiId !== poi.id) return;
      focusSelectedPoint(nextZoom);
      if (closeMobileSheet) {
        selectedInspector?.focus({ preventScroll: true });
        selectedInspector?.scrollIntoView?.({ block: 'start' });
      }
    });
  };

  const setTouchInteraction = (enabled: boolean) => {
    touchInteractionEnabled = enabled;
    panzoom?.setOptions({
      disablePan: coarsePointer && !enabled,
      touchAction: coarsePointer && !enabled ? 'pan-y' : 'none',
    });
  };

  const markerKeydown = (event: KeyboardEvent, poi: MapPoi) => {
    if (event.key === 'Enter' || event.key === ' ') {
      event.preventDefault();
      focusPoi(poi);
      return;
    }
    const direction =
      event.key === 'ArrowRight' || event.key === 'ArrowDown'
        ? 1
        : event.key === 'ArrowLeft' || event.key === 'ArrowUp'
          ? -1
          : 0;
    if (direction === 0 && event.key !== 'Home' && event.key !== 'End') return;
    event.preventDefault();
    const currentIndex = visiblePois.findIndex((candidate) => candidate.id === poi.id);
    const targetIndex =
      event.key === 'Home'
        ? 0
        : event.key === 'End'
          ? visiblePois.length - 1
          : (currentIndex + direction + visiblePois.length) % visiblePois.length;
    const target = visiblePois[targetIndex];
    if (!target) return;
    activeMarkerId = target.id;
    void tick().then(() => {
      const marker = [...mapSvg.querySelectorAll<SVGGElement>('[data-poi-marker]')].find(
        (candidate) => candidate.dataset['poiId'] === target.id,
      );
      marker?.focus({ preventScroll: true });
      return undefined;
    });
  };

  onMount(() => {
    const mobileQuery = window.matchMedia('(max-width: 719px)');
    const syncMobileSheet = () => {
      mobileSheet = mobileQuery.matches;
      if (!mobileSheet && filtersOpen) filtersOpen = false;
    };
    syncMobileSheet();
    mobileQuery.addEventListener('change', syncMobileSheet);
    return () => mobileQuery.removeEventListener('change', syncMobileSheet);
  });

  onMount(() => {
    coarsePointer = window.matchMedia('(pointer: coarse)').matches;
    panzoom = Panzoom(mapViewport, {
      minScale: MIN_RELATIVE_ZOOM,
      maxScale: MAX_RELATIVE_ZOOM,
      step: 0.18,
      canvas: true,
      panOnlyWhenZoomed: true,
      excludeClass: 'panzoom-exclude',
      disablePan: coarsePointer,
      touchAction: coarsePointer ? 'pan-y' : 'none',
    });
    baseZoom = panzoom.getScale();
    zoom = baseZoom;
    panzoom.setOptions({ maxScale: baseZoom * MAX_RELATIVE_ZOOM });
    const handleChange = (event: Event) => {
      zoom = (event as CustomEvent<PanzoomEventDetail>).detail.scale;
    };
    const handleWheel = (event: WheelEvent) => {
      const delta = event.deltaY === 0 && event.deltaX ? event.deltaX : event.deltaY;
      if (delta > 0 && relativeZoom <= 1.001) return;
      panzoom?.zoomWithWheel(event);
    };
    mapViewport.addEventListener('panzoomchange', handleChange);
    mapSvg.addEventListener('wheel', handleWheel, { passive: false });
    return () => {
      mapViewport.removeEventListener('panzoomchange', handleChange);
      mapSvg.removeEventListener('wheel', handleWheel);
      panzoom?.destroy();
      panzoom?.resetStyle();
      panzoom = null;
      if (focusFrame !== null) cancelAnimationFrame(focusFrame);
      focusFrame = null;
    };
  });
</script>

<svelte:window onkeydown={filterPanelKeydown} />

{#snippet mapResults()}
  <section class="results" aria-label="지도 검색 결과">
    <header>
      <h2>{query.trim() ? '검색 결과' : '빠른 위치'}</h2>
      <small aria-live="polite">{resultPois.length}/{filteredPois.length}</small>
    </header>
    {#if resultPois.length === 0}
      <div class="no-results">
        <strong>일치하는 위치가 없습니다.</strong><span>검색어를 줄여 보세요.</span>
      </div>
    {:else}
      <div class="result-list">
        {#each resultPois as poi (poi.id)}
          {@const iconPath = poiIconPath(poi)}
          <button
            type="button"
            class:selected={selectedPoiId === poi.id}
            onclick={() => focusPoi(poi)}
          >
            <span
              class="result-icon"
              class:generic={iconPath === null}
              style={`--kind-color:${kindColor(poi.kind)}`}
            >
              {#if iconPath}
                <img src={iconPath} alt="" width="34" height="34" loading="lazy" />
              {:else}
                <i aria-hidden="true"></i>
              {/if}
            </span>
            <span><strong>{poi.display_name}</strong><small>{mapAreaLabel(poi)}</small></span>
            <em>{terms.get(poi.kind)?.label_ko ?? poi.kind}</em>
          </button>
        {/each}
      </div>
    {/if}
  </section>
{/snippet}

{#snippet filterBody()}
  <label class="map-search">
    <span>위치 검색</span>
    <input type="search" bind:value={query} placeholder="위치 이름 또는 분류" autocomplete="off" />
    {#if query.length > 0}
      <button type="button" aria-label="지도 검색어 지우기" onclick={() => (query = '')}
        >지우기</button
      >
    {/if}
  </label>

  {#if query.trim().length > 0}
    {@render mapResults()}
  {/if}

  <section class="filter-section" class:searching={query.trim().length > 0}>
    <header>
      <h2>표시 항목</h2>
      <small>{activeRegionFilterCount}/{filterOptions.length}종</small>
    </header>
    <div class="purpose-grid" aria-label="지도 목적별 필터">
      <button
        type="button"
        class:active={purposeSelected('location')}
        aria-pressed={purposeSelected('location')}
        onclick={() => applyPurpose('location')}>탐험</button
      >
      <button
        type="button"
        class:active={purposeSelected('resource')}
        aria-pressed={purposeSelected('resource')}
        onclick={() => applyPurpose('resource')}>재료</button
      >
      <button
        type="button"
        class:active={purposeSelected('enemy')}
        aria-pressed={purposeSelected('enemy')}
        onclick={() => applyPurpose('enemy')}>보스</button
      >
      <button
        type="button"
        class:active={purposeSelected('collectible')}
        aria-pressed={purposeSelected('collectible')}
        onclick={() => applyPurpose('collectible')}>수집</button
      >
    </div>
    <div class="filter-actions">
      <button type="button" onclick={() => replaceEnabledFilters(filterOptions.map(({ id }) => id))}
        >모두 표시</button
      >
      <button type="button" onclick={() => replaceEnabledFilters([])}>모두 숨기기</button>
    </div>
    <div class="filter-groups">
      {#each filterGroups as group (group.id)}
        <details open>
          <summary><strong>{group.label_ko}</strong><small>{group.options.length}종</small></summary
          >
          <div class="kind-grid">
            {#each group.options as option (option.id)}
              <button
                type="button"
                class:active={enabledFilterIds.has(option.id)}
                aria-pressed={enabledFilterIds.has(option.id)}
                onclick={() => toggleFilter(option.id)}
              >
                <span style={`--kind-color:${kindColor(option.id)}`} aria-hidden="true"></span>
                <strong>{option.label}</strong><small>{option.count.toLocaleString('ko-KR')}</small>
              </button>
            {/each}
          </div>
        </details>
      {/each}
    </div>
  </section>

  {#if query.trim().length === 0}
    {@render mapResults()}
  {/if}
{/snippet}

<section class="map-screen">
  <header class="map-head">
    <h1>탐험 지도</h1>
  </header>

  <div class="region-tabs" role="tablist" aria-label="지도 지역">
    {#each mapRegions as option (option.id)}
      <button
        type="button"
        role="tab"
        aria-selected={region.id === option.id}
        class:active={region.id === option.id}
        onclick={() => changeRegion(option.id)}
      >
        <span aria-hidden="true"></span>{option.label}
      </button>
    {/each}
  </div>

  <div class="map-layout" class:with-inspector={selectedPoi !== null}>
    {#if mobileSheet}
      <Dialog.Root bind:open={filtersOpen}>
        <Dialog.Overlay class="filter-overlay" />
        <Dialog.Content class="mobile-dialog-root" onCloseAutoFocus={dialogCloseAutoFocus}>
          <div id="map-filter-panel" class="control-panel mobile-dialog open">
            <header class="mobile-filter-head">
              <Dialog.Title class="mobile-filter-title">지도 필터</Dialog.Title>
              <Dialog.Close>닫기</Dialog.Close>
            </header>
            {@render filterBody()}
          </div>
        </Dialog.Content>
      </Dialog.Root>
    {:else}
      <aside
        id="map-filter-panel"
        class="control-panel"
        class:open={filtersOpen}
        aria-label="지도 검색과 필터"
      >
        <header class="mobile-filter-head">
          <strong>지도 필터</strong>
          <button bind:this={filterCloseButton} type="button" onclick={() => void closeFilters()}
            >닫기</button
          >
        </header>
        {@render filterBody()}
      </aside>
    {/if}

    <div class="map-stage">
      <button
        bind:this={filterTriggerButton}
        type="button"
        class="filter-trigger"
        aria-expanded={filtersOpen}
        aria-controls="map-filter-panel"
        onclick={() => void openFilters()}>필터 {activeRegionFilterCount}</button
      >
      <div class="map-tools" aria-label="지도 배율">
        <div class="map-tool-item">
          <button
            type="button"
            aria-label="지도 축소"
            aria-describedby="map-zoom-out-tip"
            disabled={relativeZoom <= MIN_RELATIVE_ZOOM + 0.001}
            onclick={() => setZoom(relativeZoom / ZOOM_FACTOR)}>−</button
          >
          <span id="map-zoom-out-tip" class="map-tooltip" role="tooltip">지도 축소</span>
        </div>
        <span aria-live="polite">{Math.round(relativeZoom * 100)}%</span>
        <div class="map-tool-item">
          <button
            type="button"
            aria-label="지도 확대"
            aria-describedby="map-zoom-in-tip"
            disabled={relativeZoom >= MAX_RELATIVE_ZOOM - 0.001}
            onclick={() => setZoom(relativeZoom * ZOOM_FACTOR)}>＋</button
          >
          <span id="map-zoom-in-tip" class="map-tooltip" role="tooltip">지도 확대</span>
        </div>
        <button type="button" onclick={resetView}>전체</button>
        {#if coarsePointer}
          <button
            type="button"
            class:active={touchInteractionEnabled}
            aria-pressed={touchInteractionEnabled}
            onclick={() => setTouchInteraction(!touchInteractionEnabled)}
            >{touchInteractionEnabled ? '조작 종료' : '지도 조작'}</button
          >
        {/if}
      </div>

      <svg
        bind:this={mapSvg}
        viewBox="0 0 2048 2048"
        role="region"
        aria-label={`${region.label} 지도`}
      >
        <g bind:this={mapViewport}>
          <rect width="2048" height="2048" fill="#07131c" />
          {#each tiles as tile (tile.relative_path)}
            <image
              href={tilePublicPath(region, tile)}
              x={tile.map_rect.min_x}
              y={tile.map_rect.min_y}
              width={tile.map_rect.max_x - tile.map_rect.min_x}
              height={tile.map_rect.max_y - tile.map_rect.min_y}
              preserveAspectRatio="none"
            />
          {/each}

          {#each visiblePois as poi (poi.id)}
            {@const iconPath = poiIconPath(poi)}
            <g
              data-poi-marker
              data-poi-id={poi.id}
              class="panzoom-exclude"
              role="button"
              tabindex={markerTabStopId === poi.id ? 0 : -1}
              aria-label={`${poi.display_name}, ${mapAreaLabel(poi)}, ${terms.get(poi.kind)?.label_ko ?? poi.kind}`}
              class:selected={selectedPoiId === poi.id}
              transform={['translate(', poi.map_x, ' ', poi.map_y, ')'].join('')}
              onclick={() => focusPoi(poi)}
              onfocus={() => (activeMarkerId = poi.id)}
              onkeydown={(event) => markerKeydown(event, poi)}
            >
              <title
                >{poi.display_name} · {mapAreaLabel(poi)} · {terms.get(poi.kind)?.label_ko ??
                  poi.kind}</title
              >
              <circle
                r={markerSize * 0.58}
                fill="#091018"
                stroke={selectedPoiId === poi.id ? '#f0c75e' : kindColor(poi.kind)}
                stroke-width={markerSize * 0.09}
              />
              {#if iconPath}
                <image
                  href={iconPath}
                  x={-markerSize / 2}
                  y={-markerSize / 2}
                  width={markerSize}
                  height={markerSize}
                  preserveAspectRatio="xMidYMid meet"
                />
              {:else}
                <rect
                  x={-markerSize * 0.2}
                  y={-markerSize * 0.2}
                  width={markerSize * 0.4}
                  height={markerSize * 0.4}
                  rx={markerSize * 0.06}
                  fill={kindColor(poi.kind)}
                  transform="rotate(45)"
                />
              {/if}
            </g>
          {/each}
        </g>
      </svg>
      <p class="map-summary" aria-live="polite">
        지도에 {visiblePois.length.toLocaleString('ko-KR')}개 표시 중{#if visiblePois.length < filteredPois.length}{' · 확대하면 더 표시'}{/if}
      </p>
    </div>

    {#if selectedPoi}
      {@const selectedIconPath = poiIconPath(selectedPoi)}
      <aside
        bind:this={selectedInspector}
        class="inspector"
        tabindex="-1"
        aria-label="선택한 지도 위치 상세"
      >
        <header>
          <span
            class="detail-icon"
            class:generic={selectedIconPath === null}
            style={`--kind-color:${kindColor(selectedPoi.kind)}`}
          >
            {#if selectedIconPath}
              <img src={selectedIconPath} alt="" width="62" height="62" />
            {:else}
              <i aria-hidden="true"></i>
            {/if}
          </span>
          <div>
            <small>{terms.get(selectedPoi.kind)?.label_ko ?? selectedPoi.kind}</small>
            <h2>{selectedPoi.display_name}</h2>
          </div>
          <button
            type="button"
            aria-label="지도 위치 상세 닫기"
            onclick={() => (selectedPoiId = null)}>닫기</button
          >
        </header>
        <p>{terms.get(selectedPoi.kind)?.description_ko ?? '설명 데이터가 없습니다.'}</p>
        <dl>
          <div>
            <dt>지역</dt>
            <dd>{region.label}</dd>
          </div>
          <div>
            <dt>지도 위치</dt>
            <dd>{mapAreaLabel(selectedPoi)}</dd>
          </div>
          <div>
            <dt>분류</dt>
            <dd>{terms.get(selectedPoi.kind)?.label_ko ?? selectedPoi.kind}</dd>
          </div>
        </dl>
      </aside>
    {/if}
  </div>
</section>

<style>
  .map-screen {
    padding: 18px clamp(12px, 1.8vw, 28px) 32px;
  }
  .map-head {
    padding: 14px 18px;
    border: 1px solid var(--border);
    background: var(--ink);
  }
  .map-head h1 {
    margin: 0;
    font-size: 1.4rem;
    letter-spacing: -0.025em;
  }
  dt {
    color: var(--muted);
    font-size: 0.75rem;
  }
  dd {
    margin: 4px 0 0;
    font-size: 0.75rem;
    font-weight: 800;
  }
  .region-tabs {
    display: flex;
    border: 1px solid var(--border);
    border-top: 0;
    background: var(--sidebar);
  }
  .region-tabs button {
    position: relative;
    min-height: 44px;
    padding: 0 18px;
    border: 0;
    border-right: 1px solid var(--border);
    color: var(--muted-strong);
    background: transparent;
    cursor: pointer;
    font-size: 0.75rem;
    font-weight: 760;
  }
  .region-tabs button span {
    position: absolute;
    right: 18px;
    bottom: 0;
    left: 18px;
    height: 3px;
  }
  .region-tabs button.active {
    color: var(--text);
    background: rgb(88 205 210 / 7%);
  }
  .region-tabs button.active span {
    background: var(--tech-normal);
    box-shadow:
      5px 0 var(--tech-normal),
      -5px 0 var(--tech-normal);
  }
  .map-layout {
    display: grid;
    grid-template-columns: 300px minmax(460px, 1fr);
    gap: 10px;
    margin-top: 10px;
  }
  .map-layout.with-inspector {
    grid-template-columns: 300px minmax(460px, 1fr) 300px;
  }
  .control-panel,
  .inspector {
    min-width: 0;
    border: 1px solid var(--border);
    background: var(--ink);
  }
  .control-panel {
    max-height: calc(100vh - 148px);
    overflow: auto;
  }
  :global(.filter-overlay) {
    position: fixed;
    z-index: 79;
    inset: 0;
    background: rgb(3 7 12 / 66%);
  }
  :global(.mobile-dialog-root) {
    position: fixed;
    z-index: 80;
    inset: 0;
    pointer-events: none;
  }
  :global(.mobile-dialog-root .control-panel) {
    pointer-events: auto;
  }
  .mobile-filter-head,
  .filter-trigger {
    display: none;
  }
  .map-search {
    position: relative;
    display: grid;
    gap: 7px;
    padding: 14px;
    border-bottom: 1px solid var(--border);
    color: var(--text-soft);
    font-size: 0.75rem;
    font-weight: 760;
  }
  .map-search input {
    min-width: 0;
    min-height: 42px;
    padding: 0 11px;
    border: 1px solid var(--border-strong);
    border-radius: 4px;
    background: var(--void);
    font-size: 0.76rem;
  }
  .map-search input {
    padding-right: 54px;
  }
  .map-search button {
    position: absolute;
    right: 20px;
    bottom: 20px;
    min-height: 30px;
    border: 0;
    color: var(--muted-strong);
    background: var(--surface-raised);
    cursor: pointer;
    font-size: 0.75rem;
  }
  .filter-section {
    padding: 13px 14px;
    border-bottom: 1px solid var(--border);
  }
  .filter-section.searching {
    display: none;
  }
  .filter-section > header,
  .results > header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 9px;
  }
  h2 {
    margin: 0;
    font-size: 0.78rem;
  }
  .filter-section header small,
  .results header small {
    color: var(--muted);
    font-size: 0.75rem;
  }
  .purpose-grid {
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    gap: 4px;
  }
  .purpose-grid button,
  .filter-actions button {
    min-height: 36px;
    border: 1px solid var(--border);
    border-radius: 4px;
    color: var(--muted-strong);
    background: var(--void);
    cursor: pointer;
    font-size: 0.75rem;
    font-weight: 760;
  }
  .purpose-grid button.active {
    border-color: var(--tech-normal);
    color: var(--text);
    background: color-mix(in srgb, var(--tech-normal) 11%, var(--void));
    box-shadow: inset 0 -3px var(--tech-normal);
  }
  .filter-actions {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 4px;
    margin-top: 6px;
  }
  .filter-actions button {
    min-height: 32px;
    color: var(--muted);
    font-size: 0.7rem;
  }
  .filter-groups {
    display: grid;
    gap: 6px;
    margin-top: 10px;
  }
  .filter-groups details {
    border-top: 1px solid var(--border);
  }
  .filter-groups summary {
    display: flex;
    min-height: 36px;
    align-items: center;
    justify-content: space-between;
    color: var(--text-soft);
    cursor: pointer;
    list-style: none;
  }
  .filter-groups summary::-webkit-details-marker {
    display: none;
  }
  .filter-groups summary::before {
    width: 12px;
    content: '›';
    color: var(--muted);
    transform: rotate(0deg);
    transition: transform var(--motion-fast) var(--ease-standard);
  }
  .filter-groups details[open] summary::before {
    transform: rotate(90deg);
  }
  .filter-groups summary strong {
    margin-right: auto;
    font-size: 0.75rem;
  }
  .filter-groups summary small {
    color: var(--muted);
    font-size: 0.7rem;
  }
  .kind-grid {
    display: grid;
    grid-template-columns: 1fr;
    gap: 3px;
    padding-bottom: 5px;
  }
  .kind-grid button {
    display: grid;
    min-height: 40px;
    grid-template-columns: 10px minmax(0, 1fr) auto;
    gap: 7px;
    align-items: center;
    padding: 8px;
    border: 1px solid var(--border);
    border-radius: 4px;
    color: var(--muted-strong);
    background: var(--void);
    cursor: pointer;
    text-align: left;
  }
  .kind-grid button > span {
    width: 7px;
    height: 20px;
    background: var(--kind-color);
    opacity: 0.35;
  }
  .kind-grid button strong {
    overflow: hidden;
    font-size: 0.75rem;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .kind-grid button small {
    color: var(--muted);
    font-size: 0.75rem;
  }
  .kind-grid button.active {
    border-color: color-mix(in srgb, var(--kind-color), transparent 25%);
    color: var(--text);
    background: color-mix(in srgb, var(--kind-color) 8%, var(--void));
  }
  .kind-grid button.active > span {
    opacity: 1;
    box-shadow: 3px 0 color-mix(in srgb, var(--kind-color), transparent 65%);
  }
  .results {
    padding: 13px 14px;
  }
  .result-list {
    display: grid;
    gap: 3px;
  }
  .result-list button {
    display: grid;
    min-width: 0;
    min-height: 52px;
    grid-template-columns: 38px minmax(0, 1fr) auto;
    gap: 8px;
    align-items: center;
    padding: 6px 8px;
    border: 1px solid transparent;
    border-radius: 4px;
    color: inherit;
    background: transparent;
    cursor: pointer;
    text-align: left;
    transition:
      transform var(--motion-fast) var(--ease-standard),
      border-color var(--motion-base) var(--ease-standard),
      background-color var(--motion-base) var(--ease-standard),
      box-shadow var(--motion-base) var(--ease-standard);
  }
  .result-list button:hover,
  .result-list button.selected {
    border-color: var(--border);
    background: var(--surface);
    transform: translateX(2px);
  }
  .result-list button.selected {
    box-shadow: inset 3px 0 var(--tech-normal);
  }
  .result-icon,
  .detail-icon {
    display: grid;
    place-items: center;
    overflow: hidden;
    border: 1px solid var(--kind-color);
    border-radius: 50%;
    background: var(--void);
  }
  .result-icon {
    width: 36px;
    height: 36px;
  }
  .result-icon img,
  .detail-icon img {
    width: 88%;
    height: 88%;
    object-fit: contain;
  }
  .result-icon.generic i,
  .detail-icon.generic i {
    width: 34%;
    aspect-ratio: 1;
    border-radius: 2px;
    background: var(--kind-color);
    transform: rotate(45deg);
  }
  .result-list button > span:nth-child(2) {
    display: grid;
    min-width: 0;
  }
  .result-list strong {
    overflow: hidden;
    font-size: 0.75rem;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .result-list button > span:nth-child(2) small {
    margin-top: 2px;
    color: var(--muted);
    font-size: 0.68rem;
  }
  .result-list em {
    color: var(--muted-strong);
    font-size: 0.75rem;
    font-style: normal;
  }
  .no-results {
    display: grid;
    min-height: 110px;
    place-items: center;
    align-content: center;
    text-align: center;
  }
  .no-results strong {
    font-size: 0.75rem;
  }
  .no-results span {
    margin-top: 5px;
    color: var(--muted);
    font-size: 0.75rem;
  }
  .map-stage {
    --map-height: clamp(480px, calc(100dvh - 210px), 820px);
    position: relative;
    min-width: 0;
    min-height: var(--map-height);
    border: 1px solid var(--border-strong);
    background: #07131c;
    box-shadow: var(--shadow-panel);
  }
  svg {
    display: block;
    width: 100%;
    height: var(--map-height);
    min-height: var(--map-height);
    cursor: grab;
    touch-action: none;
    user-select: none;
  }
  svg :global(g[role='button']) {
    cursor: pointer;
    outline: none;
  }
  svg :global(g[role='button']:focus-visible circle) {
    stroke: var(--focus);
    stroke-width: 10;
  }
  .map-tools {
    position: absolute;
    z-index: 2;
    top: 12px;
    right: 12px;
    display: flex;
    align-items: center;
    overflow: hidden;
    border: 1px solid var(--border-strong);
    border-radius: 5px;
    background: rgb(8 12 18 / 92%);
  }
  .map-tool-item {
    position: relative;
    display: grid;
  }
  :global(.map-tools button),
  .map-tools > span {
    display: grid;
    min-width: 44px;
    min-height: 44px;
    place-items: center;
    border: 0;
    border-right: 1px solid var(--border);
    background: transparent;
    font-size: 0.75rem;
  }
  :global(.map-tools button) {
    cursor: pointer;
  }
  :global(.map-tools button:disabled) {
    color: var(--muted);
    cursor: not-allowed;
    opacity: 0.55;
  }
  :global(.map-tools button:not(:disabled):hover),
  :global(.map-tools button:not(:disabled):focus-visible) {
    color: var(--text);
    background: var(--surface-raised);
  }
  :global(.map-tools button.active) {
    color: var(--text);
    background: color-mix(in srgb, var(--accent), var(--surface-raised) 78%);
  }
  .map-tools > span {
    color: var(--muted-strong);
  }
  .map-tooltip {
    position: absolute;
    z-index: 120;
    top: calc(100% + 8px);
    left: 50%;
    padding: 7px 9px;
    border: 1px solid var(--border-strong);
    border-radius: 4px;
    color: var(--text);
    background: var(--surface-raised);
    box-shadow: var(--shadow-panel);
    font-size: 0.75rem;
    font-weight: 700;
    white-space: nowrap;
    opacity: 0;
    pointer-events: none;
    transform: translate(-50%, -4px);
    transition:
      opacity var(--motion-fast) var(--ease-standard),
      transform var(--motion-fast) var(--ease-standard);
  }
  .map-tool-item button:hover + .map-tooltip,
  .map-tool-item button:focus-visible + .map-tooltip {
    opacity: 1;
    transform: translate(-50%, 0);
  }
  .map-summary {
    position: absolute;
    right: 12px;
    bottom: 12px;
    margin: 0;
    padding: 8px 10px;
    border: 1px solid var(--border);
    background: rgb(8 12 18 / 88%);
    color: var(--muted);
    font-size: 0.75rem;
  }
  .inspector {
    align-self: start;
    min-height: 260px;
  }
  .inspector > header,
  .inspector > p,
  .inspector > dl {
    animation: inspector-content-enter var(--motion-base) var(--ease-emphasized) both;
  }
  .inspector > p {
    animation-delay: 25ms;
  }
  .inspector > dl {
    animation-delay: 50ms;
  }
  @keyframes inspector-content-enter {
    from {
      opacity: 0;
      transform: translateY(4px);
    }
    to {
      opacity: 1;
      transform: translateY(0);
    }
  }
  .inspector > header {
    display: grid;
    grid-template-columns: 64px minmax(0, 1fr) auto;
    gap: 10px;
    align-items: center;
    padding: 14px;
    border-bottom: 1px solid var(--border);
  }
  .detail-icon {
    width: 64px;
    height: 64px;
    border-radius: 6px;
  }
  .inspector header small {
    color: var(--tech-normal);
    font-size: 0.75rem;
    font-weight: 800;
  }
  .inspector header h2 {
    margin-top: 5px;
    font-size: 0.9rem;
  }
  .inspector header button {
    min-width: 40px;
    min-height: 38px;
    border: 1px solid var(--border);
    border-radius: 4px;
    background: var(--surface-raised);
    cursor: pointer;
    font-size: 0.75rem;
  }
  .inspector > p {
    margin: 12px 14px 16px;
    color: var(--text-soft);
    font-size: 0.75rem;
    line-height: 1.65;
  }
  .inspector dl {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    margin: 0;
    border-block: 1px solid var(--border);
  }
  .inspector dl div {
    padding: 10px 12px;
    border-right: 1px solid var(--border);
    border-bottom: 1px solid var(--border);
  }
  @media (max-width: 1240px) {
    .map-layout,
    .map-layout.with-inspector {
      grid-template-columns: 280px minmax(420px, 1fr);
    }
    .inspector {
      grid-column: 1 / -1;
      display: grid;
      grid-template-columns: minmax(280px, 1fr) minmax(340px, 1fr);
    }
    .inspector > header {
      grid-row: span 3;
    }
  }
  @media (max-width: 820px) {
    :global(.route-stage:has(#map-filter-panel.open)) {
      position: relative;
      z-index: 41;
      animation: none;
    }

    .map-layout,
    .map-layout.with-inspector {
      grid-template-columns: 1fr;
    }
    .control-panel {
      position: fixed;
      z-index: 80;
      right: 12px;
      bottom: 12px;
      left: 12px;
      max-height: min(72vh, 640px);
      overflow: auto;
      visibility: hidden;
      opacity: 0;
      pointer-events: none;
      transform: translateY(calc(100% + 24px));
      transition:
        transform var(--motion-base) var(--ease-emphasized),
        opacity var(--motion-fast) var(--ease-standard);
      box-shadow: 0 22px 70px rgb(0 0 0 / 72%);
    }
    .control-panel.open {
      visibility: visible;
      opacity: 1;
      pointer-events: auto;
      transform: translateY(0);
    }
    .mobile-filter-head {
      position: sticky;
      z-index: 2;
      top: 0;
      display: flex;
      min-height: 48px;
      align-items: center;
      justify-content: space-between;
      padding: 8px 14px;
      border-bottom: 1px solid var(--border-strong);
      background: var(--surface-raised);
    }
    .mobile-filter-head strong,
    :global(.mobile-filter-title) {
      font-size: 0.82rem;
      font-weight: 700;
    }
    .mobile-filter-head button {
      min-width: 52px;
      min-height: 44px;
      border: 1px solid var(--border);
      border-radius: 4px;
      background: var(--ink);
      cursor: pointer;
      font-size: 0.75rem;
    }
    .filter-trigger {
      position: absolute;
      z-index: 3;
      top: 12px;
      left: 12px;
      display: block;
      min-width: 74px;
      min-height: 44px;
      border: 1px solid var(--tech-normal);
      border-radius: 5px;
      color: var(--text);
      background: rgb(8 12 18 / 92%);
      cursor: pointer;
      font-size: 0.75rem;
      font-weight: 800;
    }
    .kind-grid {
      grid-template-columns: repeat(2, minmax(0, 1fr));
    }
    .results {
      max-height: 280px;
      overflow: auto;
    }
    .map-stage {
      grid-row: 1;
      --map-height: clamp(440px, calc(100dvh - 270px), 700px);
    }
    .inspector {
      position: static;
      grid-row: 2;
      display: block;
      border-color: var(--border-strong);
      background: var(--surface-raised);
      box-shadow: 0 20px 60px rgb(0 0 0 / 55%);
    }
  }
  @media (min-width: 720px) and (max-width: 820px) {
    :global(.route-stage:has(#map-filter-panel.open)) {
      z-index: auto;
    }
    .map-layout,
    .map-layout.with-inspector {
      grid-template-columns: minmax(0, 1fr);
    }
    .map-layout:has(.control-panel.open),
    .map-layout.with-inspector:has(.control-panel.open) {
      grid-template-columns: 280px minmax(0, 1fr);
    }
    .control-panel,
    .control-panel.open {
      position: static;
      display: none;
      max-height: 520px;
      visibility: visible;
      opacity: 1;
      pointer-events: auto;
      transform: none;
      transition: none;
      box-shadow: none;
    }
    .control-panel.open {
      display: block;
    }
    .map-layout:has(.control-panel.open) .map-stage {
      grid-column: 2;
    }
    .map-stage {
      grid-column: 1;
      --map-height: clamp(560px, calc(100dvh - 250px), 760px);
    }
    .inspector {
      grid-column: 1 / -1;
      grid-row: 2;
    }
  }
  @media (max-width: 560px) {
    .map-screen {
      padding: 10px 8px 82px;
    }
    .map-head {
      padding: 18px 14px;
    }
    .region-tabs button {
      flex: 1;
    }
    .kind-grid {
      grid-template-columns: 1fr;
    }
    .map-stage {
      --map-height: clamp(420px, calc(100dvh - 270px), 620px);
    }
    .map-summary {
      right: 8px;
      bottom: 8px;
      left: 8px;
    }
    .control-panel {
      right: 8px;
      bottom: calc(72px + env(safe-area-inset-bottom));
      left: 8px;
      max-height: calc(100dvh - 136px - env(safe-area-inset-bottom));
    }
  }
</style>
