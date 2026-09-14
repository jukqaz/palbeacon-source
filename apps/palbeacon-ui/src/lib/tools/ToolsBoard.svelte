<script lang="ts">
  import { resolve } from '$app/paths';
  import { get } from 'svelte/store';
  import { untrack } from 'svelte';
  import { BreedingCalculator, materialTotals, rankTravel, rankWork, recommendTeam } from './tools';
  import type {
    BreedingDirection,
    TeamGoal,
    ToolBuilding,
    ToolMode,
    ToolPal,
    ToolsCatalog,
  } from './types';
  import { formatUserNumber } from '$lib/shared/format/user-number';
  import { replaceCurrentQueryParameter } from '$lib/shared/navigation/query-state';
  import { normalizeSearchText } from '$lib/shared/search/search-core';
  import { ownedSpeciesCount, personalPalData } from '$lib/personal/personal-data';
  import {
    planningWorkspace,
    updatePlanningWorkspace,
    type PlanningWorkspaceState,
  } from './plan-workspace';
  import PlanTransfer from './PlanTransfer.svelte';

  interface Props {
    catalog: ToolsCatalog;
    initialMode?: ToolMode;
    initialBuildingId?: string;
    initialTargetId?: string;
    initialOwnedOnly?: boolean;
  }
  let {
    catalog,
    initialMode = 'team',
    initialBuildingId = '',
    initialTargetId = '',
    initialOwnedOnly = false,
  }: Props = $props();
  const savedPlan = untrack(() => get(planningWorkspace));
  let mode = $state<ToolMode>(untrack(() => initialMode));
  let goal = $state<TeamGoal>(savedPlan.team_goal);
  let ownedOnly = $state(untrack(() => initialOwnedOnly || savedPlan.owned_only));
  const planningPals = $derived(
    ownedOnly && $personalPalData.ready
      ? catalog.pals.filter((pal) => ownedSpeciesCount($personalPalData, pal.id) > 0)
      : catalog.pals,
  );
  const workOptions = $derived(Object.entries(catalog.work_names_ko));
  let workType = $state(
    untrack(() =>
      Object.hasOwn(catalog.work_names_ko, savedPlan.work_type) ? savedPlan.work_type : 'Handcraft',
    ),
  );
  const requestedBuilding = untrack(() =>
    catalog.buildings.find((entry) => entry.id === initialBuildingId.trim()),
  );
  let buildingId = $state(
    untrack(() =>
      requestedBuilding
        ? requestedBuilding.id
        : catalog.buildings.some((entry) => entry.id === savedPlan.building_id)
          ? savedPlan.building_id
          : (catalog.buildings[0]?.id ?? ''),
    ),
  );
  let buildingSearch = $state(
    untrack(() => catalog.buildings.find((entry) => entry.id === buildingId)?.name_ko ?? ''),
  );
  let buildingPickerOpen = $state(false);
  let activeBuildingIndex = $state(0);
  let buildingPickerShell = $state<HTMLDivElement | null>(null);
  let quantity = $state(savedPlan.building_quantity);
  const breeding = $derived(new BreedingCalculator(catalog));
  const breedingOptions = $derived(
    catalog.breeding.species
      .flatMap((species) => {
        const pal = breeding.palById(species.internal_id);
        return pal ? [pal] : [];
      })
      .toSorted(
        (left, right) =>
          (left.paldex_number ?? 9999) - (right.paldex_number ?? 9999) ||
          left.name_ko.localeCompare(right.name_ko, 'ko'),
      ),
  );
  const requestedTargetId = untrack(() =>
    catalog.breeding.species.some((pal) => pal.internal_id === initialTargetId.trim())
      ? initialTargetId.trim()
      : '',
  );
  let breedingDirection = $state<BreedingDirection>(
    requestedTargetId ? 'reverse' : savedPlan.breeding_direction,
  );
  let parentAId = $state(
    untrack(() =>
      breedingOptions.some((pal) => pal.id === savedPlan.parent_a_id)
        ? savedPlan.parent_a_id
        : (breedingOptions[0]?.id ?? ''),
    ),
  );
  let parentBId = $state(
    untrack(() =>
      breedingOptions.some((pal) => pal.id === savedPlan.parent_b_id)
        ? savedPlan.parent_b_id
        : (breedingOptions[1]?.id ?? breedingOptions[0]?.id ?? ''),
    ),
  );
  let targetId = $state(
    untrack(() =>
      requestedTargetId
        ? requestedTargetId
        : breedingOptions.some((pal) => pal.id === savedPlan.target_id)
          ? savedPlan.target_id
          : (breedingOptions[2]?.id ?? breedingOptions[0]?.id ?? ''),
    ),
  );
  let parentASearch = $state('');
  let parentBSearch = $state('');
  let targetSearch = $state('');
  let reverseLimit = $state(60);
  const matchingBreedingOptions = (value: string, selectedId: string) => {
    const normalized = normalizeSearchText(value);
    if (!normalized) return breedingOptions;
    const matches = breedingOptions.filter((pal) =>
      normalizeSearchText(
        `${pal.name_ko} ${pal.paldex_number === null ? '' : pal.paldex_number.toString()}`,
      ).includes(normalized),
    );
    const selected = breedingOptions.find((pal) => pal.id === selectedId);
    return selected && !matches.some((pal) => pal.id === selected.id)
      ? [selected, ...matches]
      : matches;
  };
  const parentAOptions = $derived(matchingBreedingOptions(parentASearch, parentAId));
  const parentBOptions = $derived(matchingBreedingOptions(parentBSearch, parentBId));
  const targetOptions = $derived(matchingBreedingOptions(targetSearch, targetId));
  const forwardOutcomes = $derived(breeding.forward(parentAId, parentBId));
  const reverseCombinations = $derived(breeding.reverse(targetId));
  const breedingPath = $derived(breeding.shortestPath([parentAId, parentBId], targetId));
  const recommendations = $derived(recommendTeam(planningPals, goal));
  const workRanking = $derived(rankWork(planningPals, workType));
  const travelRanking = $derived(rankTravel(planningPals));
  const selectedParentsAvailable = $derived.by(() => {
    if (!$personalPalData.ready) return null;
    const firstCount = ownedSpeciesCount($personalPalData, parentAId);
    const secondCount = ownedSpeciesCount($personalPalData, parentBId);
    return parentAId === parentBId ? firstCount >= 2 : firstCount >= 1 && secondCount >= 1;
  });
  const building = $derived(catalog.buildings.find((entry) => entry.id === buildingId) ?? null);
  const materials = $derived(materialTotals(building, quantity));
  const buildingMaterialSummary = (entry: ToolBuilding): string => {
    const visible = entry.materials
      .slice(0, 2)
      .map((material) => `${material.name_ko} ${material.quantity.toLocaleString('ko-KR')}`);
    const remaining = entry.materials.length - visible.length;
    return `${visible.join(' · ')}${remaining > 0 ? ` 외 ${remaining.toString()}종` : ''}`;
  };
  const entityHref = (base: string, name: string, id: string): string => {
    const parameters = new URLSearchParams({ q: name, id });
    return `${base}?${parameters.toString()}`;
  };
  const palHref = (pal: ToolPal): string => entityHref(resolve('/pals/', {}), pal.name_ko, pal.id);
  const buildingHref = (entry: ToolBuilding): string =>
    entityHref(resolve('/buildings/', {}), entry.name_ko, entry.id);
  const itemHref = (name: string, id: string): string =>
    entityHref(resolve('/items/', {}), name, id);
  const matchingBuildings = $derived.by(() => {
    const normalized = normalizeSearchText(buildingSearch);
    return catalog.buildings
      .filter((entry) =>
        normalizeSearchText(
          [entry.name_ko, ...entry.materials.map((material) => material.name_ko)].join(' '),
        ).includes(normalized),
      )
      .toSorted((left, right) => left.name_ko.localeCompare(right.name_ko, 'ko'))
      .slice(0, 30);
  });
  const chooseBuilding = (entry: ToolBuilding) => {
    buildingId = entry.id;
    buildingSearch = entry.name_ko;
    buildingPickerOpen = false;
    activeBuildingIndex = 0;
  };
  const updateBuildingSearch = (value: string) => {
    buildingSearch = value;
    activeBuildingIndex = 0;
    const normalized = normalizeSearchText(value);
    const exact = catalog.buildings.filter(
      (entry) => normalizeSearchText(entry.name_ko) === normalized,
    );
    if (exact.length === 1 && exact[0]) {
      buildingId = exact[0].id;
      buildingPickerOpen = false;
      return;
    }
    buildingId = '';
    buildingPickerOpen = true;
  };
  const buildingPickerKeydown = (event: KeyboardEvent) => {
    if (event.key === 'Escape') {
      buildingPickerOpen = false;
      return;
    }
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      buildingPickerOpen = true;
      const offset = event.key === 'ArrowDown' ? 1 : -1;
      activeBuildingIndex = Math.max(
        0,
        Math.min(matchingBuildings.length - 1, activeBuildingIndex + offset),
      );
      return;
    }
    if (event.key !== 'Enter' || !buildingPickerOpen) return;
    const next = matchingBuildings[activeBuildingIndex];
    if (!next) return;
    event.preventDefault();
    chooseBuilding(next);
  };
  const closeBuildingPickerOnBlur = (event: FocusEvent) => {
    if (buildingPickerShell?.contains(event.relatedTarget as Node | null)) return;
    buildingPickerOpen = false;
  };

  const modePresentation: Record<ToolMode, { title: string }> = {
    breeding: {
      title: '교배 계산',
    },
    team: {
      title: '팀 구성',
    },
    work: {
      title: '팰 비교',
    },
    travel: {
      title: '팰 비교',
    },
    materials: {
      title: '재료 계산',
    },
  };
  const presentation = $derived(modePresentation[mode]);

  const goals: { id: TeamGoal; label: string }[] = [
    { id: 'balanced', label: '공격 우선' },
    { id: 'base', label: '작업 적성' },
    { id: 'night', label: '야간 거점' },
    { id: 'travel', label: '이동·탐험' },
  ];

  const selectComparisonMode = (next: 'work' | 'travel') => {
    mode = next;
    replaceCurrentQueryParameter('mode', next === 'travel' ? 'travel' : null);
  };

  const setOwnedOnly = (next: boolean) => {
    ownedOnly = next;
    replaceCurrentQueryParameter('mine', next ? '1' : null);
  };

  const applyImportedPlan = (state: PlanningWorkspaceState) => {
    goal = state.team_goal;
    breedingDirection = state.breeding_direction;
    parentAId = breedingOptions.some((pal) => pal.id === state.parent_a_id)
      ? state.parent_a_id
      : parentAId;
    parentBId = breedingOptions.some((pal) => pal.id === state.parent_b_id)
      ? state.parent_b_id
      : parentBId;
    targetId = breedingOptions.some((pal) => pal.id === state.target_id)
      ? state.target_id
      : targetId;
    workType = Object.hasOwn(catalog.work_names_ko, state.work_type) ? state.work_type : workType;
    ownedOnly = state.owned_only;
    const importedBuilding = catalog.buildings.find((entry) => entry.id === state.building_id);
    if (importedBuilding) {
      buildingId = importedBuilding.id;
      buildingSearch = importedBuilding.name_ko;
    }
    quantity = state.building_quantity;
  };

  $effect(() => {
    updatePlanningWorkspace({
      team_goal: goal,
      breeding_direction: breedingDirection,
      parent_a_id: parentAId,
      parent_b_id: parentBId,
      target_id: targetId,
      work_type: workType,
      owned_only: ownedOnly,
      building_id: buildingId,
      building_quantity: Math.max(1, Math.min(999, Math.trunc(Number(quantity)) || 1)),
    });
  });
</script>

{#snippet ownedFilter()}
  {#if $personalPalData.ready}
    <button
      type="button"
      class="owned-filter"
      class:active={ownedOnly}
      aria-pressed={ownedOnly}
      onclick={() => setOwnedOnly(!ownedOnly)}>내 팰만</button
    >
  {/if}
{/snippet}

{#snippet comparisonRow(pal: ToolPal, index: number, detail: string, value: string)}
  {@const owned = ownedSpeciesCount($personalPalData, pal.id)}
  <article>
    <span>{index + 1}</span>
    {#if pal.image_path}<img src={pal.image_path} alt="" width="48" height="48" />{/if}
    <div>
      <strong><a class="entity-link" href={palHref(pal)}>{pal.name_ko}</a></strong>
      <small>{owned > 0 ? `보유 ${owned.toLocaleString('ko-KR')}마리 · ` : ''}{detail}</small>
    </div>
    <b>{value}</b>
  </article>
{/snippet}

<section class="tools-screen pal-screen">
  <PlanTransfer onImported={applyImportedPlan} />
  {#key mode}
    <div class="tool-mode-stage" data-testid="tool-mode-stage">
      {#if mode === 'breeding'}
        <section class="tool-panel breeding-panel">
          <header>
            <div>
              <h1>{presentation.title}</h1>
            </div>
            <div class="pills" aria-label="교배 계산 방식">
              <button
                type="button"
                class:active={breedingDirection === 'forward'}
                aria-pressed={breedingDirection === 'forward'}
                onclick={() => (breedingDirection = 'forward')}>정방향</button
              >
              <button
                type="button"
                class:active={breedingDirection === 'reverse'}
                aria-pressed={breedingDirection === 'reverse'}
                onclick={() => (breedingDirection = 'reverse')}>역방향</button
              >
              <button
                type="button"
                class:active={breedingDirection === 'path'}
                aria-pressed={breedingDirection === 'path'}
                onclick={() => (breedingDirection = 'path')}>도달 경로</button
              >
            </div>
          </header>
          <div class="breeding-workspace">
            <form class="breeding-inputs" onsubmit={(event) => event.preventDefault()}>
              {#if breedingDirection !== 'reverse'}
                <label
                  >첫 번째 팰<input
                    bind:value={parentASearch}
                    type="search"
                    aria-label="첫 번째 부모 팰 검색"
                    placeholder="이름 또는 도감 번호"
                  /><select bind:value={parentAId} aria-label="첫 번째 부모 팰"
                    >{#each parentAOptions as pal (pal.id)}<option value={pal.id}
                        >{pal.name_ko}</option
                      >{/each}</select
                  ></label
                >
                <label
                  >두 번째 팰<input
                    bind:value={parentBSearch}
                    type="search"
                    aria-label="두 번째 부모 팰 검색"
                    placeholder="이름 또는 도감 번호"
                  /><select bind:value={parentBId} aria-label="두 번째 부모 팰"
                    >{#each parentBOptions as pal (pal.id)}<option value={pal.id}
                        >{pal.name_ko}</option
                      >{/each}</select
                  ></label
                >
              {/if}
              {#if breedingDirection !== 'forward'}
                <label
                  >목표 팰<input
                    bind:value={targetSearch}
                    type="search"
                    aria-label="목표 팰 검색"
                    placeholder="이름 또는 도감 번호"
                  /><select
                    bind:value={targetId}
                    aria-label="목표 팰"
                    onchange={() => (reverseLimit = 60)}
                    >{#each targetOptions as pal (pal.id)}<option value={pal.id}
                        >{pal.name_ko}</option
                      >{/each}</select
                  ></label
                >
              {/if}
            </form>

            <div class="breeding-results" aria-live="polite">
              {#if breedingDirection === 'forward'}
                <header>
                  <strong>예상 결과</strong><span
                    >{forwardOutcomes.length}개{selectedParentsAvailable === null
                      ? ''
                      : selectedParentsAvailable
                        ? ' · 교배 가능'
                        : ' · 부모 팰 필요'}</span
                  >
                </header>
                {#if forwardOutcomes.length > 0}
                  {#each forwardOutcomes as outcome (`${outcome.kind}:${outcome.child.id}:${outcome.ruleId ?? ''}`)}
                    <article class="breeding-result">
                      {#if outcome.child.image_path}<img
                          src={outcome.child.image_path}
                          alt=""
                          width="76"
                          height="76"
                        />{/if}
                      <div>
                        <span>{outcome.kind === 'special' ? '특수 교배' : '일반 교배'}</span>
                        <h3>
                          <a class="entity-link" href={palHref(outcome.child)}
                            >{outcome.child.name_ko}</a
                          >
                        </h3>
                      </div>
                    </article>
                  {/each}
                {:else}<p class="empty">확인 가능한 교배 결과가 없습니다.</p>{/if}
              {:else if breedingDirection === 'reverse'}
                <header>
                  <strong>부모 조합</strong><span
                    >{reverseCombinations.length.toLocaleString('ko-KR')}개</span
                  >
                </header>
                {#if reverseCombinations.length > 0}
                  <div class="combination-list">
                    {#each reverseCombinations.slice(0, reverseLimit) as combination, index (`${combination.parentA.id}:${combination.parentB.id}:${combination.outcome.kind}:${String(index)}`)}
                      <article>
                        <span>{String(index + 1).padStart(2, '0')}</span>
                        <div>
                          <strong
                            ><a class="entity-link" href={palHref(combination.parentA)}
                              >{combination.parentA.name_ko}</a
                            ></strong
                          >
                        </div>
                        <b>＋</b>
                        <div>
                          <strong
                            ><a class="entity-link" href={palHref(combination.parentB)}
                              >{combination.parentB.name_ko}</a
                            ></strong
                          >
                        </div>
                        <em>{combination.outcome.kind === 'special' ? '특수' : '일반'}</em>
                      </article>
                    {/each}
                  </div>
                  {#if reverseCombinations.length > reverseLimit}
                    <button class="load-more" type="button" onclick={() => (reverseLimit += 60)}>
                      다음 {Math.min(60, reverseCombinations.length - reverseLimit).toLocaleString(
                        'ko-KR',
                      )}개 조합 보기
                    </button>
                  {/if}
                {:else}<p class="empty">확인 가능한 부모 조합이 없습니다.</p>{/if}
              {:else}
                <header>
                  <strong>최단 도달 경로</strong><span
                    >{breedingPath?.reachable
                      ? `${String(breedingPath.generations)}세대`
                      : '도달 불가'}</span
                  >
                </header>
                {#if breedingPath?.reachable}
                  <div class="path-list">
                    {#each breedingPath.steps as step (`${String(step.generation)}:${step.pal.id}`)}
                      <article>
                        <span
                          >{step.generation === 0
                            ? $personalPalData.ready &&
                              ownedSpeciesCount($personalPalData, step.pal.id) > 0
                              ? '보유'
                              : '시작'
                            : `${String(step.generation)}세대`}</span
                        >
                        <div>
                          <strong
                            ><a class="entity-link" href={palHref(step.pal)}>{step.pal.name_ko}</a
                            ></strong
                          >
                        </div>
                        <p>
                          {step.parentA && step.parentB
                            ? `${step.parentA.name_ko} ＋ ${step.parentB.name_ko}`
                            : '출발 팰'}
                        </p>
                        {#if step.kind}<em>{step.kind === 'special' ? '특수' : '일반'}</em>{/if}
                      </article>
                    {/each}
                  </div>
                  <p class="result-limit">최대 6세대 안에서 찾은 가장 짧은 경로입니다.</p>
                {:else}<p class="empty">6세대 안에서 확인 가능한 교배 경로가 없습니다.</p>{/if}
              {/if}
            </div>
          </div>
        </section>
      {:else if mode === 'team'}
        <section class="tool-panel">
          <header>
            <div>
              <h1>{presentation.title}</h1>
            </div>
            <div class="header-actions">
              <div class="pills">
                {#each goals as item (item.id)}<button
                    type="button"
                    class:active={goal === item.id}
                    aria-pressed={goal === item.id}
                    onclick={() => (goal = item.id)}>{item.label}</button
                  >{/each}
              </div>
              {@render ownedFilter()}
            </div>
          </header>
          {#if recommendations.length > 0}<div class="recommendations">
              {#each recommendations as entry, index (entry.pal.id)}<article>
                  <span class="rank">{String(index + 1).padStart(2, '0')}</span
                  >{#if entry.pal.image_path}<img
                      src={entry.pal.image_path}
                      alt=""
                      width="72"
                      height="72"
                    />{/if}
                  <div>
                    {#if entry.pal.paldex_number}<small>도감 {entry.pal.paldex_number}</small>{/if}
                    <h3>
                      <a class="entity-link" href={palHref(entry.pal)}>{entry.pal.name_ko}</a>
                    </h3>
                    {#if ownedSpeciesCount($personalPalData, entry.pal.id) > 0}<small
                        class="owned-value"
                        >보유 {ownedSpeciesCount($personalPalData, entry.pal.id).toLocaleString(
                          'ko-KR',
                        )}마리</small
                      >{/if}
                    <p>{entry.reason}</p>
                  </div>
                </article>{/each}
            </div>{:else}<p class="empty">선택한 조건에 맞는 내 팰이 없습니다.</p>{/if}
        </section>
      {:else if mode === 'work'}
        <section class="tool-panel">
          <header>
            <div>
              <h1>{presentation.title}</h1>
            </div>
            <div class="compare-controls">
              <div class="pills" aria-label="비교 기준">
                <button
                  type="button"
                  class="active"
                  aria-pressed="true"
                  onclick={() => selectComparisonMode('work')}>작업</button
                >
                <button
                  type="button"
                  aria-pressed="false"
                  onclick={() => selectComparisonMode('travel')}>이동</button
                >
              </div>
              <label
                >작업 종류<select bind:value={workType}
                  >{#each workOptions as option (option[0])}<option value={option[0]}
                      >{option[1]}</option
                    >{/each}</select
                ></label
              >
              {@render ownedFilter()}
            </div>
          </header>
          <div class="ranking">
            {#each workRanking as pal, index (pal.id)}{@const work = pal.work_suitability.find(
                (entry) => entry.id === workType,
              )}
              {@render comparisonRow(
                pal,
                index,
                `식사량 ${pal.food_amount.toString()}단계`,
                `${catalog.work_names_ko[workType] ?? '작업'} Lv.${(work?.level ?? 0).toString()}`,
              )}{/each}
          </div>
        </section>
      {:else if mode === 'travel'}
        <section class="tool-panel">
          <header>
            <div>
              <h1>{presentation.title}</h1>
            </div>
            <div class="compare-controls">
              <div class="pills" aria-label="비교 기준">
                <button
                  type="button"
                  aria-pressed="false"
                  onclick={() => selectComparisonMode('work')}>작업</button
                >
                <button
                  type="button"
                  class="active"
                  aria-pressed="true"
                  onclick={() => selectComparisonMode('travel')}>이동</button
                >
              </div>
              {@render ownedFilter()}
            </div>
          </header>
          <div class="ranking">
            {#each travelRanking as pal, index (pal.id)}{@render comparisonRow(
                pal,
                index,
                `스태미나 ${pal.stamina.toString()}`,
                `탑승 질주 ${formatUserNumber(pal.ride_sprint_speed, {
                  maximumFractionDigits: 0,
                })}`,
              )}{/each}
          </div>
        </section>
      {:else}
        <section class="tool-panel">
          <header>
            <div>
              <h1>{presentation.title}</h1>
            </div>
            <div class="material-inputs">
              <div
                class="building-picker"
                bind:this={buildingPickerShell}
                onfocusout={closeBuildingPickerOnBlur}
              >
                <label for="material-building-search">건축물</label>
                <input
                  id="material-building-search"
                  type="search"
                  role="combobox"
                  aria-autocomplete="list"
                  aria-expanded={buildingPickerOpen}
                  aria-controls="material-building-options"
                  aria-activedescendant={buildingPickerOpen && matchingBuildings.length > 0
                    ? `material-building-option-${activeBuildingIndex.toString()}`
                    : undefined}
                  value={buildingSearch}
                  placeholder="한글 건축물 이름"
                  autocomplete="off"
                  onfocus={() => (buildingPickerOpen = true)}
                  oninput={(event) => updateBuildingSearch(event.currentTarget.value)}
                  onkeydown={buildingPickerKeydown}
                />
                {#if buildingPickerOpen}
                  <div
                    id="material-building-options"
                    class="building-options"
                    role="listbox"
                    aria-label="건축물 검색 결과"
                  >
                    {#if matchingBuildings.length === 0}
                      <p>일치하는 건축물이 없습니다.</p>
                    {:else}
                      {#each matchingBuildings as entry, index (entry.id)}
                        <button
                          id={`material-building-option-${index.toString()}`}
                          type="button"
                          role="option"
                          aria-selected={entry.id === buildingId}
                          class:active={index === activeBuildingIndex}
                          onmouseenter={() => (activeBuildingIndex = index)}
                          onclick={() => chooseBuilding(entry)}
                        >
                          <strong>{entry.name_ko}</strong>
                          <small>{buildingMaterialSummary(entry)}</small>
                        </button>
                      {/each}
                    {/if}
                  </div>
                {/if}
              </div>
              <label class="quantity-field"
                >수량<input type="number" min="1" max="999" bind:value={quantity} /></label
              >
            </div>
          </header>
          {#if building}<div class="material-summary">
              <div>
                <small>선택한 건축물</small>
                <h3>
                  <a class="entity-link" href={buildingHref(building)}>{building.name_ko}</a>
                </h3>
              </div>
              <strong>× {Math.max(1, Math.min(999, Math.trunc(quantity) || 1))}</strong>
            </div>
            <div class="materials" role="list" aria-label="필요 재료">
              {#each materials as material (material.item_id)}<article role="listitem">
                  <a
                    class="material-row"
                    href={itemHref(material.name_ko, material.item_id)}
                    aria-label={`${material.name_ko} 아이템 상세 열기`}
                  >
                    <div><strong>{material.name_ko}</strong><small>필요 재료</small></div>
                    <span
                      >{material.quantity} × {Math.max(
                        1,
                        Math.min(999, Math.trunc(quantity) || 1),
                      )}</span
                    ><b>{material.total.toLocaleString('ko-KR')}</b>
                  </a>
                </article>{/each}
            </div>{/if}
        </section>
      {/if}
    </div>
  {/key}
</section>

<style>
  .tool-panel header span {
    color: var(--brass);
    font-size: 0.75rem;
    font-weight: 850;
    letter-spacing: 0.13em;
  }
  .tool-panel {
    border: 1px solid var(--border);
    border-radius: 7px;
    background: var(--ink);
  }
  .entity-link {
    color: inherit;
    text-decoration-color: transparent;
    text-underline-offset: 3px;
    transition:
      color var(--motion-fast) var(--ease-standard),
      text-decoration-color var(--motion-fast) var(--ease-standard);
  }
  .entity-link:hover,
  .entity-link:focus-visible {
    color: var(--accent);
    text-decoration-color: currentColor;
  }
  .tool-mode-stage {
    min-width: 0;
    animation: tool-mode-enter var(--motion-base) var(--ease-emphasized) both;
  }
  @keyframes tool-mode-enter {
    from {
      opacity: 0;
      transform: translateY(4px);
    }
    to {
      opacity: 1;
      transform: translateY(0);
    }
  }
  .tool-panel > header {
    display: flex;
    align-items: end;
    justify-content: space-between;
    gap: 18px;
    padding: 18px;
    border-bottom: 1px solid var(--border);
  }
  h1 {
    margin: 0;
    font-size: 1.2rem;
  }
  .pills {
    display: flex;
    flex-wrap: wrap;
    gap: 5px;
  }
  .header-actions,
  .compare-controls {
    display: flex;
    flex-wrap: wrap;
    align-items: end;
    justify-content: flex-end;
    gap: 8px;
  }
  .owned-filter {
    min-height: 40px;
    padding-inline: 12px;
    border: 1px solid var(--border);
    border-radius: 4px;
    background: var(--surface);
    cursor: pointer;
    font-size: 0.75rem;
    font-weight: 800;
  }
  .owned-filter.active {
    border-color: var(--success);
    color: var(--success);
  }
  .owned-value {
    color: var(--success) !important;
    font-weight: 800;
  }
  .pills button {
    min-height: 40px;
    padding-inline: 12px;
    border: 1px solid var(--border);
    border-radius: 4px;
    background: var(--surface);
    cursor: pointer;
    font-size: 0.75rem;
  }
  .pills button.active {
    border-color: var(--accent);
    color: var(--accent);
    background: color-mix(in srgb, var(--accent), var(--void) 88%);
    font-weight: 800;
  }
  .recommendations {
    display: grid;
    grid-template-columns: repeat(5, minmax(0, 1fr));
    gap: 1px;
    background: var(--border);
  }
  .recommendations article {
    position: relative;
    display: grid;
    min-width: 0;
    min-height: 230px;
    align-content: start;
    padding: 18px;
    background: var(--surface);
  }
  .recommendations img {
    width: 84px;
    height: 84px;
    margin: auto;
    object-fit: contain;
  }
  .recommendations .rank {
    position: absolute;
    top: 12px;
    left: 12px;
    color: var(--brass);
    font-size: 0.75rem;
    font-weight: 850;
  }
  .recommendations h3 {
    margin: 5px 0;
    font-size: 0.9rem;
  }
  .recommendations small,
  .recommendations p {
    color: var(--muted);
    font-size: 0.75rem;
  }
  .recommendations p {
    min-height: 45px;
    line-height: 1.5;
  }
  .tool-panel label {
    display: grid;
    gap: 6px;
    color: var(--muted-strong);
    font-size: 0.75rem;
  }
  .tool-panel select,
  .tool-panel input {
    min-height: 42px;
    min-width: 220px;
    padding-inline: 10px;
    border: 1px solid var(--border-strong);
    border-radius: 4px;
    background: var(--void);
  }
  .ranking {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 1px;
    background: var(--border);
  }
  .ranking article {
    display: grid;
    min-height: 68px;
    grid-template-columns: 28px 48px minmax(0, 1fr) auto;
    gap: 10px;
    align-items: center;
    padding: 9px 14px;
    background: var(--surface);
  }
  .ranking img {
    width: 48px;
    height: 48px;
    object-fit: contain;
  }
  .ranking article > span {
    color: var(--brass);
    font-weight: 850;
  }
  .ranking article > div {
    display: grid;
    min-width: 0;
  }
  .ranking small {
    color: var(--muted);
    font-size: 0.75rem;
  }
  .ranking b {
    color: var(--tech-normal);
    font-size: 0.76rem;
  }
  .material-inputs {
    display: flex;
    gap: 8px;
  }
  .building-picker {
    position: relative;
    display: grid;
    min-width: 280px;
    gap: 6px;
    color: var(--muted-strong);
    font-size: 0.75rem;
  }
  .building-picker > input {
    width: 100%;
    min-width: 0;
  }
  .building-options {
    position: absolute;
    z-index: 12;
    top: calc(100% + 4px);
    right: 0;
    left: 0;
    display: grid;
    max-height: min(360px, 54vh);
    overflow-y: auto;
    padding: 4px;
    border: 1px solid var(--border-strong);
    border-radius: 5px;
    background: var(--surface-raised);
    box-shadow: 0 18px 42px rgb(0 0 0 / 46%);
  }
  .building-options button {
    display: grid;
    min-width: 0;
    min-height: 52px;
    align-content: center;
    gap: 3px;
    padding: 8px 10px;
    border: 0;
    border-bottom: 1px solid var(--border);
    color: var(--text);
    background: transparent;
    cursor: pointer;
    text-align: left;
  }
  .building-options button:last-child {
    border-bottom: 0;
  }
  .building-options button.active,
  .building-options button:focus-visible,
  .building-options button[aria-selected='true'] {
    background: color-mix(in srgb, var(--accent), var(--surface-raised) 90%);
    box-shadow: inset 3px 0 var(--accent);
  }
  .building-options strong {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .building-options small,
  .building-options p {
    margin: 0;
    color: var(--text-soft);
    font-size: 0.75rem;
  }
  .building-options p {
    padding: 16px 10px;
  }
  .quantity-field input {
    min-width: 90px;
    width: 90px;
  }
  .material-summary {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 22px;
    border-bottom: 1px solid var(--border);
  }
  .material-summary small,
  .material-summary h3 {
    margin: 5px 0;
  }
  .material-summary > strong {
    color: var(--brass);
    font-size: 1.4rem;
  }
  .materials {
    display: grid;
    grid-template-columns: repeat(2, 1fr);
    gap: 1px;
    background: var(--border);
  }
  .materials article {
    background: var(--surface);
  }
  .material-row {
    display: grid;
    min-height: 68px;
    grid-template-columns: minmax(0, 1fr) auto auto 12px;
    gap: 14px;
    align-items: center;
    padding: 12px 16px;
    color: inherit;
    text-decoration: none;
  }
  .material-row:hover,
  .material-row:focus-visible {
    color: var(--accent);
    background: color-mix(in srgb, var(--accent) 7%, var(--surface));
  }
  .material-row::after {
    color: var(--muted-strong);
    content: '›';
    font-size: 1.1rem;
  }
  .material-row > div {
    display: grid;
  }
  .materials small {
    color: var(--muted);
    font-size: 0.75rem;
  }
  .materials span {
    color: var(--muted-strong);
    font-size: 0.75rem;
  }
  .materials b {
    color: var(--brass);
  }
  .breeding-workspace {
    display: grid;
    grid-template-columns: minmax(260px, 0.34fr) minmax(0, 1fr);
    min-height: 440px;
  }
  .breeding-inputs {
    display: grid;
    align-content: start;
    gap: 14px;
    padding: 18px;
    border-right: 1px solid var(--border);
    background: var(--sidebar);
  }
  .breeding-inputs select,
  .breeding-inputs input {
    width: 100%;
    min-width: 0;
  }
  .breeding-inputs input {
    min-height: 38px;
  }
  .load-more {
    width: calc(100% - 32px);
    min-height: 42px;
    margin: 12px 16px 16px;
    border: 1px solid var(--border-strong);
    border-radius: 4px;
    color: var(--text-soft);
    background: var(--surface-raised);
    cursor: pointer;
    font-weight: 800;
  }
  .breeding-results {
    min-width: 0;
  }
  .breeding-results > header {
    display: flex;
    min-height: 52px;
    align-items: center;
    justify-content: space-between;
    padding: 10px 16px;
    border-bottom: 1px solid var(--border);
  }
  .breeding-results > header span {
    color: var(--brass);
    font-size: 0.75rem;
    font-weight: 800;
  }
  .breeding-result {
    display: grid;
    grid-template-columns: 90px minmax(0, 1fr);
    gap: 16px;
    align-items: center;
    padding: 24px;
  }
  .breeding-result img {
    width: 90px;
    height: 90px;
    object-fit: contain;
  }
  .breeding-result span {
    color: var(--brass);
    font-size: 0.75rem;
    font-weight: 850;
  }
  .breeding-result h3 {
    margin: 6px 0 4px;
    font-size: 1.25rem;
  }
  .combination-list,
  .path-list {
    display: grid;
    max-height: 520px;
    overflow-y: auto;
    scrollbar-color: var(--border-strong) var(--void);
  }
  .combination-list article,
  .path-list article {
    display: grid;
    min-height: 64px;
    align-items: center;
    gap: 12px;
    padding: 10px 14px;
    border-bottom: 1px solid var(--border);
  }
  .combination-list article {
    grid-template-columns: 28px minmax(0, 1fr) 18px minmax(0, 1fr) auto;
  }
  .path-list article {
    grid-template-columns: 58px minmax(0, 0.7fr) minmax(0, 1fr) auto;
  }
  .combination-list article > span,
  .path-list article > span {
    color: var(--brass);
    font-size: 0.75rem;
    font-weight: 850;
  }
  .combination-list article > div,
  .path-list article > div {
    display: grid;
    min-width: 0;
  }
  .combination-list strong,
  .path-list strong {
    overflow: hidden;
    font-size: 0.76rem;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .combination-list b {
    color: var(--muted);
  }
  .combination-list em,
  .path-list em {
    color: var(--tech-normal);
    font-size: 0.75rem;
    font-style: normal;
    font-weight: 800;
  }
  .path-list p {
    margin: 0;
    color: var(--muted-strong);
    font-size: 0.75rem;
  }
  .result-limit,
  .empty {
    margin: 0;
    padding: 16px;
    color: var(--muted-strong);
    font-size: 0.75rem;
    text-align: center;
  }
  @media (max-width: 1120px) {
    .recommendations {
      grid-template-columns: repeat(3, 1fr);
    }
    .breeding-workspace {
      grid-template-columns: minmax(240px, 0.4fr) minmax(0, 1fr);
    }
  }
  @media (max-width: 719px) {
    .breeding-workspace {
      grid-template-columns: 1fr;
    }
    .breeding-inputs {
      border-right: 0;
      border-bottom: 1px solid var(--border);
    }
    .combination-list article {
      grid-template-columns: 24px minmax(0, 1fr) 16px minmax(0, 1fr);
    }
    .combination-list em {
      grid-column: 2 / -1;
    }
    .path-list article {
      grid-template-columns: 54px minmax(0, 1fr) auto;
    }
    .path-list p {
      grid-column: 2 / -1;
    }
    .tool-panel > header {
      align-items: stretch;
      flex-direction: column;
    }
    .recommendations {
      grid-template-columns: 1fr;
    }
    .recommendations article {
      min-height: 132px;
      grid-template-columns: 82px minmax(0, 1fr);
      gap: 10px;
    }
    .recommendations img {
      grid-row: 1/3;
    }
    .ranking {
      grid-template-columns: 1fr;
    }
    .ranking article {
      grid-template-columns: 24px 44px minmax(0, 1fr) auto;
    }
    .material-inputs {
      align-items: stretch;
      flex-direction: column;
    }
    .building-picker {
      min-width: 0;
    }
    .tool-panel select {
      min-width: 0;
      width: 100%;
    }
    .materials {
      grid-template-columns: 1fr;
    }
  }
</style>
