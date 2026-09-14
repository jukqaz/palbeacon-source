<script lang="ts">
  import { technologyDisplayUnlocks } from './catalog';
  import type { TechnologyRecord } from './types';

  interface Props {
    technology: TechnologyRecord;
    selected?: boolean;
    onselect: (technology: TechnologyRecord, trigger: HTMLButtonElement) => void;
  }

  let { technology, selected = false, onselect }: Props = $props();
  let failedIconPath = $state<string | null>(null);
  const showIcon = $derived(
    technology.icon_path !== null && technology.icon_path !== failedIconPath,
  );
  const visibleUnlocks = $derived(technologyDisplayUnlocks(technology));
  const unlockKind = $derived.by(() => {
    const first = visibleUnlocks[0]?.kind;
    return first && visibleUnlocks.every((unlock) => unlock.kind === first) ? first : null;
  });
  const unlockKindLabel = $derived(
    unlockKind === 'item' ? '아이템' : unlockKind === 'building' ? '건축물' : null,
  );
  const accessibleName = $derived(
    `${technology.name_ko}, 레벨 ${technology.level.toString()}, ${technology.cost.toString()} 포인트${technology.lane === 'ancient' ? ', 고대 기술' : ''}`,
  );
</script>

<button
  type="button"
  class="technology-card {technology.lane}"
  class:selected
  class:no-kind={unlockKindLabel === null}
  aria-label={accessibleName}
  aria-pressed={selected}
  onclick={(event) => onselect(technology, event.currentTarget)}
>
  {#if unlockKindLabel}
    <span class="kind">{unlockKindLabel}</span>
  {/if}
  <span class="visual" class:no-icon={!showIcon}>
    {#if showIcon}
      <span class="icon-frame">
        <img
          src={technology.icon_path ?? ''}
          alt=""
          loading="lazy"
          width="58"
          height="58"
          onerror={() => (failedIconPath = technology.icon_path)}
        />
      </span>
    {/if}
    <span class="cost" aria-label={`${technology.cost.toString()} 포인트`}>
      <b>{technology.cost}</b><small>포인트</small>
    </span>
  </span>
  <span class="copy">
    <strong>{technology.name_ko}</strong>
  </span>
</button>

<style>
  .technology-card {
    container-type: inline-size;
    position: relative;
    display: grid;
    min-width: 0;
    min-height: 116px;
    grid-template-rows: auto 58px minmax(28px, auto);
    align-content: center;
    justify-items: stretch;
    gap: 3px;
    overflow: hidden;
    padding: 6px 7px 8px;
    border: 1px solid color-mix(in srgb, var(--lane-color), var(--border) 66%);
    border-radius: 4px;
    color: var(--text);
    background: var(--surface);
    text-align: center;
    cursor: pointer;
    transition:
      border-color 120ms ease,
      background 120ms ease;
  }

  .technology-card:hover,
  .technology-card.selected {
    border-color: var(--lane-color);
    background: color-mix(in srgb, var(--lane-surface), var(--surface) 35%);
  }

  .technology-card.selected {
    box-shadow: inset 0 -3px 0 var(--lane-color);
  }

  .technology-card.no-kind {
    grid-template-rows: 58px minmax(28px, auto);
  }

  .normal {
    --lane-color: var(--tech-normal);
    --lane-surface: var(--tech-normal-surface);
  }

  .ancient {
    --lane-color: var(--tech-ancient);
    --lane-surface: var(--tech-ancient-surface);
  }

  .kind {
    min-height: 16px;
    overflow: hidden;
    color: var(--brass);
    font-size: 0.75rem;
    font-weight: 800;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .visual {
    position: relative;
    display: grid;
    min-width: 0;
    min-height: 58px;
    place-items: center;
  }

  .visual.no-icon {
    min-height: 44px;
  }

  .icon-frame {
    display: grid;
    width: 58px;
    height: 58px;
    place-items: center;
    overflow: hidden;
  }

  img {
    display: block;
    width: 58px;
    height: 58px;
    object-fit: contain;
  }

  .copy {
    display: grid;
    min-width: 0;
    place-items: center;
  }

  .copy strong {
    display: -webkit-box;
    overflow: hidden;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    text-overflow: ellipsis;
    font-size: 0.75rem;
    line-height: 1.25;
  }

  .cost {
    position: absolute;
    right: 1px;
    bottom: 0;
    display: flex;
    align-items: baseline;
    gap: 2px;
    color: var(--lane-color);
  }

  .cost b {
    font-size: 1rem;
  }

  .cost small {
    font-size: 0.68rem;
    font-weight: 850;
    letter-spacing: -0.03em;
  }

  @container (max-width: 96px) {
    .technology-card {
      min-height: 108px;
      padding-inline: 4px;
    }

    .icon-frame,
    img {
      width: 48px;
      height: 48px;
    }

    .cost small {
      position: absolute;
      width: 1px;
      height: 1px;
      overflow: hidden;
      clip-path: inset(50%);
    }
  }
</style>
