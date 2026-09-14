<script lang="ts">
  import { resolve } from '$app/paths';
  import { ArrowLeft02Icon, Cancel01Icon } from '@hugeicons/core-free-icons';
  import { HugeiconsIcon } from '@hugeicons/svelte';
  import WorkspaceAction from '$lib/shared/workspace/WorkspaceAction.svelte';
  import {
    technologyDisplayDescription,
    technologyDisplayPrerequisite,
    technologyDisplayUnlocks,
  } from './catalog';
  import type { TechnologyRecord } from './types';

  interface Props {
    technology: TechnologyRecord;
    onclose: () => void;
  }

  let { technology, onclose }: Props = $props();
  let failedIconPath = $state<string | null>(null);
  const showIcon = $derived(
    technology.icon_path !== null && technology.icon_path !== failedIconPath,
  );
  const description = $derived(technologyDisplayDescription(technology));
  const visibleUnlocks = $derived(technologyDisplayUnlocks(technology));
  const prerequisite = $derived(technologyDisplayPrerequisite(technology));
  const workspaceEntry = $derived.by(() => {
    const parameters = new URLSearchParams({ q: technology.name_ko, id: technology.id });
    return {
      kind: 'technology' as const,
      id: technology.id,
      name_ko: technology.name_ko,
      href: `${resolve('/technology/', {})}?${parameters.toString()}`,
      image_path: technology.icon_path,
    };
  });
</script>

<aside
  id="technology-detail"
  class="inspector {technology.lane}"
  aria-label="선택한 기술 상세"
  tabindex="-1"
>
  <button class="compact-back" type="button" onclick={onclose}>
    <span aria-hidden="true"
      ><HugeiconsIcon icon={ArrowLeft02Icon} size={18} strokeWidth={2} /></span
    >
    기술 목록
  </button>
  <div class="inspector-head" class:no-icon={!showIcon}>
    {#if showIcon}
      <div class="selected-icon">
        <img
          src={technology.icon_path ?? ''}
          alt=""
          width="72"
          height="72"
          onerror={() => (failedIconPath = technology.icon_path)}
        />
      </div>
    {/if}
    <div>
      <span class="lane-label">{technology.lane === 'ancient' ? '고대 기술' : '일반 기술'}</span>
      <h2>{technology.name_ko}</h2>
    </div>
    <button class="desktop-close" type="button" aria-label="기술 상세 닫기" onclick={onclose}>
      <span aria-hidden="true"><HugeiconsIcon icon={Cancel01Icon} size={18} strokeWidth={2} /></span
      >
    </button>
  </div>

  <div class="workspace-detail-action"><WorkspaceAction entry={workspaceEntry} /></div>

  <div class="quick-facts">
    <div><small>요구 레벨</small><strong>레벨 {technology.level}</strong></div>
    <div><small>필요 포인트</small><strong>{technology.cost}포인트</strong></div>
  </div>

  {#if description}
    <p class="description">{description}</p>
  {/if}

  {#if visibleUnlocks.length > 0}
    <section>
      <h3>해금 항목 <span>{visibleUnlocks.length}</span></h3>
      <ul>
        {#each visibleUnlocks as unlock (`${unlock.kind}:${unlock.id}`)}
          <li>
            <span>{unlock.kind === 'item' ? '아이템' : '건축물'}</span>
            <strong>{unlock.name_ko}</strong>
          </li>
        {/each}
      </ul>
    </section>
  {/if}

  {#if prerequisite}
    <section>
      <h3>해금 조건</h3>
      <dl>
        <div>
          <dt>선행 기술</dt>
          <dd>{prerequisite}</dd>
        </div>
      </dl>
    </section>
  {/if}
</aside>

<style>
  .inspector {
    --lane-color: var(--tech-normal);
    position: sticky;
    top: 82px;
    max-height: calc(100vh - 102px);
    overflow: auto;
    border: 1px solid color-mix(in srgb, var(--lane-color), var(--border) 55%);
    border-radius: 8px;
    background: var(--ink);
    box-shadow: inset 0 3px 0 var(--lane-color);
  }

  .inspector.ancient {
    --lane-color: var(--tech-ancient);
  }

  .inspector-head {
    display: grid;
    grid-template-columns: 74px minmax(0, 1fr) auto;
    gap: 12px;
    align-items: center;
    padding: 16px;
    border-bottom: 1px solid var(--border);
  }

  .inspector-head.no-icon {
    grid-template-columns: minmax(0, 1fr) auto;
  }

  .selected-icon {
    display: grid;
    width: 74px;
    height: 74px;
    place-items: center;
    border: 1px solid color-mix(in srgb, var(--lane-color), transparent 35%);
    border-radius: 6px;
    color: var(--lane-color);
    background: rgb(8 12 18 / 72%);
    font-size: 1.2rem;
    font-weight: 850;
  }

  img {
    width: 72px;
    height: 72px;
    object-fit: contain;
  }

  .lane-label {
    color: var(--lane-color);
    font-size: 0.75rem;
    font-weight: 850;
    letter-spacing: 0.08em;
  }

  h2 {
    margin: 5px 0;
    font-size: 1.05rem;
    line-height: 1.25;
  }

  button {
    min-width: 44px;
    min-height: 38px;
    border: 1px solid var(--border);
    border-radius: 5px;
    color: var(--muted-strong);
    background: var(--surface);
    cursor: pointer;
    font-size: 0.75rem;
  }

  .compact-back {
    display: none;
  }

  .desktop-close {
    display: grid;
    width: 38px;
    padding: 0;
    place-items: center;
  }

  .quick-facts {
    display: grid;
    grid-template-columns: repeat(2, 1fr);
    gap: 1px;
    border-bottom: 1px solid var(--border);
    background: var(--border);
  }

  .workspace-detail-action {
    display: flex;
    justify-content: flex-end;
    padding: 8px 12px;
    border-bottom: 1px solid var(--border);
  }

  .quick-facts div {
    display: grid;
    gap: 4px;
    padding: 12px;
    background: var(--surface);
  }

  .quick-facts small,
  dt {
    color: var(--muted);
    font-size: 0.75rem;
  }

  .quick-facts strong {
    color: var(--lane-color);
    font-size: 0.82rem;
  }

  .description {
    margin: 0;
    padding: 16px;
    border-bottom: 1px solid var(--border);
    color: var(--text-soft);
    font-size: 0.8rem;
    line-height: 1.65;
  }

  section {
    padding: 16px;
    border-bottom: 1px solid var(--border);
  }

  h3 {
    margin: 0 0 11px;
    color: var(--text-soft);
    font-size: 0.75rem;
    letter-spacing: 0.03em;
  }

  h3 span {
    color: var(--lane-color);
  }

  ul,
  dl {
    display: grid;
    gap: 7px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  li {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr);
    gap: 3px 9px;
    padding: 9px;
    border: 1px solid var(--border);
    border-radius: 5px;
    background: rgb(18 26 36 / 72%);
  }

  li span {
    color: var(--lane-color);
    font-size: 0.75rem;
    font-weight: 800;
  }

  li strong {
    font-size: 0.76rem;
  }

  dl div {
    display: grid;
    grid-template-columns: 76px minmax(0, 1fr);
  }

  dd {
    margin: 0;
    color: var(--text-soft);
    font-size: 0.75rem;
    overflow-wrap: anywhere;
  }

  @media (max-width: 1023px) {
    .inspector {
      position: relative;
      top: auto;
      max-height: none;
      outline: none;
    }

    .compact-back {
      display: flex;
      width: 100%;
      min-height: 48px;
      align-items: center;
      gap: 7px;
      border-width: 0 0 1px;
      border-radius: 0;
      color: var(--lane-color);
      text-align: left;
    }

    .desktop-close {
      display: none;
    }
  }

  @media (max-width: 420px) {
    .inspector-head {
      grid-template-columns: 56px minmax(0, 1fr) auto;
      padding: 13px;
    }

    .inspector-head.no-icon {
      grid-template-columns: minmax(0, 1fr) auto;
    }

    .selected-icon,
    img {
      width: 56px;
      height: 56px;
    }

    .quick-facts {
      grid-template-columns: 1fr;
    }
  }
</style>
