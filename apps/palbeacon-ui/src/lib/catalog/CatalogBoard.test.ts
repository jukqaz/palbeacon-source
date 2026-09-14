import { fireEvent, render, screen, within } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';
import { catalogFixture } from './catalog.fixture';
import CatalogBoard from './CatalogBoard.svelte';
import type { UnifiedCatalog } from './types';

const props = {
  catalog: catalogFixture,
  scope: 'all' as const,
  title: '통합 검색',
};

describe('CatalogBoard', () => {
  it('keeps healthy provenance and developer labels out of the default screen', () => {
    render(CatalogBoard, props);
    expect(screen.queryByText('EXACT DATA')).not.toBeInTheDocument();
    expect(screen.queryByText('데이터 기준')).not.toBeInTheDocument();
    expect(screen.queryByText('설치 게임')).not.toBeInTheDocument();
    expect(screen.queryByText('검증 완료')).not.toBeInTheDocument();
  });

  it('searches original IDs while keeping the visible result human-readable', async () => {
    render(CatalogBoard, props);
    await fireEvent.input(screen.getByRole('searchbox', { name: '통합 검색' }), {
      target: { value: 'UniqueMaterial_FlowerPrince' },
    });

    const result = screen.getByRole('link', {
      name: /부패 독 여과막, 제작 가능, 팰 드롭, 상점 판매, 기술 해금, 아이템, 상세 보기/,
    });
    expect(result).toBeInTheDocument();
    expect(within(result).getByText('획득 3가지')).toBeInTheDocument();
    expect(within(result).getByText('기술 해금')).toBeInTheDocument();
    expect(within(result).queryByText('제작 가능')).not.toBeInTheDocument();
    expect(within(result).queryByText('팰 드롭')).not.toBeInTheDocument();
    expect(within(result).queryByText('상점 판매')).not.toBeInTheDocument();
    expect(within(result).queryByText('UniqueMaterial_FlowerPrince')).not.toBeInTheDocument();
    const href = new URL(result.getAttribute('href')!, 'http://localhost');
    expect(href.pathname).toBe('/items');
    expect(href.searchParams.get('q')).toBe('부패 독 여과막');
    expect(href.searchParams.get('id')).toBe('UniqueMaterial_FlowerPrince');
    expect(
      screen.queryByRole('complementary', { name: '선택한 데이터 상세' }),
    ).not.toBeInTheDocument();
  });

  it('turns exact item relations into concise player actions', async () => {
    render(CatalogBoard, { ...props, scope: 'item', title: '아이템 도감' });
    await fireEvent.click(
      screen.getByRole('button', {
        name: /부패 독 여과막, 제작 가능, 팰 드롭, 상점 판매, 기술 해금, 아이템/,
      }),
    );

    const inspector = screen.getByRole('complementary', { name: '선택한 데이터 상세' });
    expect(within(inspector).getByText('획득 경로')).toBeInTheDocument();
    expect(
      within(inspector).getByText('제작식 1개 · 드롭 팰 1종 · 판매 상점 1곳'),
    ).toBeInTheDocument();
    expect(within(inspector).getByText('해금 조건')).toBeInTheDocument();
    expect(within(inspector).getByText('Lv. 19부터 · 기술 1개')).toBeInTheDocument();
    expect(within(inspector).getByRole('heading', { name: '만드는 법' })).toBeInTheDocument();
    expect(within(inspector).getByRole('heading', { name: '얻는 법' })).toBeInTheDocument();
    expect(within(inspector).getByRole('heading', { name: '구매' })).toBeInTheDocument();
    expect(within(inspector).queryByRole('link', { name: /골드 상점/ })).not.toBeInTheDocument();
    expect(within(inspector).getByText('방폭 섬유')).toBeInTheDocument();
    expect(within(inspector).getByText('가시공주')).toBeInTheDocument();
    expect(within(inspector).getByText('5%')).toBeInTheDocument();
    expect(within(inspector).getByText('1–2개')).toBeInTheDocument();
    expect(within(inspector).getByText('5,000 골드')).toBeInTheDocument();
    const technologyLink = within(inspector).getByRole('link', { name: /배합 목장/ });
    const technologyUrl = new URL(technologyLink.getAttribute('href')!, 'http://localhost');
    expect(technologyUrl.searchParams.get('q')).toBe('배합 목장');
    expect(technologyUrl.searchParams.get('id')).toBe('Technology_Farm');
    const ingredientUrl = new URL(
      within(inspector).getByRole('link', { name: '방폭 섬유' }).getAttribute('href')!,
      'http://localhost',
    );
    expect(ingredientUrl.searchParams.get('q')).toBe('방폭 섬유');
    expect(ingredientUrl.searchParams.get('id')).toBe('UniqueMaterial_Mothman');
    expect(within(inspector).queryByText('pal-drop:Anubis070:1')).not.toBeInTheDocument();
    expect(within(inspector).queryByText('shop:MoneyShop:1')).not.toBeInTheDocument();
  });

  it('opens an exact handoff while keeping the visible query human-readable', async () => {
    const { rerender } = render(CatalogBoard, {
      ...props,
      scope: 'item',
      title: '아이템 도감',
      initialQuery: '부패 독 여과막',
      initialSelectedId: 'UniqueMaterial_FlowerPrince',
    });

    expect(screen.getByRole('searchbox', { name: '아이템 도감 검색' })).toHaveValue(
      '부패 독 여과막',
    );
    expect(
      within(screen.getByRole('complementary', { name: '선택한 데이터 상세' })).getByRole(
        'heading',
        { name: '부패 독 여과막' },
      ),
    ).toBeInTheDocument();

    await rerender({
      ...props,
      scope: 'item',
      title: '아이템 도감',
      initialQuery: '방폭 섬유',
      initialSelectedId: 'UniqueMaterial_Mothman',
    });
    expect(screen.getByRole('searchbox', { name: '아이템 도감 검색' })).toHaveValue('방폭 섬유');
    expect(
      within(screen.getByRole('complementary', { name: '선택한 데이터 상세' })).getByRole(
        'heading',
        { name: '방폭 섬유' },
      ),
    ).toBeInTheDocument();
  });

  it('finds exact raw rarity 5 items through the proven legendary label', async () => {
    render(CatalogBoard, { ...props, scope: 'item' });
    await fireEvent.input(screen.getByRole('searchbox', { name: '통합 검색' }), {
      target: { value: '등급 5' },
    });
    expect(screen.getByText('2개 결과')).toBeInTheDocument();
    expect(screen.getByText('부패 독 여과막')).toBeInTheDocument();
    expect(screen.getByText('방폭 섬유')).toBeInTheDocument();
    expect(screen.getAllByText('전설', { exact: true })).toHaveLength(2);
    expect(screen.queryByText(/등급 5/u)).not.toBeInTheDocument();
  });

  it('recovers from an empty filter state', async () => {
    render(CatalogBoard, { ...props, initialQuery: '존재하지않음' });
    expect(screen.getByText('조건에 맞는 항목이 없습니다.')).toBeInTheDocument();
    await fireEvent.click(screen.getByRole('button', { name: '검색과 필터 초기화' }));
    expect(screen.getByText('배합 목장')).toBeInTheDocument();
  });

  it('groups identical shop inventories and names the matching sold item', async () => {
    const shop = {
      ...catalogFixture.records.items[0]!,
      kind: 'shop' as const,
      id: 'shop-a',
      name_ko: '금화 상점',
      category: '금화',
      description_ko: null,
      image_path: null,
      products: [
        {
          product_id: 'offer-a',
          item_id: 'LambMeat',
          item_name_ko: '도로롱의 양고기',
          quantity: 1,
          price: 1_000,
          product_type: 'item',
          stock: null,
        },
        {
          product_id: 'offer-b',
          item_id: 'Egg',
          item_name_ko: '알',
          quantity: 1,
          price: 50,
          product_type: 'item',
          stock: null,
        },
      ],
    };
    const catalog: UnifiedCatalog = {
      ...catalogFixture,
      records: {
        ...catalogFixture.records,
        shops: [
          shop,
          {
            ...shop,
            id: 'shop-b',
            products: shop.products?.map((product, index) =>
              Object.assign({}, product, { product_id: `duplicate-${index.toString()}` }),
            ),
          },
        ],
      },
    };
    render(CatalogBoard, { ...props, catalog, scope: 'shop', title: '상점 도감' });
    await fireEvent.input(screen.getByRole('searchbox', { name: '상점 도감 검색' }), {
      target: { value: '도로롱' },
    });

    const result = screen.getByRole('button', {
      name: /금화 상점, 도로롱의 양고기 1,000 금화, 알 50 금화, 판매품 2종, 판매 위치 2곳, 상점/u,
    });
    expect(result).toBeVisible();
    expect(within(result).getByText('도로롱의 양고기')).toBeVisible();
    expect(within(result).getByText('판매품 2종')).toBeVisible();
    expect(within(result).getByText('판매 위치 2곳')).toBeVisible();
    expect(within(result).queryByLabelText(/판매품/u)).not.toBeInTheDocument();
    expect(within(result).queryByLabelText(/화폐/u)).not.toBeInTheDocument();
    expect(within(result).queryByText(/판매품 2개 · 결제 화폐/u)).not.toBeInTheDocument();
    expect(within(result).queryByText('상점 · 금화')).not.toBeInTheDocument();
    expect(screen.getAllByRole('button', { name: /금화 상점/u })).toHaveLength(1);
    await fireEvent.click(result);
    const inspector = screen.getByRole('complementary', { name: '선택한 데이터 상세' });
    expect(inspector).toHaveFocus();
    await fireEvent.click(within(inspector).getByRole('button', { name: '상세 닫기' }));
    expect(result).toHaveFocus();
  });

  it('turns exact Pal fields into a compact game-facing compendium', async () => {
    render(CatalogBoard, { ...props, scope: 'pal', title: '팰 도감' });

    const result = screen.getByRole('button', {
      name: /가시공주, No\. 060, 풀, 수작업 레벨 2, 운반 레벨 1, 팰/,
    });
    expect(within(result).getByText('No. 060')).toBeInTheDocument();
    expect(result.textContent?.match(/No\. 060/gu)).toHaveLength(1);
    expect(within(result).getByText('팰', { exact: true })).toBeInTheDocument();
    expect(within(result).getByText('수작업')).toBeInTheDocument();
    expect(within(result).getByText('Lv.2')).toBeInTheDocument();
    expect(within(result).getByLabelText('방어 80')).toBeInTheDocument();

    await fireEvent.click(result);
    const inspector = screen.getByRole('complementary', { name: '선택한 데이터 상세' });
    expect(within(inspector).getByRole('heading', { name: /^작업 적성/ })).toBeInTheDocument();
    expect(within(inspector).getByRole('heading', { name: /^이동·생활/ })).toBeInTheDocument();
    expect(
      within(inspector).getByRole('heading', { name: /^습득 액티브 스킬/ }),
    ).toBeInTheDocument();
    expect(within(inspector).getByRole('heading', { name: /^드롭 아이템/ })).toBeInTheDocument();
    expect(within(inspector).getByRole('link', { name: /부패 독 여과막/ })).toBeInTheDocument();
    expect(within(inspector).getByText('바람의 칼날')).toBeInTheDocument();
    expect(within(inspector).getByText('식사량')).toBeInTheDocument();
    expect(within(inspector).getByText('2단계')).toBeInTheDocument();
    expect(within(inspector).getAllByText(/^상위 \d+%$/).length).toBeGreaterThan(0);
    expect(within(inspector).queryByText('WindCutter')).not.toBeInTheDocument();
  });

  it('hides a failed Pal image and keeps work data readable without diagnostics', async () => {
    render(CatalogBoard, { ...props, scope: 'pal', title: '팰 도감' });

    const result = screen.getByRole('button', { name: /가시공주/ });
    const palImage = result.querySelector<HTMLImageElement>(
      'img[src="/generated/game/pals/T_LittleBriarRose_icon_normal.webp"]',
    );
    expect(palImage).not.toBeNull();
    await fireEvent.error(palImage!);
    expect(within(result).queryByText('이미지 미확인')).not.toBeInTheDocument();
    expect(result.querySelector('.card-media')).not.toBeInTheDocument();

    const workImage = result.querySelector<HTMLImageElement>(
      'img[src="/generated/game/work-suitability/Handcraft.png"]',
    );
    expect(workImage).not.toBeNull();
    await fireEvent.error(workImage!);
    expect(
      result.querySelector('img[src="/generated/game/work-suitability/Handcraft.png"]'),
    ).not.toBeInTheDocument();
    expect(within(result).getByText('수작업')).toBeInTheDocument();
    expect(within(result).getByText('Lv.2')).toBeInTheDocument();
  });

  it('shows the exact Pals that learn an active skill without exposing identifiers', async () => {
    render(CatalogBoard, { ...props, scope: 'active_skill', title: '액티브 스킬' });
    await fireEvent.click(screen.getByRole('button', { name: /바람의 칼날/ }));

    const inspector = screen.getByRole('complementary', { name: '선택한 데이터 상세' });
    expect(within(inspector).getByRole('heading', { name: /^습득 팰/ })).toBeInTheDocument();
    const palLink = within(inspector).getByRole('link', { name: /가시공주/ });
    expect(palLink).toBeInTheDocument();
    expect(within(inspector).queryByText('LittleBriarRose')).not.toBeInTheDocument();
    const href = new URL(palLink.getAttribute('href')!, 'http://localhost');
    expect(href.searchParams.get('q')).toBe('가시공주');
    expect(href.searchParams.get('id')).toBe('LittleBriarRose');
  });

  it('searches Pal work suitability and learned skill names', async () => {
    render(CatalogBoard, { ...props, scope: 'pal', title: '팰 도감' });
    const search = screen.getByRole('searchbox', { name: '팰 도감 검색' });

    await fireEvent.input(search, { target: { value: '수작업' } });
    expect(screen.getByText('1개 결과')).toBeInTheDocument();
    expect(screen.getByText('가시공주')).toBeInTheDocument();

    await fireEvent.input(search, { target: { value: '바람의 칼날' } });
    expect(screen.getByText('1개 결과')).toBeInTheDocument();
  });

  it('keeps long mobile skill lists compact until the user expands them', async () => {
    const pal = catalogFixture.records.pals[0]!;
    const baseSkill = pal.pal_skills![0]!;
    const manySkills: UnifiedCatalog = {
      ...catalogFixture,
      records: {
        ...catalogFixture.records,
        pals: [
          {
            ...pal,
            pal_skills: Array.from({ length: 6 }, (_, index) => ({
              ...baseSkill,
              id: `${baseSkill.id}_${index.toString()}`,
              name_ko: `${baseSkill.name_ko} ${index + 1}`,
            })),
          },
        ],
      },
    };

    const { container } = render(CatalogBoard, {
      ...props,
      catalog: manySkills,
      scope: 'pal',
      title: '팰 도감',
    });
    await fireEvent.click(screen.getByRole('button', { name: /가시공주/ }));
    expect(container.querySelectorAll('.skill-list li')).toHaveLength(4);
    await fireEvent.click(screen.getByRole('button', { name: '나머지 2개 보기' }));
    expect(container.querySelectorAll('.skill-list li')).toHaveLength(6);
    await fireEvent.click(screen.getByRole('button', { name: '스킬 접기' }));
    expect(container.querySelectorAll('.skill-list li')).toHaveLength(4);
  });

  it('hides unsupported movement sentinels and internal-only metrics', async () => {
    const pal = catalogFixture.records.pals[0]!;
    const technology = catalogFixture.records.technologies[0]!;
    const unsupported: UnifiedCatalog = {
      ...catalogFixture,
      records: {
        ...catalogFixture.records,
        pals: [
          {
            ...pal,
            pal_profile: {
              ...pal.pal_profile!,
              run_speed: -1,
              ride_sprint_speed: -1,
              transport_speed: -1,
            },
          },
        ],
        technologies: [
          {
            ...technology,
            metrics: [
              { label: '레벨', value: '10' },
              { label: '티어', value: '0' },
            ],
          },
        ],
      },
    };

    const { unmount } = render(CatalogBoard, {
      ...props,
      catalog: unsupported,
      scope: 'pal',
      title: '팰 도감',
    });
    await fireEvent.click(screen.getByRole('button', { name: /가시공주/ }));
    const palInspector = screen.getByRole('complementary', { name: '선택한 데이터 상세' });
    expect(within(palInspector).queryByText('-1')).not.toBeInTheDocument();
    expect(within(palInspector).queryByText('탑승 질주')).not.toBeInTheDocument();
    expect(within(palInspector).queryByText('운반 속도')).not.toBeInTheDocument();
    unmount();

    render(CatalogBoard, { ...props, catalog: unsupported, scope: 'technology' });
    await fireEvent.click(screen.getByRole('button', { name: /배합 목장/ }));
    const technologyInspector = screen.getByRole('complementary', {
      name: '선택한 데이터 상세',
    });
    expect(within(technologyInspector).queryByText('티어')).not.toBeInTheDocument();
    expect(within(technologyInspector).queryByText('0')).not.toBeInTheDocument();
  });

  it('hides unknown Pal attributes instead of labeling them as unverified', async () => {
    const pal = catalogFixture.records.pals[0]!;
    const palWithoutUnknowns = { ...pal };
    delete palWithoutUnknowns.elements;
    delete palWithoutUnknowns.pal_profile;
    const withoutUnknowns: UnifiedCatalog = {
      ...catalogFixture,
      records: {
        ...catalogFixture.records,
        pals: [palWithoutUnknowns],
      },
    };

    render(CatalogBoard, {
      ...props,
      catalog: withoutUnknowns,
      scope: 'pal',
      title: '팰 도감',
    });
    await fireEvent.click(screen.getByRole('button', { name: /가시공주/ }));
    const inspector = screen.getByRole('complementary', { name: '선택한 데이터 상세' });
    expect(within(inspector).queryByText('속성 미확인')).not.toBeInTheDocument();
    expect(within(inspector).queryByText('주행성')).not.toBeInTheDocument();
  });

  it('hides unverified media instead of rendering a fake thumbnail', () => {
    const passive = {
      ...catalogFixture,
      statistics: { ...catalogFixture.statistics, passive_skills: 1 },
      records: {
        ...catalogFixture.records,
        passive_skills: [
          {
            kind: 'passive_skill' as const,
            id: 'PAL_FullStomach_Down_1',
            name_ko: '소식',
            description_ko: '포만도 쉽게 내려가지 않음 +10%',
            image_path: null,
            category: '강화 효과',
            tags: ['강화 효과'],
            metrics: [{ label: '효과 등급', value: '+1' }],
            search_terms: ['등급 1'],
            localization_fallback: false,
          },
        ],
      },
    };

    render(CatalogBoard, { ...props, catalog: passive, scope: 'passive_skill' });
    expect(screen.getByText('포만도 쉽게 내려가지 않음 +10%')).toBeInTheDocument();
    expect(document.querySelector('.image-fallback')).not.toBeInTheDocument();
  });
});
