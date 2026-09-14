import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import MapBoard from './MapBoard.svelte';
import { mapFixture } from './map.fixture';

const filterButton = (container: HTMLElement, label: string): HTMLElement => {
  const button = within(container)
    .getAllByRole('button', { name: new RegExp(label) })
    .find((candidate) => candidate.hasAttribute('aria-pressed'));
  if (!button) throw new Error(`${label} 지도 필터를 찾지 못했습니다.`);
  return button;
};

describe('MapBoard', () => {
  it('searches Korean names without exposing internal IDs in the inspector', async () => {
    render(MapBoard, { bundle: mapFixture });
    await fireEvent.input(screen.getByRole('searchbox', { name: '위치 검색' }), {
      target: { value: '라이바오' },
    });

    const controls = screen.getByRole('complementary', { name: '지도 검색과 필터' });
    await fireEvent.click(within(controls).getByRole('button', { name: /라이바오/ }));
    const inspector = screen.getByRole('complementary', { name: '선택한 지도 위치 상세' });
    expect(within(inspector).getByRole('heading', { name: '라이바오' })).toBeInTheDocument();
    expect(within(inspector).getByText('동쪽')).toBeInTheDocument();
    expect(within(inspector).queryByText('GrassPanda_Electric')).not.toBeInTheDocument();
    expect(within(inspector).queryByText('검증 상태')).not.toBeInTheDocument();
    expect(within(inspector).queryByText('게임 데이터 검증 완료')).not.toBeInTheDocument();
  });

  it('places active Korean search results directly after the search field', async () => {
    render(MapBoard, { bundle: mapFixture });
    const search = screen.getByRole('searchbox', { name: '위치 검색' });
    await fireEvent.input(search, { target: { value: '라이바오' } });

    const results = document.querySelector<HTMLElement>('.results');
    expect(results).not.toBeNull();
    if (!results) return;
    expect(within(results).getByRole('button', { name: /라이바오/ })).toBeInTheDocument();
    expect(search.closest('.map-search')?.nextElementSibling).toBe(results);
    expect(document.querySelector('.filter-section')).toHaveClass('searching');
  });

  it('shows every public data-backed category and no hidden or merged source labels', () => {
    render(MapBoard, { bundle: mapFixture });
    const controls = screen.getByRole('complementary', { name: '지도 검색과 필터' });
    expect(filterButton(controls, '참수리 상')).toBeInTheDocument();
    expect(filterButton(controls, '보스 타워')).toBeInTheDocument();
    expect(filterButton(controls, '순수한 석영')).toBeInTheDocument();
    expect(within(controls).queryByText('봉인된 영역')).not.toBeInTheDocument();
    expect(within(controls).queryByText('고대 유적')).not.toBeInTheDocument();
    expect(screen.getByText(/지도에 \d+개 표시 중/)).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /주변 장소/ })).not.toBeInTheDocument();
    expect(screen.queryByText(/보조 좌표|외부 보조 레이어|PalMods|PalDB/)).not.toBeInTheDocument();
  });

  it('switches regions without inventing a live player position', async () => {
    render(MapBoard, { bundle: mapFixture });
    await fireEvent.click(screen.getByRole('tab', { name: '세계수' }));
    expect(screen.queryByText(/현재 위치/)).not.toBeInTheDocument();
    expect(screen.getByText('지도에 1개 표시 중')).toBeInTheDocument();
    const controls = screen.getByRole('complementary', { name: '지도 검색과 필터' });
    expect(within(controls).getByRole('button', { name: /현상범 에고/ })).toBeInTheDocument();
    const egg = filterButton(controls, '팰의 알');
    await fireEvent.click(egg);
    expect(screen.getByText('지도에 2개 표시 중')).toBeInTheDocument();
  });

  it('publishes exact and supplemental filters as one shared selection', async () => {
    const onEnabledFilterIdsChange = vi.fn<(enabledFilterIds: readonly string[]) => void>();
    render(MapBoard, {
      bundle: mapFixture,
      initialEnabledFilterIds: ['fast_travel', 'dungeon', 'ore-quartz'],
      onEnabledFilterIdsChange,
    });

    const controls = screen.getByRole('complementary', { name: '지도 검색과 필터' });
    expect(filterButton(controls, '필드 보스')).toHaveAttribute('aria-pressed', 'false');
    await fireEvent.click(filterButton(controls, '필드 보스'));
    expect(onEnabledFilterIdsChange).toHaveBeenLastCalledWith([
      'fast_travel',
      'boss',
      'dungeon',
      'ore-quartz',
    ]);
  });

  it('supports purpose presets and enables every eligible category on request', async () => {
    render(MapBoard, { bundle: mapFixture });
    const controls = screen.getByRole('complementary', { name: '지도 검색과 필터' });

    await fireEvent.click(within(controls).getByRole('button', { name: '재료' }));
    expect(filterButton(controls, '순수한 석영')).toHaveAttribute('aria-pressed', 'true');
    expect(filterButton(controls, '필드 보스')).toHaveAttribute('aria-pressed', 'false');

    await fireEvent.click(within(controls).getByRole('button', { name: '모두 표시' }));
    for (const label of ['참수리 상', '던전', '필드 보스', '보스 타워', '순수한 석영']) {
      expect(filterButton(controls, label)).toHaveAttribute('aria-pressed', 'true');
    }
  });

  it('opens and closes the responsive filter panel without losing focus', async () => {
    render(MapBoard, { bundle: mapFixture });
    const trigger = document.querySelector<HTMLButtonElement>('.filter-trigger');
    if (!trigger) throw new Error('반응형 지도 필터 버튼을 찾지 못했습니다.');

    await fireEvent.click(trigger);
    expect(trigger).toHaveAttribute('aria-expanded', 'true');
    const close = document.querySelector<HTMLButtonElement>('.mobile-filter-head button');
    if (!close) throw new Error('반응형 지도 필터 닫기 버튼을 찾지 못했습니다.');
    await fireEvent.click(close);
    expect(trigger).toHaveAttribute('aria-expanded', 'false');
    expect(trigger).toHaveFocus();
  });

  it('uses a modal mobile filter and moves focus to the selected detail', async () => {
    const mediaQuery = vi.spyOn(window, 'matchMedia').mockImplementation(
      (query) =>
        ({
          matches: query.includes('max-width: 719px') || query.includes('prefers-reduced-motion'),
          media: query,
          onchange: null,
          addListener: vi.fn<() => void>(),
          removeListener: vi.fn<() => void>(),
          addEventListener: vi.fn<() => void>(),
          removeEventListener: vi.fn<() => void>(),
          dispatchEvent: vi.fn<() => boolean>(),
        }) as MediaQueryList,
    );
    try {
      render(MapBoard, { bundle: mapFixture });
      const trigger = document.querySelector<HTMLButtonElement>('.filter-trigger');
      if (!trigger) throw new Error('모바일 지도 필터 버튼을 찾지 못했습니다.');
      await fireEvent.click(trigger);

      const dialog = await screen.findByRole('dialog', { name: '지도 필터' });
      await fireEvent.input(within(dialog).getByRole('searchbox', { name: '위치 검색' }), {
        target: { value: '라이바오' },
      });
      await fireEvent.click(within(dialog).getByRole('button', { name: /라이바오/ }));

      await waitFor(() =>
        expect(screen.queryByRole('dialog', { name: '지도 필터' })).not.toBeInTheDocument(),
      );
      const detail = screen.getByRole('complementary', { name: '선택한 지도 위치 상세' });
      await waitFor(() => expect(detail).toHaveFocus());
    } finally {
      mediaQuery.mockRestore();
    }
  });

  it('keeps one map marker in the Tab order and uses arrow keys between markers', async () => {
    render(MapBoard, { bundle: mapFixture });
    const markers = [...document.querySelectorAll<SVGGElement>('[data-poi-marker]')];
    expect(markers.length).toBeGreaterThan(1);
    expect(markers.filter((marker) => marker.tabIndex === 0)).toHaveLength(1);

    const first = markers[0];
    const second = markers[1];
    if (!first || !second) throw new Error('키보드 이동을 검증할 지도 표식이 부족합니다.');
    first.focus();
    await fireEvent.keyDown(first, { key: 'ArrowRight' });
    await waitFor(() => expect(second).toHaveFocus());
    expect(second).toHaveAttribute('tabindex', '0');
    expect(markers.filter((marker) => marker.tabIndex === 0)).toHaveLength(1);
  });

  it('supports map exploration up to 800 percent without exceeding the limit', async () => {
    render(MapBoard, { bundle: mapFixture });
    const zoomIn = screen.getByRole('button', { name: '지도 확대' });
    const zoomOut = screen.getByRole('button', { name: '지도 축소' });

    expect(screen.getByText('100%')).toBeInTheDocument();
    expect(zoomOut).toBeDisabled();
    await fireEvent.click(zoomIn);
    await fireEvent.click(zoomIn);
    await fireEvent.click(zoomIn);
    await fireEvent.click(zoomIn);
    await fireEvent.click(zoomIn);
    await fireEvent.click(zoomIn);
    await fireEvent.click(zoomIn);

    expect(screen.getByText('800%')).toBeInTheDocument();
    expect(zoomIn).toBeDisabled();
    await fireEvent.click(zoomOut);
    expect(screen.queryByText('800%')).not.toBeInTheDocument();
  });
});
