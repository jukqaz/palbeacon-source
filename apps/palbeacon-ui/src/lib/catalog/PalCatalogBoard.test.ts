import { readFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { beforeEach, describe, expect, it } from 'vitest';
import { catalogFixture } from './catalog.fixture';
import PalCatalogBoard from './PalCatalogBoard.svelte';
import type { CatalogRecord, UnifiedCatalog } from './types';
import { clearPersonalPalData, personalPalData } from '$lib/personal/personal-data';

const loadGeneratedCatalog = async (): Promise<UnifiedCatalog> =>
  JSON.parse(
    await readFile(resolve(process.cwd(), 'static/generated/game/catalog.v1.json'), 'utf8'),
  ) as UnifiedCatalog;

const loadExactPals = async (...ids: string[]): Promise<UnifiedCatalog> => {
  const generated = await loadGeneratedCatalog();
  const requested = new Set(ids);
  const pals = generated.records.pals.filter((pal) => requested.has(pal.id));
  expect(pals).toHaveLength(ids.length);
  return {
    ...generated,
    records: { ...generated.records, pals },
  };
};

const expectResultCount = (count: number) => {
  const heading = screen.getByRole('heading', { name: '팰 도감' });
  expect(within(heading.closest('header')!).getByText(`${count.toString()}종`)).toBeInTheDocument();
};

describe('PalCatalogBoard', () => {
  beforeEach(() => {
    clearPersonalPalData();
    window.history.replaceState({}, '', '/pals/');
  });

  it('uses exact Paldex order and searches raw IDs without exposing them', async () => {
    const catalog = await loadExactPals('LittleBriarRose', 'WindChimes', 'KendoFrog_Dark');
    render(PalCatalogBoard, { catalog });

    const results = [
      ...screen.getByLabelText('팰 도감 결과').querySelectorAll<HTMLButtonElement>('.pal-select'),
    ];
    expect(results.map((result) => result.querySelector('strong')?.textContent)).toEqual([
      '검구리',
      '건다리',
      '가시공주',
    ]);

    await fireEvent.input(screen.getByRole('searchbox', { name: '팰 도감 검색' }), {
      target: { value: 'LittleBriarRose' },
    });
    expect(screen.getByRole('button', { name: /가시공주.*팰/ })).toBeInTheDocument();
    expect(screen.queryByText('LittleBriarRose')).not.toBeInTheDocument();
  });

  it('combines multi-element and exact work requirements without inventing values', async () => {
    const catalog = await loadExactPals('LittleBriarRose', 'WindChimes', 'KendoFrog_Dark');
    render(PalCatalogBoard, { catalog });

    await fireEvent.click(screen.getByRole('button', { name: '속성' }));
    await fireEvent.click(screen.getByRole('checkbox', { name: '풀' }));
    await fireEvent.click(screen.getByRole('button', { name: '속성 1' }));
    await fireEvent.click(screen.getByRole('checkbox', { name: '땅' }));
    expectResultCount(2);
    expect(screen.getByRole('button', { name: /가시공주.*팰/ })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /건다리.*팰/ })).toBeInTheDocument();

    await fireEvent.click(screen.getByRole('button', { name: '작업 적성' }));
    await fireEvent.click(screen.getByRole('checkbox', { name: '수작업' }));
    await fireEvent.click(screen.getByRole('checkbox', { name: '운반' }));
    await fireEvent.change(screen.getByRole('combobox', { name: '선택 작업 최소 레벨' }), {
      target: { value: '2' },
    });
    expectResultCount(0);
    await fireEvent.click(screen.getByRole('radio', { name: '하나 이상' }));
    expectResultCount(2);

    await fireEvent.click(screen.getAllByRole('button', { name: '초기화' })[0]!);
    expectResultCount(3);
  });

  it('filters exact nocturnal and minimum combat values', async () => {
    const catalog = await loadExactPals('LittleBriarRose', 'WindChimes', 'KendoFrog_Dark');
    render(PalCatalogBoard, { catalog });

    await fireEvent.click(screen.getByRole('button', { name: '상세 필터' }));
    await fireEvent.click(screen.getByRole('checkbox', { name: '야행성만' }));
    await fireEvent.input(screen.getByRole('spinbutton', { name: '공격 최소' }), {
      target: { value: '100' },
    });

    expectResultCount(1);
    expect(screen.getByRole('button', { name: /검구리.*팰/ })).toBeInTheDocument();
    expect(screen.queryByText('KendoFrog_Dark')).not.toBeInTheDocument();
  });

  it('keeps query, filters, and selection when switching comparison modes', async () => {
    const catalog = await loadExactPals('LittleBriarRose', 'WindChimes', 'KendoFrog_Dark');
    render(PalCatalogBoard, { catalog });

    await fireEvent.input(screen.getByRole('searchbox', { name: '팰 도감 검색' }), {
      target: { value: '검구리' },
    });
    const gridResult = screen.getByRole('button', { name: /검구리.*팰/ });
    await fireEvent.click(gridResult);
    await fireEvent.click(screen.getByRole('button', { name: '목록 보기' }));

    expect(screen.getByRole('button', { name: '목록 보기' })).toHaveAttribute(
      'aria-pressed',
      'true',
    );
    expect(screen.getByRole('searchbox', { name: '팰 도감 검색' })).toHaveValue('검구리');
    expect(screen.getByRole('button', { name: /검구리.*팰/ })).toHaveAttribute(
      'aria-pressed',
      'true',
    );
  });

  it('explains compact work icons on focus or click', async () => {
    render(PalCatalogBoard, { catalog: catalogFixture });
    const workButton = screen.getByRole('button', { name: '수작업 레벨 2' });

    await fireEvent.click(workButton);
    expect(workButton).toHaveAttribute('aria-pressed', 'true');
    expect(document.querySelector('.work-tooltip.pinned')).toHaveTextContent('수작업 Lv. 2');
  });

  it('keeps exact overview, work, skills, and drops in separate detail tabs', async () => {
    render(PalCatalogBoard, { catalog: catalogFixture });
    await fireEvent.click(screen.getByRole('button', { name: /가시공주.*팰/ }));

    const inspector = screen.getByRole('complementary', { name: '선택한 팰 상세' });
    expect(within(inspector).getByRole('heading', { name: '가시공주' })).toBeInTheDocument();
    expect(within(inspector).getByText('주요 작업')).toBeInTheDocument();
    expect(within(inspector).getByText('수작업 Lv. 2 외 1개')).toBeInTheDocument();
    expect(within(inspector).getByText('식사량')).toBeInTheDocument();
    expect(within(inspector).getByRole('link', { name: '교배 계획' })).toHaveAttribute(
      'href',
      '/plan/breeding/?target=LittleBriarRose',
    );
    expect(within(inspector).queryByText('LittleBriarRose')).not.toBeInTheDocument();

    await fireEvent.click(within(inspector).getByRole('tab', { name: '작업 2' }));
    expect(within(inspector).getByText('수작업')).toBeInTheDocument();
    expect(within(inspector).getByText('Lv. 2')).toBeInTheDocument();

    await fireEvent.click(within(inspector).getByRole('tab', { name: '스킬 1' }));
    expect(within(inspector).getByText('바람의 칼날')).toBeInTheDocument();
    const skillMeta = inspector.querySelector<HTMLElement>('.skill-meta');
    expect(skillMeta).not.toBeNull();
    expect(within(skillMeta!).getByText('풀')).toBeInTheDocument();
    expect(within(inspector).getByText('위력 40')).toBeInTheDocument();
    expect(within(inspector).getByText('재사용 2초')).toBeInTheDocument();

    await fireEvent.click(within(inspector).getByRole('tab', { name: '획득 1' }));
    expect(within(inspector).getByRole('link', { name: /부패 독 여과막/ })).toBeInTheDocument();
    expect(within(inspector).getByText('5%')).toBeInTheDocument();
    expect(within(inspector).getByText('1–2개')).toBeInTheDocument();
  });

  it('groups exact normal and boss drops and reads certain probability as confirmed', async () => {
    const catalog = await loadExactPals('LittleBriarRose');
    render(PalCatalogBoard, { catalog });
    await fireEvent.click(screen.getByRole('button', { name: /가시공주.*팰/ }));

    const inspector = screen.getByRole('complementary', { name: '선택한 팰 상세' });
    await fireEvent.click(within(inspector).getByRole('tab', { name: '획득 4' }));
    expect(within(inspector).getByRole('heading', { name: '일반 드롭' })).toBeInTheDocument();
    expect(within(inspector).getByRole('heading', { name: '보스 드롭' })).toBeInTheDocument();
    expect(within(inspector).getByRole('link', { name: /토마토 씨/ })).toBeInTheDocument();
    expect(within(inspector).getAllByText('확정')).toHaveLength(3);
    expect(within(inspector).queryByText('100%')).not.toBeInTheDocument();
    expect(within(inspector).queryByText('보스', { exact: true })).not.toBeInTheDocument();
  });

  it('hides absent sections and failed exact media without a diagnostic label', async () => {
    const pal = catalogFixture.records.pals[0]!;
    const partialPal: CatalogRecord = { ...pal };
    delete partialPal.work_suitability;
    delete partialPal.pal_skills;
    delete partialPal.pal_drops;
    const partialCatalog: UnifiedCatalog = {
      ...catalogFixture,
      records: { ...catalogFixture.records, pals: [partialPal] },
    };
    render(PalCatalogBoard, { catalog: partialCatalog });

    const result = screen.getByRole('button', { name: /가시공주.*팰/ });
    const image = result.querySelector<HTMLImageElement>('img[src*="LittleBriarRose"]');
    expect(image).not.toBeNull();
    await fireEvent.error(image!);
    expect(within(result).queryByText('이미지 미확인')).not.toBeInTheDocument();
    expect(result.querySelector('.card-media')).not.toBeInTheDocument();

    await fireEvent.click(result);
    const inspector = screen.getByRole('complementary', { name: '선택한 팰 상세' });
    expect(within(inspector).getAllByRole('tab')).toHaveLength(1);
    expect(within(inspector).queryByText(/작업 \d/u)).not.toBeInTheDocument();
    expect(within(inspector).queryByText(/스킬 \d/u)).not.toBeInTheDocument();
    expect(within(inspector).queryByText(/획득 \d/u)).not.toBeInTheDocument();
  });

  it('returns focus to the selected row after closing the narrow detail', async () => {
    const { container } = render(PalCatalogBoard, { catalog: catalogFixture });
    const result = screen.getByRole('button', { name: /가시공주.*팰/ });
    await fireEvent.click(result);
    const back = container.querySelector<HTMLButtonElement>('.mobile-back');
    expect(back).not.toBeNull();
    await fireEvent.click(back!);
    await waitFor(() => expect(result).toHaveFocus());
  });

  it('renders exact narrow results in bounded batches', async () => {
    const catalog = await loadGeneratedCatalog();
    const { container } = render(PalCatalogBoard, { catalog });

    expect(container.querySelectorAll('.pal-card')).toHaveLength(24);
    await fireEvent.click(screen.getByRole('button', { name: '다음 24종 보기' }));
    expect(container.querySelectorAll('.pal-card')).toHaveLength(48);

    await fireEvent.input(screen.getByRole('searchbox', { name: '팰 도감 검색' }), {
      target: { value: '가시공주' },
    });
    expect(container.querySelectorAll('.pal-card')).toHaveLength(1);
    expect(screen.getByRole('button', { name: /가시공주.*팰/u })).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /다음 \d+종 보기/u })).not.toBeInTheDocument();
  });

  it('closes the compact quick attribute filter after one choice', async () => {
    render(PalCatalogBoard, { catalog: catalogFixture });
    await fireEvent.click(screen.getByRole('button', { name: '속성' }));
    await fireEvent.click(screen.getByRole('checkbox', { name: '풀' }));

    expect(screen.getByRole('button', { name: '속성 1' })).toHaveAttribute(
      'aria-expanded',
      'false',
    );
    expect(screen.getByRole('searchbox', { name: '팰 도감 검색' })).toBeVisible();
  });

  it('shows an exact owned count only when a selected local save provides it', async () => {
    personalPalData.set({
      ready: true,
      owner_name: 'Arthur',
      owned_by_species: {
        LittleBriarRose: { name_ko: '가시공주', count: 2, highest_level: 42 },
      },
    });
    render(PalCatalogBoard, { catalog: catalogFixture });

    const result = screen.getByRole('button', { name: /가시공주.*보유 2마리.*팰/ });
    expect(within(result).getByText('보유 2마리')).toBeInTheDocument();
    await fireEvent.click(result);
    expect(
      within(screen.getByRole('complementary', { name: '선택한 팰 상세' })).getByText('보유 2마리'),
    ).toBeInTheDocument();
  });

  it('opens from the Windows handoff with only exact owned species and keeps the filter removable', async () => {
    const catalog = await loadExactPals('LittleBriarRose', 'WindChimes', 'KendoFrog_Dark');
    personalPalData.set({
      ready: true,
      owner_name: 'Arthur',
      owned_by_species: {
        LittleBriarRose: { name_ko: '가시공주', count: 2, highest_level: 42 },
      },
    });
    window.history.replaceState({}, '', '/pals/?mine=1');

    render(PalCatalogBoard, { catalog, initialOwnedOnly: true });

    expect(screen.getByRole('button', { name: '내 팰만' })).toHaveAttribute('aria-pressed', 'true');
    expectResultCount(1);
    expect(screen.getByRole('button', { name: /가시공주.*보유 2마리.*팰/ })).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /검구리.*팰/ })).not.toBeInTheDocument();

    await fireEvent.click(screen.getByRole('button', { name: '내 팰 ×' }));
    expectResultCount(3);
    expect(window.location.search).toBe('');
  });
});
