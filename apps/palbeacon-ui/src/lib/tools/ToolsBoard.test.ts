import { fireEvent, render, screen } from '@testing-library/svelte';
import { beforeEach, describe, expect, it } from 'vitest';
import ToolsBoard from './ToolsBoard.svelte';
import { toolsFixture } from './tools.fixture';
import { clearPersonalPalData, personalPalData } from '$lib/personal/personal-data';
import { resetPlanningWorkspace } from './plan-workspace';

describe('ToolsBoard', () => {
  beforeEach(() => {
    localStorage.clear();
    clearPersonalPalData();
    resetPlanningWorkspace();
  });

  it('filters the large breeding selector by Korean name or paldeck number', async () => {
    render(ToolsBoard, { catalog: toolsFixture, initialMode: 'breeding' });
    const search = screen.getByRole('searchbox', { name: '첫 번째 부모 팰 검색' });
    const select = screen.getByRole<HTMLSelectElement>('combobox', { name: '첫 번째 부모 팰' });

    await fireEvent.input(search, { target: { value: '야간' } });
    expect(Array.from(select.options, (option) => option.text)).toEqual([
      '아누비스',
      '야간 작업팰',
    ]);
    expect(screen.queryByText('NightWorker')).not.toBeInTheDocument();
  });

  it('exposes exact forward, reverse and path breeding modes', async () => {
    render(ToolsBoard, { catalog: toolsFixture, initialMode: 'breeding' });
    const first = screen.getByRole('combobox', { name: '첫 번째 부모 팰' });
    const second = screen.getByRole('combobox', { name: '두 번째 부모 팰' });
    await fireEvent.change(first, { target: { value: 'Anubis' } });
    await fireEvent.change(second, { target: { value: 'NightWorker' } });
    expect(screen.getByRole('heading', { name: '빠른 탈것' })).toBeInTheDocument();
    expect(screen.getByText('특수 교배')).toBeInTheDocument();

    await fireEvent.click(screen.getByRole('button', { name: '역방향' }));
    const target = screen.getByRole('combobox', { name: '목표 팰' });
    await fireEvent.change(target, { target: { value: 'FastMount' } });
    expect(screen.getAllByText('아누비스').length).toBeGreaterThan(0);
    expect(screen.getAllByText('야간 작업팰').length).toBeGreaterThan(0);

    await fireEvent.click(screen.getByRole('button', { name: '도달 경로' }));
    expect(screen.getAllByText('1세대')).toHaveLength(2);
    expect(screen.getByText('최대 6세대 안에서 찾은 가장 짧은 경로입니다.')).toBeInTheDocument();
    expect(screen.queryByText('데이터 근거')).not.toBeInTheDocument();
  });

  it('shows exact decision values without internal scores and switches goals', async () => {
    render(ToolsBoard, { catalog: toolsFixture });
    expect(screen.getByRole('heading', { name: '팀 구성', level: 1 })).toBeInTheDocument();
    expect(screen.getByText('체력 120 · 공격 130 · 방어 100')).toBeInTheDocument();
    expect(screen.queryByText(/비교 점수|비교 모델|목표값/)).not.toBeInTheDocument();
    expect(screen.queryByText('119.0')).not.toBeInTheDocument();
    await fireEvent.click(screen.getByRole('button', { name: '야간 거점' }));
    expect(screen.getByText('야간 작업팰')).toBeInTheDocument();
    expect(screen.getByText(/야행성/)).toBeInTheDocument();
  });

  it('switches work and travel inside one comparison workspace', async () => {
    render(ToolsBoard, { catalog: toolsFixture, initialMode: 'work' });
    expect(screen.getByRole('heading', { name: '팰 비교', level: 1 })).toBeInTheDocument();
    expect(screen.getByRole('combobox', { name: '작업 종류' })).toBeInTheDocument();
    await fireEvent.click(screen.getByRole('button', { name: '이동' }));
    expect(screen.getAllByText(/탑승 질주/).length).toBeGreaterThan(0);
    expect(screen.queryByText(/이동 지수/)).not.toBeInTheDocument();
  });

  it('calculates materials inside the same responsive workspace', async () => {
    render(ToolsBoard, { catalog: toolsFixture, initialMode: 'materials' });
    const quantity = screen.getByRole('spinbutton', { name: '수량' });
    await fireEvent.input(quantity, { target: { value: '5' } });
    expect(screen.getByText('500')).toBeInTheDocument();
    expect(screen.getByText('100')).toBeInTheDocument();
  });

  it('finds a building by Korean name without rendering the full selector', async () => {
    const catalog = {
      ...toolsFixture,
      buildings: [
        ...toolsFixture.buildings,
        {
          id: 'SecondFarm',
          name_ko: '배합 농장',
          category: 'Pal',
          materials: [{ item_id: 'Fiber', name_ko: '섬유', quantity: 50 }],
        },
      ],
    };
    render(ToolsBoard, { catalog, initialMode: 'materials' });
    const picker = screen.getByRole('combobox', { name: '건축물' });

    await fireEvent.input(picker, { target: { value: '배합' } });
    const options = screen.getAllByRole('option');
    expect(options).toHaveLength(2);
    expect(screen.getByRole('option', { name: /배합 목장.*목재 100.*돌 20/u })).toBeVisible();
    await fireEvent.click(screen.getByRole('option', { name: /배합 농장.*섬유 50/u }));
    expect(picker).toHaveValue('배합 농장');
    expect(screen.getByRole('heading', { name: '배합 농장', level: 3 })).toBeVisible();
  });

  it('accepts an exact building handoff and links the result back to catalog details', () => {
    const catalog = {
      ...toolsFixture,
      buildings: [
        ...toolsFixture.buildings,
        {
          id: 'SecondFarm',
          name_ko: '배합 농장',
          category: 'Pal',
          materials: [{ item_id: 'Fiber', name_ko: '섬유', quantity: 50 }],
        },
      ],
    };
    render(ToolsBoard, {
      catalog,
      initialMode: 'materials',
      initialBuildingId: 'SecondFarm',
    });

    expect(screen.getByRole('combobox', { name: '건축물' })).toHaveValue('배합 농장');
    expect(screen.getByRole('link', { name: '배합 농장' })).toHaveAttribute(
      'href',
      '/buildings/?q=%EB%B0%B0%ED%95%A9+%EB%86%8D%EC%9E%A5&id=SecondFarm',
    );
    expect(screen.getByRole('link', { name: '섬유 아이템 상세 열기' })).toHaveAttribute(
      'href',
      '/items/?q=%EC%84%AC%EC%9C%A0&id=Fiber',
    );
  });

  it('accepts an exact Pal handoff as a reverse breeding target', () => {
    render(ToolsBoard, {
      catalog: toolsFixture,
      initialMode: 'breeding',
      initialTargetId: 'FastMount',
    });

    expect(screen.getByRole('button', { name: '역방향' })).toHaveAttribute('aria-pressed', 'true');
    expect(screen.getByRole('combobox', { name: '목표 팰' })).toHaveValue('FastMount');
    expect(screen.getAllByRole('link', { name: '아누비스' })[0]).toHaveAttribute(
      'href',
      '/pals/?q=%EC%95%84%EB%88%84%EB%B9%84%EC%8A%A4&id=Anubis',
    );
  });

  it('accepts the Windows handoff and limits recommendations to exact owned species', () => {
    personalPalData.set({
      ready: true,
      owner_name: 'Arthur',
      owned_by_species: {
        Anubis: { name_ko: '아누비스', count: 2, highest_level: 55 },
      },
    });
    render(ToolsBoard, { catalog: toolsFixture, initialOwnedOnly: true });

    expect(screen.getByRole('button', { name: '내 팰만' })).toHaveAttribute('aria-pressed', 'true');
    expect(screen.getByText('보유 2마리')).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: '아누비스' })).toBeInTheDocument();
    expect(screen.queryByRole('heading', { name: '빠른 탈것' })).not.toBeInTheDocument();
  });

  it('restores the current team criterion after leaving the planning route', async () => {
    const first = render(ToolsBoard, { catalog: toolsFixture });
    await fireEvent.click(screen.getByRole('button', { name: '야간 거점' }));
    first.unmount();

    render(ToolsBoard, { catalog: toolsFixture });
    expect(screen.getByRole('button', { name: '야간 거점' })).toHaveAttribute(
      'aria-pressed',
      'true',
    );
  });
});
