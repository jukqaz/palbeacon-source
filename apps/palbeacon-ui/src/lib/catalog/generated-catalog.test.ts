import { access, readFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';
import type { UnifiedCatalog } from './types';

interface CatalogSummary {
  schema_version: number;
  statistics: UnifiedCatalog['statistics'];
}

const loadGeneratedCatalog = async (): Promise<UnifiedCatalog> =>
  JSON.parse(
    await readFile(resolve(process.cwd(), 'static/generated/game/catalog.v1.json'), 'utf8'),
  ) as UnifiedCatalog;

describe('generated exact-build catalog presentation', () => {
  it('keeps home counts derived from the generated catalog statistics', async () => {
    const catalog = await loadGeneratedCatalog();
    const summary = JSON.parse(
      await readFile(resolve(process.cwd(), 'src/lib/generated/catalog-summary.json'), 'utf8'),
    ) as CatalogSummary;

    expect(summary.schema_version).toBe(catalog.schema_version);
    expect(summary.statistics).toEqual(catalog.statistics);
  });

  it('projects Korean labels and strips game-only markup from visible descriptions', async () => {
    const catalog = await loadGeneratedCatalog();
    expect(catalog.game_build_id).toBe('steam:24575825');
    expect(catalog.records.pals.some((record) => record.category === '풀')).toBe(true);
    expect(catalog.records.items.some((record) => record.category === '재료')).toBe(true);
    expect(
      Object.values(catalog.records)
        .flat()
        .filter((record) => record.description_ko)
        .some((record) => /<[^>]+>|\{EffectValue/.test(record.description_ko ?? '')),
    ).toBe(false);
    expect(
      Object.values(catalog.records)
        .flat()
        .flatMap((record) => record.metrics)
        .some((metric) => ['티어', '랭크', '효과 등급'].includes(metric.label)),
    ).toBe(false);
    expect(
      Object.values(catalog.records)
        .flat()
        .some((record) => /[+-]?\d+\.0+%/.test(record.description_ko ?? '')),
    ).toBe(false);
    expect(
      catalog.records.items
        .flatMap((record) => record.metrics)
        .filter((metric) => metric.label === '무게')
        .every((metric) => metric.value.endsWith(' kg')),
    ).toBe(true);
  });

  it('uses exact resources for elements, building materials, and shop products', async () => {
    const catalog = await loadGeneratedCatalog();
    const active = catalog.records.active_skills.find((record) => record.element_id === 'Leaf');
    expect(active?.element_icon_path).toMatch(/T_Icon_element_s_04\.webp$/);
    expect(active?.image_path).toMatch(/T_prt_pal_skill_base_element_04\.webp$/);

    const building = catalog.records.buildings.find((record) => record.materials?.length);
    expect(building?.materials?.some((material) => material.image_path)).toBe(true);
    expect(building?.building_ui_category).toEqual(expect.any(String));
    expect(building?.building_subcategory).toEqual(expect.any(String));
    expect(building?.building_sort_order).toEqual(expect.any(Number));

    const shop = catalog.records.shops.find((record) => record.products?.length);
    expect(shop?.preview_images?.length).toBeGreaterThan(0);
    expect(shop?.products?.some((product) => product.image_path)).toBe(true);
  });

  it('projects exact item recipes, Pal drops, shops, and technology relations', async () => {
    const catalog = await loadGeneratedCatalog();
    const relations = Object.values(catalog.item_relations ?? {});

    expect(relations.filter((relation) => relation.recipes.length > 0)).toHaveLength(1273);
    expect(relations.filter((relation) => relation.drops.length > 0)).toHaveLength(149);
    expect(relations.filter((relation) => relation.shops.length > 0)).toHaveLength(266);
    expect(relations.filter((relation) => relation.technologies.length > 0)).toHaveLength(380);

    const aiCore = catalog.item_relations?.['AIcore'];
    expect(aiCore?.recipes[0]).toMatchObject({
      output_quantity: 1,
      ingredients: expect.arrayContaining([
        expect.objectContaining({ item_id: 'Computer', name_ko: '컴퓨터', quantity: 5 }),
      ]),
    });
    expect(aiCore?.technologies).toEqual(
      expect.arrayContaining([expect.objectContaining({ technology_id: 'AIcore', level: 67 })]),
    );

    const technologyBook = catalog.item_relations?.['TechnologyBook_G2'];
    expect(technologyBook?.drops).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          pal_id: 'Anubis',
          minimum_quantity: 1,
          maximum_quantity: 1,
          probability_ppm: 50_000,
        }),
      ]),
    );

    const palIds = new Set(catalog.records.pals.map((pal) => pal.id));
    for (const relation of relations) {
      for (const drop of relation.drops) expect(palIds.has(drop.pal_id)).toBe(true);
      for (const recipe of relation.recipes) {
        for (const ingredient of recipe.ingredients) expect(ingredient.name_ko.trim()).not.toBe('');
      }
    }
  });

  it('projects human-readable Pal identity, work, movement, and skill data', async () => {
    const catalog = await loadGeneratedCatalog();
    const bristla = catalog.records.pals.find((record) => record.id === 'LittleBriarRose');

    expect(bristla?.pal_profile).toMatchObject({
      paldex_number: 60,
      run_speed: 400,
      ride_sprint_speed: 550,
      transport_speed: 250,
      stamina: 100,
      food_amount: 2,
      nocturnal: false,
    });
    expect(bristla?.elements).toEqual([expect.objectContaining({ id: 'Leaf', name_ko: '풀' })]);
    expect(bristla?.work_suitability).toEqual(
      expect.arrayContaining([
        expect.objectContaining({ id: 'Handcraft', name_ko: '수작업', level: 2 }),
        expect.objectContaining({ id: 'Transport', name_ko: '운반', level: 1 }),
      ]),
    );
    expect(bristla?.pal_skills).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          id: 'WindCutter',
          name_ko: '바람의 칼날',
          level: 1,
          power: 40,
          cooldown_seconds: 2,
        }),
      ]),
    );
    expect(bristla?.search_terms).toEqual(
      expect.arrayContaining(['도감 60', '수작업', '바람의 칼날']),
    );

    await Promise.all(
      (bristla?.work_suitability ?? []).map((work) =>
        access(resolve(process.cwd(), 'static', work.icon_path.replace(/^\//, ''))),
      ),
    );
  });

  it('keeps every exact Pal profile and referenced game resource loadable', async () => {
    const catalog = await loadGeneratedCatalog();
    const assetPaths = new Set<string>();

    expect(catalog.records.pals).toHaveLength(288);
    for (const pal of catalog.records.pals) {
      expect(pal.pal_profile).toBeDefined();
      expect(pal.metrics.map((metric) => metric.label)).toEqual(['HP', '공격', '방어']);

      if (pal.image_path) assetPaths.add(pal.image_path);
      for (const element of pal.elements ?? []) assetPaths.add(element.icon_path);
      for (const work of pal.work_suitability ?? []) assetPaths.add(work.icon_path);
      for (const skill of pal.pal_skills ?? []) assetPaths.add(skill.icon_path);
    }

    const unresolvedElement = catalog.records.pals.find((pal) => pal.id === 'WorldTreeDragon');
    expect(unresolvedElement).toMatchObject({
      name_ko: '제로버스',
      category: '속성 미확인',
      elements: [],
    });
    expect(unresolvedElement?.element_icon_path).toBeUndefined();

    expect(assetPaths.size).toBeGreaterThan(300);
    await Promise.all(
      [...assetPaths].map((assetPath) =>
        access(resolve(process.cwd(), 'static', assetPath.replace(/^\//, ''))),
      ),
    );
  });
});
