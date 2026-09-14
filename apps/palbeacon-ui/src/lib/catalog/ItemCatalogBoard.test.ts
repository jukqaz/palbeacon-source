import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';
import { catalogFixture } from './catalog.fixture';
import ItemCatalogBoard from './ItemCatalogBoard.svelte';
import type { UnifiedCatalog } from './types';

const duplicateRelationCatalog = (): UnifiedCatalog => {
  const relations = catalogFixture.item_relations?.['UniqueMaterial_FlowerPrince'];
  if (!relations) throw new Error('Item relation fixture is missing');
  return {
    ...catalogFixture,
    item_relations: {
      UniqueMaterial_FlowerPrince: {
        recipes: [...relations.recipes, ...relations.recipes],
        drops: [
          ...relations.drops,
          { ...relations.drops[0]!, method_id: 'pal-drop:Anubis080:9', level: 80 },
        ],
        shops: [
          ...relations.shops,
          {
            ...relations.shops[0]!,
            product_id: 'shop:AnotherMoneyShop:4',
            shop_group_id: 'AnotherMoneyShop',
          },
        ],
        technologies: [...relations.technologies, ...relations.technologies],
      },
    },
  };
};

describe('ItemCatalogBoard', () => {
  it('uses the graphical inventory view by default and preserves the result when comparing rows', async () => {
    render(ItemCatalogBoard, { catalog: catalogFixture });

    const gridToggle = screen.getByRole('button', { name: '그리드 보기' });
    const listToggle = screen.getByRole('button', { name: '목록 보기' });
    expect(gridToggle).toHaveAttribute('aria-pressed', 'true');
    expect(document.querySelector('.item-card')).not.toBeNull();
    expect(document.querySelector('.item-row')).toBeNull();

    await fireEvent.input(screen.getByRole('searchbox', { name: '아이템 도감 검색' }), {
      target: { value: '부패 독 여과막' },
    });
    const selected = screen.getByRole('button', { name: /부패 독 여과막.*아이템/ });
    await fireEvent.click(selected);
    await fireEvent.click(listToggle);

    expect(listToggle).toHaveAttribute('aria-pressed', 'true');
    expect(screen.getByRole('searchbox', { name: '아이템 도감 검색' })).toHaveValue(
      '부패 독 여과막',
    );
    expect(screen.getByRole('button', { name: /부패 독 여과막.*아이템/ })).toHaveAttribute(
      'aria-pressed',
      'true',
    );
    expect(document.querySelector('.item-row')).not.toBeNull();
    expect(document.querySelector('.item-card')).toBeNull();
  });

  it('searches internal identifiers while rendering only Korean item information', async () => {
    render(ItemCatalogBoard, { catalog: catalogFixture });
    await fireEvent.input(screen.getByRole('searchbox', { name: '아이템 도감 검색' }), {
      target: { value: 'UniqueMaterial_FlowerPrince' },
    });

    expect(screen.getByRole('button', { name: /부패 독 여과막.*아이템/ })).toBeInTheDocument();
    expect(screen.queryByText('UniqueMaterial_FlowerPrince')).not.toBeInTheDocument();
    expect(screen.queryByText('등급 5')).not.toBeInTheDocument();
    expect(screen.getAllByText('전설').length).toBeGreaterThan(0);
  });

  it('combines category, rarity, and acquisition filters over exact data', async () => {
    render(ItemCatalogBoard, { catalog: catalogFixture });

    await fireEvent.click(screen.getByRole('checkbox', { name: '전설' }));
    await fireEvent.click(screen.getByRole('checkbox', { name: '제작' }));

    expect(screen.getByRole('button', { name: /부패 독 여과막.*아이템/ })).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /방폭 섬유.*아이템/ })).not.toBeInTheDocument();
    const heading = screen.getByRole('heading', { name: '아이템 도감' });
    expect(within(heading.closest('header')!).getByText('1개')).toBeInTheDocument();
  });

  it('keeps overview, recipe, drops, shops, and technology in focused detail tabs', async () => {
    render(ItemCatalogBoard, { catalog: duplicateRelationCatalog() });
    await fireEvent.click(screen.getByRole('button', { name: /부패 독 여과막.*아이템/ }));

    const inspector = screen.getByRole('complementary', { name: '선택한 아이템 상세' });
    expect(within(inspector).getByRole('heading', { name: '부패 독 여과막' })).toBeInTheDocument();
    expect(within(inspector).getByText('전설')).toBeInTheDocument();
    expect(within(inspector).queryByText('등급 5')).not.toBeInTheDocument();

    await fireEvent.click(within(inspector).getByRole('tab', { name: '제작 1' }));
    expect(within(inspector).getByRole('heading', { name: '만드는 법' })).toBeInTheDocument();
    expect(within(inspector).getByRole('link', { name: /방폭 섬유/ })).toBeInTheDocument();

    await fireEvent.click(within(inspector).getByRole('tab', { name: '드롭 1' }));
    expect(within(inspector).getAllByRole('link', { name: /가시공주/ })).toHaveLength(1);
    expect(within(inspector).getByText('5% · 1–2개')).toBeInTheDocument();

    await fireEvent.click(within(inspector).getByRole('tab', { name: '상점 1' }));
    expect(within(inspector).getByText('골드 상점 2곳')).toBeInTheDocument();
    expect(within(inspector).getByText('5,000 골드')).toBeInTheDocument();

    await fireEvent.click(within(inspector).getByRole('tab', { name: '기술 1' }));
    const technology = within(inspector).getByRole('link', { name: /배합 목장/ });
    expect(technology).toBeInTheDocument();
    const href = new URL(technology.getAttribute('href')!, 'http://localhost');
    expect(href.searchParams.get('q')).toBe('배합 목장');
    expect(href.searchParams.get('id')).toBe('Technology_Farm');
  });

  it('hides unsafe description fragments without replacing them with diagnostics', async () => {
    const unsafe: UnifiedCatalog = {
      ...catalogFixture,
      records: {
        ...catalogFixture.records,
        items: catalogFixture.records.items.map((item, index) =>
          index === 0
            ? {
                ...item,
                description_ko: '복잡한 판단에 사용하는 부품. Factory_Hard_04 에서 제작할 수 있다.',
              }
            : item,
        ),
      },
    };
    render(ItemCatalogBoard, { catalog: unsafe });

    expect(screen.queryByText('복잡한 판단에 사용하는 부품.')).not.toBeInTheDocument();
    await fireEvent.click(screen.getByRole('button', { name: /부패 독 여과막.*아이템/ }));
    expect(screen.getByText('복잡한 판단에 사용하는 부품.')).toBeInTheDocument();
    expect(screen.queryByText(/Factory_Hard_04/u)).not.toBeInTheDocument();
    expect(screen.queryByText('미확인')).not.toBeInTheDocument();
  });

  it('returns focus to the selected row after closing the narrow detail', async () => {
    render(ItemCatalogBoard, { catalog: catalogFixture });
    const result = screen.getByRole('button', { name: /부패 독 여과막.*아이템/ });
    await fireEvent.click(result);
    const back = document.querySelector<HTMLButtonElement>('.mobile-back');
    expect(back).not.toBeNull();
    await fireEvent.click(back!);
    await waitFor(() => expect(result).toHaveFocus());
  });

  it('recovers from an empty item filter state', async () => {
    render(ItemCatalogBoard, { catalog: catalogFixture, initialQuery: '존재하지않음' });
    expect(screen.getByText('조건에 맞는 아이템이 없습니다.')).toBeInTheDocument();
    await fireEvent.click(screen.getByRole('button', { name: '검색과 필터 초기화' }));
    expect(screen.getByRole('button', { name: /부패 독 여과막.*아이템/ })).toBeInTheDocument();
  });
});
