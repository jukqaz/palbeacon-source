<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { Navigation03Icon } from '@hugeicons/core-free-icons';
  import { HugeiconsIcon } from '@hugeicons/svelte';
  import { toast } from 'svelte-sonner';
  import PalSlider from '$lib/shared/ui/PalSlider.svelte';
  import PalSwitch from '$lib/shared/ui/PalSwitch.svelte';
  import { loadNativeOverlayControlSnapshot, updateNativeOverlayControl } from './native';
  import type {
    NativeOverlayControlDocument,
    NativeOverlayControlSnapshot,
    NativeOverlayHotkeyChord,
    NativeOverlaySettings,
    NativeRuntimeStatus,
  } from './types';

  interface Props {
    load?: () => Promise<NativeOverlayControlSnapshot>;
    save?: (
      expectedVersion: number,
      settings: NativeOverlaySettings,
    ) => Promise<NativeOverlayControlDocument>;
  }

  type EditableHotkey = 'overlay_visibility' | 'rotation_toggle';

  let { load = loadNativeOverlayControlSnapshot, save = updateNativeOverlayControl }: Props =
    $props();
  let runtime = $state<NativeRuntimeStatus | null>(null);
  let document = $state<NativeOverlayControlDocument | null>(null);
  let draft = $state<NativeOverlaySettings | null>(null);
  let loading = $state(true);
  let saving = $state(false);
  let error = $state(false);
  let editingHotkey = $state<EditableHotkey | null>(null);
  let hotkeyError = $state<string | null>(null);
  let captureElement = $state<HTMLDivElement | null>(null);

  const previewTiles = ['1_1.jpg', '2_1.jpg', '1_2.jpg', '2_2.jpg'] as const;

  const sameStrings = (left: readonly string[], right: readonly string[]): boolean =>
    left.length === right.length && left.every((value, index) => value === right[index]);

  const sameChord = (
    left: NativeOverlayHotkeyChord | null,
    right: NativeOverlayHotkeyChord | null,
  ): boolean => {
    if (left === null || right === null) return left === right;
    return (
      left.virtual_key === right.virtual_key &&
      left.modifiers.alt === right.modifiers.alt &&
      left.modifiers.control === right.modifiers.control &&
      left.modifiers.shift === right.modifiers.shift &&
      left.modifiers.windows === right.modifiers.windows
    );
  };

  const sameSettings = (left: NativeOverlaySettings, right: NativeOverlaySettings): boolean =>
    left.enabled === right.enabled &&
    left.rotation_mode === right.rotation_mode &&
    left.input_mode === right.input_mode &&
    left.display_mode === right.display_mode &&
    Math.abs(left.opacity - right.opacity) < 0.0001 &&
    left.diameter_px === right.diameter_px &&
    Math.abs(left.zoom - right.zoom) < 0.0001 &&
    left.fps_profile === right.fps_profile &&
    left.poi_filters.fast_travel === right.poi_filters.fast_travel &&
    left.poi_filters.boss === right.poi_filters.boss &&
    left.poi_filters.dungeon === right.poi_filters.dungeon &&
    left.poi_filters.wanted === right.poi_filters.wanted &&
    sameStrings(left.poi_filters.enabled_layer_ids, right.poi_filters.enabled_layer_ids) &&
    sameStrings(left.poi_filters.selected_pal_ids, right.poi_filters.selected_pal_ids) &&
    left.poi_filters.night_only === right.poi_filters.night_only &&
    sameChord(left.hotkey_bindings.overlay_visibility, right.hotkey_bindings.overlay_visibility) &&
    sameChord(left.hotkey_bindings.rotation_toggle, right.hotkey_bindings.rotation_toggle) &&
    sameChord(
      left.hotkey_bindings.temporary_interaction,
      right.hotkey_bindings.temporary_interaction,
    ) &&
    sameChord(left.hotkey_bindings.interaction_lock, right.hotkey_bindings.interaction_lock);

  const dirty = $derived(
    document !== null && draft !== null && !sameSettings(document.settings, draft),
  );
  const sizeName = $derived.by(() => {
    if (!draft || draft.diameter_px < 300) return '작게';
    if (draft.diameter_px < 460) return '기본';
    return '크게';
  });
  const clarityName = $derived.by(() => {
    if (!draft || draft.opacity < 0.5) return '은은하게';
    if (draft.opacity < 0.8) return '또렷하게';
    return '선명하게';
  });
  const sceneMapSize = $derived(draft ? 156 + ((draft.diameter_px - 180) / 460) * 96 : 184);
  const specimenMapSize = $derived(draft ? 202 + ((draft.diameter_px - 180) / 460) * 68 : 224);

  const copyChord = (chord: NativeOverlayHotkeyChord | null): NativeOverlayHotkeyChord | null =>
    chord === null ? null : { modifiers: { ...chord.modifiers }, virtual_key: chord.virtual_key };

  const copySettings = (settings: NativeOverlaySettings): NativeOverlaySettings => ({
    ...settings,
    poi_filters: {
      ...settings.poi_filters,
      enabled_layer_ids: [...settings.poi_filters.enabled_layer_ids],
      selected_pal_ids: [...settings.poi_filters.selected_pal_ids],
    },
    hotkey_bindings: {
      overlay_visibility: copyChord(settings.hotkey_bindings.overlay_visibility),
      rotation_toggle: copyChord(settings.hotkey_bindings.rotation_toggle),
      temporary_interaction: copyChord(settings.hotkey_bindings.temporary_interaction),
      interaction_lock: copyChord(settings.hotkey_bindings.interaction_lock),
    },
  });

  const keyName = (virtualKey: number): string => {
    if (virtualKey >= 0x70 && virtualKey <= 0x87) return `F${String(virtualKey - 0x6f)}`;
    if (virtualKey >= 0x30 && virtualKey <= 0x39) return String.fromCharCode(virtualKey);
    if (virtualKey >= 0x41 && virtualKey <= 0x5a) return String.fromCharCode(virtualKey);
    const names: Record<number, string> = {
      0x20: 'Space',
      0x21: 'Page Up',
      0x22: 'Page Down',
      0x23: 'End',
      0x24: 'Home',
      0x25: '←',
      0x26: '↑',
      0x27: '→',
      0x28: '↓',
      0x2d: 'Insert',
      0x2e: 'Delete',
    };
    return names[virtualKey] ?? '키';
  };

  const hotkeyParts = (chord: NativeOverlayHotkeyChord | null): string[] => {
    if (chord === null) return ['설정 안 됨'];
    const parts: string[] = [];
    if (chord.modifiers.control) parts.push('Ctrl');
    if (chord.modifiers.alt) parts.push('Alt');
    if (chord.modifiers.shift) parts.push('Shift');
    if (chord.modifiers.windows) parts.push('Win');
    parts.push(keyName(chord.virtual_key));
    return parts;
  };

  const virtualKeyForEvent = (event: KeyboardEvent): number | null => {
    if (/^F(?:[1-9]|1[0-9]|2[0-4])$/.test(event.key)) {
      return 0x6f + Number(event.key.slice(1));
    }
    if (/^Key[A-Z]$/.test(event.code)) return event.code.charCodeAt(3);
    if (/^Digit[0-9]$/.test(event.code)) return event.code.charCodeAt(5);
    const keys: Record<string, number> = {
      ' ': 0x20,
      Space: 0x20,
      PageUp: 0x21,
      PageDown: 0x22,
      End: 0x23,
      Home: 0x24,
      ArrowLeft: 0x25,
      ArrowUp: 0x26,
      ArrowRight: 0x27,
      ArrowDown: 0x28,
      Insert: 0x2d,
      Delete: 0x2e,
    };
    return keys[event.key] ?? null;
  };

  const editHotkey = async (field: EditableHotkey) => {
    editingHotkey = field;
    hotkeyError = null;
    await tick();
    captureElement?.focus();
  };

  const stopEditingHotkey = () => {
    editingHotkey = null;
    hotkeyError = null;
  };

  const clearHotkey = () => {
    if (!draft || !editingHotkey) return;
    draft.hotkey_bindings[editingHotkey] = null;
    stopEditingHotkey();
  };

  const captureHotkey = (event: KeyboardEvent) => {
    if (!draft || !editingHotkey) return;
    if (event.key === 'Escape') {
      event.preventDefault();
      stopEditingHotkey();
      return;
    }
    if (['Control', 'Alt', 'Shift', 'Meta'].includes(event.key)) return;
    const virtualKey = virtualKeyForEvent(event);
    if (virtualKey === null) {
      hotkeyError = '이 키는 단축키로 사용할 수 없습니다.';
      return;
    }
    event.preventDefault();
    const chord: NativeOverlayHotkeyChord = {
      modifiers: {
        control: event.ctrlKey,
        alt: event.altKey,
        shift: event.shiftKey,
        windows: event.metaKey,
      },
      virtual_key: virtualKey,
    };
    const alphanumeric =
      (virtualKey >= 0x30 && virtualKey <= 0x39) || (virtualKey >= 0x41 && virtualKey <= 0x5a);
    const modified =
      chord.modifiers.control ||
      chord.modifiers.alt ||
      chord.modifiers.shift ||
      chord.modifiers.windows;
    if (alphanumeric && !modified) {
      hotkeyError = '문자와 숫자 키는 Ctrl, Alt, Shift 또는 Win과 함께 사용하세요.';
      return;
    }
    const otherField: EditableHotkey =
      editingHotkey === 'overlay_visibility' ? 'rotation_toggle' : 'overlay_visibility';
    if (sameChord(chord, draft.hotkey_bindings[otherField])) {
      hotkeyError = '이미 다른 단축키에서 사용 중입니다.';
      return;
    }
    draft.hotkey_bindings[editingHotkey] = chord;
    stopEditingHotkey();
  };

  const refresh = async () => {
    loading = true;
    error = false;
    stopEditingHotkey();
    try {
      const snapshot = await load();
      runtime = snapshot.runtime;
      document = snapshot.control;
      draft = copySettings(snapshot.control.settings);
    } catch (reason) {
      console.error('오버레이 설정을 불러오지 못했습니다.', reason);
      error = true;
    } finally {
      loading = false;
    }
  };

  const submit = async () => {
    if (!document || !draft || !dirty) return;
    saving = true;
    error = false;
    stopEditingHotkey();
    try {
      const next = await save(document.version, copySettings(draft));
      document = next;
      draft = copySettings(next.settings);
      toast.success('오버레이 설정을 적용했습니다.');
    } catch (reason) {
      console.error('오버레이 설정을 적용하지 못했습니다.', reason);
      error = true;
    } finally {
      saving = false;
    }
  };

  onMount(() => void refresh());
</script>

<svelte:window onkeydown={captureHotkey} />

{#snippet miniMap(settings: NativeOverlaySettings, size: number, label: string)}
  <div
    class="mini-map"
    class:disabled={!settings.enabled}
    style:--map-size={`${String(size)}px`}
    style:opacity={settings.opacity}
    role="img"
    aria-label={label}
  >
    <div class="map-raster" class:heading-up={settings.rotation_mode === 'heading_up'}>
      {#each previewTiles as tile (tile)}
        <img src={`/generated/map/tiles/${tile}`} alt="" />
      {/each}
    </div>
    {#if settings.poi_filters.fast_travel}
      <img class="poi fast" src="/generated/map/icons/compass-fast-travel.png" alt="" />
    {/if}
    {#if settings.poi_filters.boss}
      <img class="poi boss" src="/generated/map/icons/boss-category.webp" alt="" />
    {/if}
    {#if settings.poi_filters.dungeon}
      <img class="poi dungeon" src="/generated/map/icons/compass-dungeon.png" alt="" />
    {/if}
    {#if settings.poi_filters.wanted}
      <img class="poi wanted" src="/generated/map/icons/compass-bounty.png" alt="" />
    {/if}
    <span class="player" class:north-up={settings.rotation_mode === 'north_up'} aria-hidden="true">
      <HugeiconsIcon icon={Navigation03Icon} size={25} strokeWidth={2.6} />
    </span>
  </div>
{/snippet}

<section class="overlay-screen pal-screen">
  <header class="page-head">
    <div>
      <h1>게임 오버레이</h1>
    </div>
    {#if runtime}<p>{runtime.message_ko}</p>{/if}
  </header>

  {#if error}
    <section class="state error" role="alert">
      <strong>오버레이 설정을 적용하지 못했습니다.</strong>
      <button type="button" disabled={loading || saving} onclick={() => void refresh()}>
        다시 시도
      </button>
    </section>
  {/if}

  {#if runtime && document && draft}
    <form
      class="overlay-layout"
      onsubmit={(event) => {
        event.preventDefault();
        void submit();
      }}
    >
      <section class="game-preview" aria-labelledby="preview-title">
        <header>
          <div>
            <span class="section-number">01</span>
            <h2 id="preview-title">게임에서 보이는 모습</h2>
          </div>
          <span class="live-state"><i></i>{draft.enabled ? '표시 중' : '숨김'}</span>
        </header>
        <div class="game-scene">
          <img class="game-scene-image" src="/media/overlay-game-preview-clean.png" alt="" />
          <div class="scene-shade"></div>
          <div class="scene-map">
            {@render miniMap(draft, sceneMapSize, '게임 화면의 원형 미니맵 미리보기')}
          </div>
          <div class="scene-caption" aria-hidden="true">
            <span>현재 위치</span>
            <strong>{draft.rotation_mode === 'north_up' ? '북쪽 고정' : '진행 방향'}</strong>
          </div>
        </div>
      </section>

      <aside class="workshop" aria-labelledby="workshop-title">
        <header>
          <div>
            <span class="section-number">02</span>
            <h2 id="workshop-title">미니맵 조정</h2>
          </div>
          <PalSwitch bind:checked={draft.enabled} label="오버레이 표시" />
        </header>

        <div class="specimen">
          {@render miniMap(draft, specimenMapSize, '설정 중인 원형 미니맵')}
        </div>

        <fieldset class="direction-field">
          <legend>지도 방향</legend>
          <div class="segments">
            <label>
              <input type="radio" bind:group={draft.rotation_mode} value="north_up" />
              <span><strong>북쪽 고정</strong><small>지도는 고정, 화살표가 회전</small></span>
            </label>
            <label>
              <input type="radio" bind:group={draft.rotation_mode} value="heading_up" />
              <span><strong>진행 방향</strong><small>이동 방향에 맞춰 지도 회전</small></span>
            </label>
          </div>
        </fieldset>

        <section class="slider-control">
          <header><strong>지도 크기</strong><output>{sizeName}</output></header>
          <PalSlider
            bind:value={draft.diameter_px}
            min={180}
            max={640}
            step={20}
            label="지도 크기"
          />
          <div class="scale-labels" aria-hidden="true">
            <span>작게</span><span>기본</span><span>크게</span>
          </div>
        </section>

        <section class="slider-control">
          <header><strong>지도 선명도</strong><output>{clarityName}</output></header>
          <PalSlider bind:value={draft.opacity} min={0.2} max={1} step={0.01} label="지도 선명도" />
          <div class="scale-labels two" aria-hidden="true">
            <span>은은하게</span><span>선명하게</span>
          </div>
        </section>

        <section class="map-sync">
          <div>
            <strong>지도 선택 항목 동기화</strong>
            <p>PC 지도에서 선택한 항목이 그대로 표시됩니다.</p>
          </div>
          <a href="/map">지도 열기</a>
        </section>

        <section class="hotkey-section" aria-labelledby="hotkey-title">
          <header>
            <h3 id="hotkey-title">단축키</h3>
            <span>복합키 지원</span>
          </header>
          {#each ['overlay_visibility', 'rotation_toggle'] as field (field)}
            {@const typedField = field as EditableHotkey}
            {@const chord = draft.hotkey_bindings[typedField]}
            <div class="hotkey-row">
              <div class="hotkey-name">
                <strong
                  >{typedField === 'overlay_visibility'
                    ? '오버레이 표시 전환'
                    : '지도 방향 전환'}</strong
                >
                <div class="keycaps" aria-label={hotkeyParts(chord).join(' + ')}>
                  {#each hotkeyParts(chord) as part, index (part)}
                    {#if index > 0}<span class="plus" aria-hidden="true">+</span>{/if}<kbd
                      >{part}</kbd
                    >
                  {/each}
                </div>
              </div>
              <button
                class="edit-hotkey"
                type="button"
                aria-expanded={editingHotkey === typedField}
                onclick={() => void editHotkey(typedField)}>변경</button
              >
              {#if editingHotkey === typedField}
                <div
                  class="hotkey-capture"
                  role="dialog"
                  aria-label={`${typedField === 'overlay_visibility' ? '오버레이 표시' : '지도 방향'} 단축키 변경`}
                  tabindex="-1"
                  bind:this={captureElement}
                >
                  <strong>원하는 키 조합을 누르세요</strong>
                  <p>문자·숫자는 Ctrl, Alt, Shift 또는 Win과 함께 사용합니다.</p>
                  {#if hotkeyError}<p class="capture-error" role="alert">{hotkeyError}</p>{/if}
                  <div>
                    <button type="button" onclick={clearHotkey}>단축키 해제</button>
                    <button type="button" onclick={stopEditingHotkey}>취소</button>
                  </div>
                </div>
              {/if}
            </div>
          {/each}
        </section>

        <footer class="apply-bar">
          <button type="submit" disabled={!dirty || saving}>{saving ? '적용 중' : '적용'}</button>
        </footer>
      </aside>
    </form>
  {:else if loading}
    <section class="state" aria-live="polite">
      <strong>오버레이 설정을 읽는 중입니다.</strong>
    </section>
  {/if}
</section>

<style>
  .overlay-screen {
    --overlay-line: color-mix(in srgb, var(--accent), var(--border) 76%);
    --overlay-panel: color-mix(in srgb, var(--ink), var(--void) 24%);
  }
  .page-head {
    display: flex;
    min-height: 58px;
    align-items: flex-end;
    justify-content: space-between;
    gap: 24px;
    padding: 0 2px 12px;
    border-bottom: 1px solid var(--overlay-line);
  }
  .page-head > div {
    display: grid;
    gap: 3px;
  }
  .section-number {
    color: var(--accent);
    font: 800 0.62rem/1.2 monospace;
    letter-spacing: 0.14em;
  }
  .page-head h1,
  .game-preview h2,
  .workshop h2,
  .hotkey-section h3 {
    margin: 0;
  }
  .page-head h1 {
    font-size: 1.28rem;
  }
  .page-head p,
  .map-sync p,
  .hotkey-capture p {
    margin: 0;
    color: var(--muted-strong);
    font-size: 0.72rem;
    line-height: 1.55;
  }
  .page-head p {
    max-width: 46ch;
    text-align: right;
  }
  .overlay-layout {
    display: grid;
    grid-template-columns: minmax(520px, 1.28fr) minmax(360px, 0.72fr);
    align-items: start;
    gap: 14px;
    margin-top: 14px;
  }
  .game-preview,
  .workshop,
  .state {
    border: 1px solid var(--overlay-line);
    border-radius: 6px;
    background: var(--overlay-panel);
  }
  .game-preview,
  .workshop {
    overflow: hidden;
  }
  .game-preview > header,
  .workshop > header {
    display: flex;
    min-height: 56px;
    align-items: center;
    justify-content: space-between;
    gap: 20px;
    padding: 11px 15px;
    border-bottom: 1px solid var(--overlay-line);
  }
  .game-preview > header > div,
  .workshop > header > div {
    display: flex;
    align-items: baseline;
    gap: 10px;
  }
  .game-preview h2,
  .workshop h2 {
    font-size: 0.88rem;
  }
  .live-state {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    color: var(--muted-strong);
    font-size: 0.68rem;
    font-weight: 800;
  }
  .live-state i {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--accent);
    box-shadow: 0 0 10px color-mix(in srgb, var(--accent), transparent 35%);
  }
  .game-scene {
    position: relative;
    min-height: 650px;
    overflow: hidden;
    background: #07131c;
  }
  .game-scene-image,
  .scene-shade {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
  }
  .game-scene-image {
    object-fit: cover;
    object-position: right center;
  }
  .scene-shade {
    background: linear-gradient(90deg, rgb(2 11 18 / 34%), transparent 45%);
    pointer-events: none;
  }
  .scene-map {
    position: absolute;
    top: 27px;
    left: 27px;
  }
  .scene-caption {
    position: absolute;
    bottom: 22px;
    left: 24px;
    display: grid;
    gap: 3px;
    padding: 9px 12px;
    border-left: 2px solid var(--accent);
    background: rgb(3 14 22 / 76%);
  }
  .scene-caption span {
    color: var(--muted-strong);
    font-size: 0.62rem;
  }
  .scene-caption strong {
    font-size: 0.74rem;
  }
  .mini-map {
    --map-size: 224px;
    position: relative;
    width: var(--map-size);
    aspect-ratio: 1;
    overflow: hidden;
    border: 2px solid color-mix(in srgb, var(--accent), white 15%);
    border-radius: 50%;
    background: #07131c;
    box-shadow:
      0 0 0 5px rgb(5 17 27 / 92%),
      0 0 0 7px color-mix(in srgb, var(--accent), transparent 58%),
      0 18px 40px rgb(0 0 0 / 54%);
    transition:
      width 160ms ease,
      opacity 160ms ease,
      filter 160ms ease;
  }
  .mini-map.disabled {
    filter: grayscale(1);
    opacity: 0.32 !important;
  }
  .map-raster {
    position: absolute;
    inset: -5%;
    display: grid;
    grid-template-columns: repeat(2, 1fr);
    transform: scale(1.12);
    transition: transform 180ms ease;
  }
  .map-raster.heading-up {
    transform: rotate(-22deg) scale(1.26);
  }
  .map-raster img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }
  .player {
    position: absolute;
    z-index: 3;
    top: 50%;
    left: 50%;
    display: grid;
    width: 30px;
    height: 30px;
    place-items: center;
    border: 2px solid rgb(247 252 255 / 92%);
    border-radius: 50%;
    color: var(--accent);
    background: rgb(3 15 24 / 82%);
    filter: drop-shadow(0 2px 4px #000);
    transform: translate(-50%, -50%);
    transition: transform 180ms ease;
  }
  .player.north-up {
    transform: translate(-50%, -50%) rotate(22deg);
  }
  .poi {
    position: absolute;
    z-index: 2;
    width: max(18px, calc(var(--map-size) * 0.1));
    height: max(18px, calc(var(--map-size) * 0.1));
    object-fit: contain;
    filter: drop-shadow(0 2px 3px #000);
  }
  .poi.fast {
    top: 21%;
    right: 25%;
  }
  .poi.boss {
    right: 16%;
    bottom: 25%;
  }
  .poi.dungeon {
    left: 19%;
    bottom: 23%;
  }
  .poi.wanted {
    top: 26%;
    left: 25%;
  }
  .specimen {
    display: grid;
    min-height: 304px;
    place-items: center;
    padding: 24px;
    border-bottom: 1px solid var(--overlay-line);
    background:
      linear-gradient(var(--overlay-line) 1px, transparent 1px),
      linear-gradient(90deg, var(--overlay-line) 1px, transparent 1px), var(--void);
    background-size: 32px 32px;
  }
  fieldset,
  .slider-control {
    margin: 0;
    padding: 14px 16px;
    border: 0;
    border-bottom: 1px solid var(--overlay-line);
  }
  legend {
    padding: 0 0 9px;
    color: var(--muted-strong);
    font-size: 0.68rem;
    font-weight: 800;
  }
  .segments {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 7px;
  }
  .segments label {
    display: grid;
    min-height: 64px;
    grid-template-columns: auto 1fr;
    align-items: start;
    gap: 8px;
    padding: 10px;
    border: 1px solid var(--border);
    border-radius: 4px;
    background: var(--surface);
    cursor: pointer;
  }
  .segments label:has(input:checked) {
    border-color: var(--accent);
    background: color-mix(in srgb, var(--accent), transparent 91%);
  }
  .segments input {
    margin-top: 3px;
    accent-color: var(--accent);
  }
  .segments span {
    display: grid;
    gap: 4px;
  }
  .segments strong {
    font-size: 0.73rem;
  }
  .segments small {
    color: var(--muted);
    font-size: 0.62rem;
    line-height: 1.35;
  }
  .slider-control {
    display: grid;
    gap: 7px;
  }
  .slider-control > header,
  .hotkey-section > header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 14px;
  }
  .slider-control strong,
  .map-sync strong,
  .hotkey-name > strong {
    font-size: 0.73rem;
  }
  .slider-control output {
    color: var(--accent);
    font-size: 0.68rem;
    font-weight: 800;
  }
  .scale-labels {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    color: var(--muted);
    font-size: 0.6rem;
  }
  .scale-labels span:nth-child(2) {
    text-align: center;
  }
  .scale-labels span:last-child {
    text-align: right;
  }
  .scale-labels.two {
    grid-template-columns: repeat(2, 1fr);
  }
  .scale-labels.two span:nth-child(2) {
    text-align: right;
  }
  .map-sync {
    display: grid;
    grid-template-columns: 1fr auto;
    align-items: center;
    gap: 12px;
    padding: 13px 16px;
    border-bottom: 1px solid var(--overlay-line);
  }
  .map-sync > div {
    display: grid;
    gap: 3px;
  }
  .map-sync a,
  button {
    min-height: 36px;
    padding: 0 13px;
    border: 1px solid var(--border-strong);
    border-radius: 4px;
    color: var(--text);
    background: var(--surface-raised);
    cursor: pointer;
    font: 800 0.7rem/1 sans-serif;
  }
  .map-sync a {
    display: inline-flex;
    align-items: center;
    text-decoration: none;
  }
  .hotkey-section > header {
    min-height: 44px;
    padding: 0 16px;
    border-bottom: 1px solid var(--overlay-line);
  }
  .hotkey-section h3 {
    font-size: 0.76rem;
  }
  .hotkey-section > header span {
    color: var(--muted);
    font-size: 0.61rem;
  }
  .hotkey-row {
    position: relative;
    display: grid;
    min-height: 64px;
    grid-template-columns: 1fr auto;
    align-items: center;
    gap: 12px;
    padding: 10px 16px;
    border-bottom: 1px solid var(--overlay-line);
  }
  .hotkey-name {
    display: grid;
    grid-template-columns: minmax(126px, 1fr) auto;
    align-items: center;
    gap: 12px;
  }
  .keycaps {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 4px;
  }
  kbd {
    min-width: 28px;
    padding: 5px 7px;
    border: 1px solid var(--border-strong);
    border-bottom-width: 2px;
    border-radius: 3px;
    background: var(--void);
    font: 750 0.64rem/1 monospace;
    text-align: center;
  }
  .plus {
    color: var(--muted);
    font-size: 0.62rem;
  }
  .edit-hotkey[aria-expanded='true'] {
    border-color: var(--accent);
    color: var(--accent);
  }
  .hotkey-capture {
    position: absolute;
    z-index: 6;
    top: calc(100% - 6px);
    right: 16px;
    display: grid;
    width: min(320px, calc(100vw - 54px));
    gap: 8px;
    padding: 14px;
    border: 1px solid var(--accent);
    border-radius: 5px;
    outline: none;
    background: var(--surface-raised);
    box-shadow: 0 16px 40px rgb(0 0 0 / 48%);
  }
  .hotkey-capture > strong {
    font-size: 0.76rem;
  }
  .hotkey-capture > div {
    display: flex;
    justify-content: flex-end;
    gap: 7px;
  }
  .hotkey-capture .capture-error {
    color: var(--danger);
  }
  .apply-bar {
    display: flex;
    justify-content: flex-end;
    padding: 14px 16px;
    background: var(--void);
  }
  .apply-bar button {
    min-width: 118px;
    min-height: 42px;
    border-color: color-mix(in srgb, var(--accent), white 12%);
    color: var(--void);
    background: var(--accent);
  }
  button:disabled {
    cursor: not-allowed;
    opacity: 0.42;
  }
  .state {
    display: flex;
    min-height: 74px;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    margin-top: 14px;
    padding: 16px;
  }
  .state.error {
    border-color: color-mix(in srgb, var(--danger), transparent 35%);
  }
  .state strong {
    font-size: 0.78rem;
  }

  @media (max-width: 1080px) {
    .overlay-layout {
      grid-template-columns: 1fr;
    }
    .game-scene {
      min-height: 520px;
    }
    .workshop {
      display: grid;
      grid-template-columns: minmax(280px, 0.8fr) minmax(360px, 1.2fr);
    }
    .workshop > header,
    .workshop .specimen,
    .workshop .apply-bar {
      grid-column: 1 / -1;
    }
    .direction-field,
    .slider-control,
    .map-sync {
      grid-column: 1;
    }
    .hotkey-section {
      grid-column: 2;
      grid-row: 2 / span 4;
      border-left: 1px solid var(--overlay-line);
    }
  }
  @media (max-width: 720px) {
    .page-head {
      align-items: flex-start;
      flex-direction: column;
      gap: 8px;
    }
    .page-head p {
      text-align: left;
    }
    .game-scene {
      min-height: 400px;
    }
    .workshop {
      display: block;
    }
    .hotkey-section {
      border-left: 0;
    }
    .hotkey-name {
      grid-template-columns: 1fr;
      gap: 7px;
    }
    .keycaps {
      justify-content: flex-start;
    }
  }
  @media (max-width: 430px) {
    .game-scene {
      min-height: 330px;
    }
    .scene-map {
      top: 22px;
      left: 22px;
      transform: scale(0.82);
      transform-origin: top left;
    }
    .specimen {
      min-height: 270px;
      padding-inline: 8px;
    }
    .segments {
      grid-template-columns: 1fr;
    }
    .map-sync {
      align-items: stretch;
      grid-template-columns: 1fr;
    }
    .map-sync a {
      justify-content: center;
    }
    .apply-bar button {
      width: 100%;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .mini-map,
    .map-raster,
    .player {
      transition: none;
    }
  }
</style>
