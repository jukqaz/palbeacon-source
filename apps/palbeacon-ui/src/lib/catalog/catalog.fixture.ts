import type { CatalogRecord, UnifiedCatalog } from './types';

const record = (
  kind: CatalogRecord['kind'],
  id: string,
  name: string,
  category: string,
  overrides: Partial<CatalogRecord> = {},
): CatalogRecord => ({
  kind,
  id,
  name_ko: name,
  description_ko: `${name} 설명`,
  image_path: null,
  category,
  tags: [category],
  metrics: [{ label: '분류', value: category }],
  search_terms: [],
  localization_fallback: false,
  ...overrides,
});

export const catalogFixture: UnifiedCatalog = {
  schema_version: 1,
  game_build_id: 'steam:24575825',
  mapping_sha256: 'verified-fixture',
  verified: true,
  statistics: {
    technologies: 1,
    pals: 1,
    items: 2,
    active_skills: 1,
    passive_skills: 0,
    buildings: 0,
    shops: 0,
  },
  records: {
    technologies: [record('technology', 'Technology_Farm', '배합 목장', '일반 기술')],
    pals: [
      record('pal', 'LittleBriarRose', '가시공주', '풀', {
        image_path: '/generated/game/pals/T_LittleBriarRose_icon_normal.webp',
        metrics: [
          { label: 'HP', value: '80' },
          { label: '공격', value: '80' },
          { label: '방어', value: '80' },
        ],
        search_terms: ['Bristla', '60', '도감 60', '수작업', '바람의 칼날'],
        element_id: 'Leaf',
        element_icon_path: '/generated/game/elements/T_Icon_element_s_04.webp',
        elements: [
          {
            id: 'Leaf',
            name_ko: '풀',
            icon_path: '/generated/game/elements/T_Icon_element_s_04.webp',
          },
        ],
        work_suitability: [
          {
            id: 'Handcraft',
            name_ko: '수작업',
            level: 2,
            icon_path: '/generated/game/work-suitability/Handcraft.png',
          },
          {
            id: 'Transport',
            name_ko: '운반',
            level: 1,
            icon_path: '/generated/game/work-suitability/Transport.png',
          },
        ],
        pal_profile: {
          paldex_number: 60,
          paldex_suffix: '',
          walk_speed: 100,
          run_speed: 400,
          ride_sprint_speed: 550,
          transport_speed: 250,
          stamina: 100,
          food_amount: 2,
          nocturnal: false,
        },
        pal_skills: [
          {
            id: 'WindCutter',
            name_ko: '바람의 칼날',
            level: 1,
            element_id: 'Leaf',
            element_name_ko: '풀',
            icon_path: '/generated/game/elements/T_prt_pal_skill_base_element_04.webp',
            power: 40,
            cooldown_seconds: 2,
          },
        ],
        pal_drops: [
          {
            item_id: 'UniqueMaterial_FlowerPrince',
            name_ko: '부패 독 여과막',
            image_path: null,
            rarity: 5,
            variant: 'boss',
            minimum_quantity: 1,
            maximum_quantity: 2,
            probability_ppm: 50_000,
          },
        ],
      }),
    ],
    items: [
      record('item', 'UniqueMaterial_FlowerPrince', '부패 독 여과막', 'Material', {
        rarity: 5,
        tags: ['Material', '전설'],
        metrics: [{ label: '등급', value: '전설 · 등급 5' }],
        search_terms: ['전설', '등급 5'],
      }),
      record('item', 'UniqueMaterial_Mothman', '방폭 섬유', 'Material', {
        rarity: 5,
        tags: ['Material', '전설'],
        metrics: [{ label: '등급', value: '전설 · 등급 5' }],
        search_terms: ['전설', '등급 5'],
      }),
    ],
    active_skills: [
      record('active_skill', 'WindCutter', '바람의 칼날', '풀', {
        learned_by_pals: [
          {
            id: 'LittleBriarRose',
            name_ko: '가시공주',
            image_path: '/generated/game/pals/T_LittleBriarRose_icon_normal.webp',
            paldex_number: 60,
          },
        ],
      }),
    ],
    passive_skills: [],
    buildings: [],
    shops: [],
  },
  item_relations: {
    UniqueMaterial_FlowerPrince: {
      recipes: [
        {
          recipe_id: 'UniqueMaterial_FlowerPrince',
          output_quantity: 1,
          work_amount: 1200,
          unlock_item_id: null,
          ingredients: [
            { item_id: 'UniqueMaterial_Mothman', name_ko: '방폭 섬유', quantity: 2 },
            { item_id: 'UniqueMaterial_FlowerPrince', name_ko: '부패 독 여과막', quantity: 1 },
          ],
        },
      ],
      drops: [
        {
          method_id: 'pal-drop:Anubis070:1',
          pal_id: 'LittleBriarRose',
          level: 70,
          variant: 'boss',
          minimum_quantity: 1,
          maximum_quantity: 2,
          probability_ppm: 50_000,
        },
      ],
      shops: [
        {
          product_id: 'shop:MoneyShop:1',
          shop_group_id: 'MoneyShop',
          currency_item_id: 'Money',
          currency_name_ko: '골드',
          quantity: 1,
          price: 5000,
          stock: null,
        },
      ],
      technologies: [{ technology_id: 'Technology_Farm', level: 19, cost: 2 }],
    },
  },
};
