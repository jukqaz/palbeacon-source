import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import TechnologyBoard from './TechnologyBoard.svelte';
import type { TechnologyCatalog, TechnologyRecord } from './types';

const record = (
  id: string,
  name: string,
  level: number,
  lane: 'normal' | 'ancient',
): TechnologyRecord => ({
  id,
  name_ko: name,
  description_ko: `${name} 설명`,
  icon_path: null,
  level,
  cost: lane === 'ancient' ? 3 : 2,
  tier: 1,
  lane,
  localization_fallback: false,
  prerequisite: {
    technology_id: null,
    technology_name_ko: null,
    tower_boss: null,
    research_id: null,
  },
  unlocks: [{ kind: 'item', id: `Item_${id}`, name_ko: `${name} 해금품` }],
});

const normal = record('Technology_WorkBench', '원시적인 작업대', 1, 'normal');
const normalTen = record('Technology_Bed', '푹신한 침대', 10, 'normal');
const ancientTen = record('Technology_AncientSaddle', '고대 안장', 10, 'ancient');

const catalog: TechnologyCatalog = {
  schema_version: 1,
  game_build_id: 'steam:24575825',
  mapping_sha256: 'mapping',
  verified: true,
  contract_review_id: null,
  source: { path: 'fixture', sha256: 'fixture' },
  statistics: { level_count: 2, technology_count: 3, ancient_count: 1, icon_count: 0 },
  technologies: [normal, normalTen, ancientTen],
};

describe('TechnologyBoard', () => {
  it('renders four consecutive game-like level rows with separate normal and ancient lanes', async () => {
    const extendedCatalog = {
      ...catalog,
      technologies: [
        normal,
        record('Technology_Level2', '레벨 이 기술', 2, 'normal'),
        record('Technology_Level3', '레벨 삼 기술', 3, 'normal'),
        record('Technology_Level4', '레벨 사 기술', 4, 'normal'),
        record('Technology_Level5', '레벨 오 기술', 5, 'normal'),
        normalTen,
        ancientTen,
      ],
    };
    render(TechnologyBoard, { catalog: extendedCatalog, initialLevel: 1 });
    expect(screen.queryByText('검증 완료')).not.toBeInTheDocument();
    expect(screen.queryByText('설치 게임')).not.toBeInTheDocument();
    expect(screen.queryByText('데이터 기준')).not.toBeInTheDocument();
    expect(screen.queryByText('포인트 합계')).not.toBeInTheDocument();
    expect(document.querySelector('.lane-columns')).toHaveTextContent('일반 기술');
    expect(document.querySelector('.lane-columns')).toHaveTextContent('고대 기술');

    const levelOne = document.querySelector('#technology-level-1');
    expect(levelOne).not.toBeNull();
    expect(within(levelOne as HTMLElement).queryByText('고대 기술')).not.toBeInTheDocument();
    expect(levelOne?.querySelector('.lane-block.ancient')).toHaveAttribute('aria-hidden', 'true');
    expect(document.querySelector('#technology-level-4')).not.toBeNull();
    expect(document.querySelector('#technology-level-5')).toBeNull();
    expect(document.querySelector('#technology-level-10')).toBeNull();

    await fireEvent.click(screen.getByRole('button', { name: '레벨 5' }));
    expect(document.querySelector('#technology-level-5')).not.toBeNull();
    expect(document.querySelector('#technology-level-10')).not.toBeNull();

    await fireEvent.click(screen.getByRole('button', { name: '레벨 10' }));
    const levelTen = document.querySelector('#technology-level-10');
    expect(levelTen).not.toBeNull();
    expect(
      within(levelTen as HTMLElement).getByRole('button', { name: /고대 안장, 레벨 10/ }),
    ).toBeInTheDocument();
    expect(levelTen?.querySelector('.lane-block.ancient')).toHaveAttribute(
      'aria-label',
      '레벨 10 고대 기술',
    );
  });

  it('moves to the nearest available level when a lane changes', async () => {
    render(TechnologyBoard, { catalog, initialLevel: 1 });
    await fireEvent.click(screen.getByRole('button', { name: '고대 기술' }));

    await waitFor(() => expect(document.querySelector('#technology-level-10')).not.toBeNull());
    expect(document.querySelector('#technology-level-1')).toBeNull();
    expect(screen.queryByText('일치하는 기술이 없습니다.')).not.toBeInTheDocument();
  });

  it('searches by unlock name and opens a labeled inspector', async () => {
    render(TechnologyBoard, { catalog });
    await fireEvent.input(screen.getByRole('searchbox'), {
      target: { value: '고대 안장 해금품' },
    });
    expect(screen.getByRole('button', { name: /고대 안장, 레벨 10/ })).toBeInTheDocument();
    expect(screen.queryByText('원시적인 작업대')).not.toBeInTheDocument();

    await fireEvent.click(screen.getByRole('button', { name: /고대 안장, 레벨 10/ }));
    const inspector = screen.getByRole('complementary', { name: '선택한 기술 상세' });
    expect(inspector).toBeInTheDocument();
    expect(within(inspector).queryByText('Technology_AncientSaddle')).not.toBeInTheDocument();
    expect(within(inspector).getAllByText(/해금 항목/)).toHaveLength(1);
    expect(within(inspector).queryByText('티어')).not.toBeInTheDocument();
    expect(within(inspector).queryByText('Item_Technology_AncientSaddle')).not.toBeInTheDocument();
    expect(document.querySelector('.icon-frame')).not.toBeInTheDocument();
    expect(within(inspector).queryByText('고', { exact: true })).not.toBeInTheDocument();
  });

  it('opens an exact technology handoff without exposing its ID in the search field', async () => {
    const { rerender } = render(TechnologyBoard, {
      catalog,
      initialQuery: '고대 안장',
      initialSelectedId: 'Technology_AncientSaddle',
    });

    expect(screen.getByRole('searchbox')).toHaveValue('고대 안장');
    expect(
      within(screen.getByRole('complementary', { name: '선택한 기술 상세' })).getByRole('heading', {
        name: '고대 안장',
      }),
    ).toBeInTheDocument();

    await rerender({
      catalog,
      initialQuery: '푹신한 침대',
      initialSelectedId: 'Technology_Bed',
    });
    expect(screen.getByRole('searchbox')).toHaveValue('푹신한 침대');
    expect(
      within(screen.getByRole('complementary', { name: '선택한 기술 상세' })).getByRole('heading', {
        name: '푹신한 침대',
      }),
    ).toBeInTheDocument();
  });

  it('recovers from an empty state without leaving stale filters', async () => {
    render(TechnologyBoard, { catalog, initialQuery: '존재하지않음', initialLane: 'ancient' });
    expect(screen.getByText('일치하는 기술이 없습니다.')).toBeInTheDocument();
    await fireEvent.click(screen.getByRole('button', { name: '검색과 필터 초기화' }));
    expect(screen.getByText('푹신한 침대')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: '전체' })).toHaveAttribute('aria-pressed', 'true');
  });

  it('steps through available levels without relying on horizontal scrolling', async () => {
    render(TechnologyBoard, { catalog, initialLevel: 1 });
    expect(screen.getByRole('button', { name: '이전 기술 레벨' })).toBeDisabled();
    await fireEvent.click(screen.getByRole('button', { name: '다음 기술 레벨' }));
    expect(document.querySelector('#technology-level-10')).not.toBeNull();
    expect(screen.getByRole('button', { name: '다음 기술 레벨' })).toBeDisabled();
  });

  it('hides unsafe display content and failed image slots', async () => {
    const unsafe = {
      ...normal,
      name_ko: '안전 기술',
      description_ko: '사용자에게 필요한 설명이다. WorkBench에서 제작한다.',
      icon_path: '/broken-technology.png',
      prerequisite: {
        technology_id: null,
        technology_name_ko: null,
        tower_boss: 'TowerBoss_A',
        research_id: 'Research_A',
      },
      unlocks: [{ kind: 'item' as const, id: 'Workbench', name_ko: 'Workbench' }],
    };
    render(TechnologyBoard, { catalog: { ...catalog, technologies: [unsafe] }, initialLevel: 1 });
    const card = screen.getByRole('button', { name: /안전 기술, 레벨 1/ });
    const image = card.querySelector('img');
    expect(image).not.toBeNull();
    await fireEvent.error(image as HTMLImageElement);
    expect(card.querySelector('img')).toBeNull();

    await fireEvent.click(card);
    const inspector = screen.getByRole('complementary', { name: '선택한 기술 상세' });
    expect(within(inspector).getByText('사용자에게 필요한 설명이다.')).toBeInTheDocument();
    expect(
      within(inspector).queryByText(/WorkBench|타워 보스|연구|처치 필요/),
    ).not.toBeInTheDocument();
    const inspectorImage = inspector.querySelector('img');
    expect(inspectorImage).not.toBeNull();
    await fireEvent.error(inspectorImage as HTMLImageElement);
    expect(inspector.querySelector('img')).toBeNull();
  });

  it('uses a compact sequential detail and restores focus to its card', async () => {
    const mediaQuery = vi.spyOn(window, 'matchMedia').mockReturnValue({
      matches: true,
      media: '(max-width: 1023px)',
      onchange: null,
      addListener: vi.fn<() => void>(),
      removeListener: vi.fn<() => void>(),
      addEventListener: vi.fn<() => void>(),
      removeEventListener: vi.fn<() => void>(),
      dispatchEvent: vi.fn<() => boolean>(),
    });
    try {
      render(TechnologyBoard, { catalog, initialLevel: 10 });
      const card = screen.getByRole('button', { name: /푹신한 침대, 레벨 10/ });
      card.focus();
      await fireEvent.click(card);
      expect(screen.getByRole('complementary', { name: '선택한 기술 상세' })).toHaveFocus();
      await fireEvent.click(screen.getByText('기술 목록'));
      expect(card).toHaveFocus();
    } finally {
      mediaQuery.mockRestore();
    }
  });
});
