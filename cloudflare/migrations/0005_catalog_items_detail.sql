-- Exact-build item details extracted from the user's local Palworld install.
-- Dataset publication still requires the existing verified + activated gate.
ALTER TABLE catalog_items ADD COLUMN subcategory TEXT;
ALTER TABLE catalog_items ADD COLUMN weight_milli INTEGER NOT NULL DEFAULT 0;
ALTER TABLE catalog_items ADD COLUMN maximum_stack_count INTEGER NOT NULL DEFAULT 0;
ALTER TABLE catalog_items ADD COLUMN rarity INTEGER NOT NULL DEFAULT 0;
ALTER TABLE catalog_items ADD COLUMN rank INTEGER NOT NULL DEFAULT 0;
ALTER TABLE catalog_items ADD COLUMN icon_name TEXT;
ALTER TABLE catalog_items ADD COLUMN legal_in_game INTEGER NOT NULL DEFAULT 0;
ALTER TABLE catalog_items ADD COLUMN localization_fallback INTEGER NOT NULL DEFAULT 0;
ALTER TABLE catalog_items ADD COLUMN source_ref TEXT;

CREATE INDEX idx_catalog_items_name_ko
ON catalog_items(dataset_version, name_ko);

CREATE INDEX idx_catalog_items_category
ON catalog_items(dataset_version, category, subcategory);

CREATE TABLE catalog_recipes (
    dataset_version TEXT NOT NULL
        REFERENCES data_versions(dataset_version) ON DELETE CASCADE,
    recipe_id TEXT NOT NULL,
    output_item_id TEXT NOT NULL,
    output_quantity INTEGER NOT NULL,
    ingredients_json TEXT NOT NULL DEFAULT '[]',
    work_amount INTEGER NOT NULL DEFAULT 0,
    unlock_item_id TEXT,
    source_ref TEXT,
    PRIMARY KEY (dataset_version, recipe_id)
);

CREATE INDEX idx_catalog_recipes_output
ON catalog_recipes(dataset_version, output_item_id);

CREATE INDEX idx_acquisition_kind
ON acquisition_methods(dataset_version, method_kind, item_id);
