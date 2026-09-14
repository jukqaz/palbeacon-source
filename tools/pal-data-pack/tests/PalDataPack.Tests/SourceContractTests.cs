namespace PalDataPack.Tests;

public sealed class SourceContractTests
{
    [Fact]
    public void ReviewedGameIconContractAcceptsOnlyTheReviewedInventory()
    {
        var contract = GameIconContract.Load(Path.Combine(
            TestPaths.RepositoryRoot(),
            "tools",
            "pal-data-pack",
            "contracts",
            "24575825.game-icon-contract.json"));
        var probe = new GameIconProbeResult(
            "24575825",
            "241c45de9d5b55b246cd4b39d62b9209faf7758ce0637e1f7a545aa0f75f71f0",
            contract.Groups
                .Select(group => new GameIconGroupProbe(
                    group.GroupId,
                    group.AssetRoot,
                    group.FileNamePrefix,
                    group.FileNameSuffix,
                    group.ExpectedTextureCount,
                    group.InventorySha256,
                    []))
                .ToArray());

        contract.RequireProbe(probe);

        var changedGroups = probe.Groups
            .Select((group, index) => index == 0
                ? group with { TextureCount = group.TextureCount - 1 }
                : group)
            .ToArray();
        Assert.Throws<DataPackFailure>(() =>
            contract.RequireProbe(probe with { Groups = changedGroups }));
    }

    [Fact]
    public void GameIconContractRejectsPublicRedistributionScope()
    {
        var path = Path.Combine(
            TestPaths.RepositoryRoot(),
            "tools",
            "pal-data-pack",
            "contracts",
            "24181527.game-icon-contract.json");
        var json = File.ReadAllText(path).Replace(
            GameIconContract.LocalDistributionScope,
            "public_web_approved",
            StringComparison.Ordinal);

        Assert.Throws<DataPackFailure>(() => GameIconContract.Parse(json));
    }

    [Fact]
    public void GameIconInventorySelectorsDoNotAdmitUnrelatedTextures()
    {
        var definition = GameIconProbe.Definitions.Single(value =>
            value.GroupId == "pal_portrait");
        var selected = GameIconProbe.SelectPackagePaths(
            [
                "Pal/Content/Pal/Texture/PalIcon/Normal/T_Alpaca_icon_normal.uasset",
                "Pal/Content/Pal/Texture/PalIcon/Normal/T_Alpaca_icon_normal.uexp",
                "Pal/Content/Pal/Texture/PalIcon/Normal/T_Alpaca_icon_normal_roughness.uasset",
                "Pal/Content/Pal/Texture/PalIcon/Boss/T_Alpaca_icon_normal.uasset",
            ],
            definition);

        var path = Assert.Single(selected);
        Assert.Equal(
            "Pal/Content/Pal/Texture/PalIcon/Normal/T_Alpaca_icon_normal",
            path);
        Assert.Equal("Alpaca", GameIconProbe.LogicalId(path, definition));
        Assert.Equal(
            "049c280443959ddb12d77acb577d986e5e3afd48d01372f7df6c85692339a2c4",
            GameIconProbe.InventorySha256(selected));
    }

    [Fact]
    public void GameIconMatcherUsesKoreanIdsAndSeparatesEveryMismatch()
    {
        var repository = TestPaths.RepositoryRoot();
        var contract = GameIconContract.Load(Path.Combine(
            repository,
            "tools",
            "pal-data-pack",
            "contracts",
            "24575825.game-icon-contract.json"));
        var output = Path.Combine(
            TestPaths.TempDirectory(),
            "game-icon-match.json");
        try
        {
            var result = GameIconLocalizationMatcher.BuildToFile(
                Path.Combine(
                    repository,
                    "docs",
                    "data",
                    "GAME_ICON_MANIFEST.24575825.json"),
                Path.Combine(
                    repository,
                    "assets",
            "palbeacon",
                    "game",
                    "catalog",
                    "l10n",
                    "ko",
                    "pals.json"),
                Path.Combine(
                    repository,
                    "docs",
                    "data",
                    "PAL_ICON_SOURCE_LINKS.24575825.json"),
                contract,
                output);

            Assert.Equal(308, result.EligiblePalIconCount);
            Assert.Equal(2, result.KoreanLocalizationWithoutIconCount);
            Assert.Equal(123, result.IconWithoutKoreanLocalizationCount);
            var report = System.Text.Json.JsonSerializer.Deserialize<
                    GameIconLocalizationMatchReport>(
                    File.ReadAllBytes(output),
                    new System.Text.Json.JsonSerializerOptions
                    {
                        PropertyNamingPolicy =
                            System.Text.Json.JsonNamingPolicy.SnakeCaseLower,
                    })
                ?? throw new InvalidOperationException("report was empty");
            Assert.Equal("ko", report.PrimaryLocale);
            Assert.Equal(288, report.ExactMatchCount);
            Assert.Equal(12, report.CaseOnlyMatchCount);
            Assert.Equal(0, report.ParameterRowMatchCount);
            Assert.Equal(8, report.LocalizedNameBridgeMatchCount);
            Assert.Equal(1, report.MultiIconGroupMatchCount);
            Assert.Equal(2, report.MultiIconGroupMemberCount);
            Assert.Contains(
                report.EligiblePalIcons,
                icon => icon.InternalId == "Alpaca"
                    && icon.KoreanName == "멜파카"
                    && icon.MatchKind == "direct_table_exact");
            Assert.Contains(
                report.EligiblePalIcons,
                icon => icon.InternalId == "GrassBoss"
                    && icon.IconLogicalId == "Human_GrassBoss"
                    && icon.MatchKind == "direct_table_exact");
            Assert.Contains(
                report.EligiblePalIcons,
                icon => icon.InternalId == "DessertBoss"
                    && icon.IconLogicalId == "Horus"
                    && icon.MatchKind == "localized_name_to_tribe");
            Assert.Contains(
                report.KoreanLocalizationsWithoutIcon,
                value => value.InternalId == "RAID_YakushimaBoss002"
                    && value.Reason
                        == "parameter_tribe_without_icon_table_entry");
            Assert.Contains(
                report.MultiIconGroups,
                value => value.InternalId == "POLICE_PalRide"
                    && value.MatchKind
                        == "shared_localization_multi_entity_group"
                    && value.Members.Select(member => member.SourceRowKey)
                        .SequenceEqual(
                            ["POLICE_HawkBird", "POLICE_ThunderDog"])
                    && value.Members.Select(member => member.IconLogicalId)
                        .SequenceEqual(["HawkBird", "ThunderDog"]));
            Assert.Contains(
                report.IconsWithoutKoreanLocalization,
                value => value.IconLogicalId == "CommonHuman");
        }
        finally
        {
            Directory.Delete(Path.GetDirectoryName(output)!, recursive: true);
        }
    }

    [Fact]
    public void PalIconTablePathsNormalizeWithoutGuessingNames()
    {
        Assert.Equal(
            "Pal/Content/Pal/Texture/PalIcon/Normal/T_Horus_icon_normal",
            PalIconSourceLinkExtractor.NormalizeIconPath(
                "/Game/Pal/Texture/PalIcon/Normal/T_Horus_icon_normal"
                + ".T_Horus_icon_normal"));
    }

    [Fact]
    public void SupplementalPalIconProbePinsEveryNegativeEvidenceTable()
    {
        var contract = AssetContract.Load(Path.Combine(
            TestPaths.RepositoryRoot(),
            "tools",
            "pal-data-pack",
            "contracts",
            "24181527.pal-icon-supplemental-probe-contract.json"));

        Assert.True(contract.Reviewed);
        Assert.Equal(
            "review:pal-icon-supplemental-probe:24181527:2026-07-28",
            contract.ReviewId);
        Assert.Equal(6, contract.Tables.Count);
        Assert.Equal(
            [674, 674, 25, 25, 11, 11],
            contract.Tables.Select(table => table.ExpectedRowCount).ToArray());
    }

    [Fact]
    public void ReviewedItemIconContractAcceptsOnlyTheReviewedProbe()
    {
        var contract = ItemIconContract.Load(Path.Combine(
            TestPaths.RepositoryRoot(),
            "tools",
            "pal-data-pack",
            "contracts",
            "24181527.item-icon-contract.json"));
        var probe = new ItemIconProbeResult(
            "24181527",
            "241c45de9d5b55b246cd4b39d62b9209faf7758ce0637e1f7a545aa0f75f71f0",
            Assert.IsType<string>(contract.ItemCatalogSha256),
            ItemIconProbe.AssetRoot,
            2466,
            939,
            722,
            896,
            185,
            592,
            63,
            26,
            14,
            709,
            [],
            [],
            contract.AllowedLegalMissingIconNames,
            new Dictionary<string, IReadOnlyList<string>>(),
            new Dictionary<string, IReadOnlyList<string>>());

        contract.RequireProbe(probe);

        Assert.Throws<DataPackFailure>(() =>
            contract.RequireProbe(probe with { LegalMatchCount = 708 }));
    }

    [Fact]
    public void ItemIconResolutionUsesOnlyExactOrUnambiguousSharedVariants()
    {
        var assets = new Dictionary<string, string[]>(
            StringComparer.OrdinalIgnoreCase)
        {
            ["Material_PalOil"] =
                ["Pal/Content/Others/InventoryItemIcon/Texture/T_itemicon_Material_PalOil"],
            ["Material_Wood"] =
                ["Pal/Content/Others/InventoryItemIcon/Texture/T_itemicon_Material_Wood"],
            ["Accessory_AirDash"] =
                ["Pal/Content/Others/InventoryItemIcon/Texture/T_itemicon_Accessory_AirDash"],
            ["Accessory_AquaResist_1"] =
                ["Pal/Content/Others/InventoryItemIcon/Texture/T_itemicon_Accessory_AquaResist_1"],
            ["Weapon_AssaultRifle_Default1"] =
                ["Pal/Content/Others/InventoryItemIcon/Texture/T_itemicon_Weapon_AssaultRifle_Default1"],
        };

        var exact = ItemIconProbe.Resolve("Material_PalOil", assets);
        var categorySuffix = ItemIconProbe.Resolve("PalOil", assets);
        var typedExact = ItemIconProbe.Resolve("Wood", assets, "Material");
        var trimmed = ItemIconProbe.Resolve("Accessory_AirDash1", assets);
        var suffixed = ItemIconProbe.Resolve("Accessory_AquaResist", assets);
        var reviewedAlias = ItemIconProbe.Resolve("AssaultRifle_Default", assets);
        var missing = ItemIconProbe.Resolve("DoesNotExist", assets);

        Assert.Equal("exact", exact.MatchKind);
        Assert.Single(exact.PackagePaths);
        Assert.Equal("typed_exact", typedExact.MatchKind);
        Assert.Equal(
            "Pal/Content/Others/InventoryItemIcon/Texture/T_itemicon_Material_Wood",
            typedExact.PackagePaths.Single());
        Assert.Equal("category_suffix", categorySuffix.MatchKind);
        Assert.Single(categorySuffix.PackagePaths);
        Assert.Equal("shared_variant", trimmed.MatchKind);
        Assert.Single(trimmed.PackagePaths);
        Assert.Equal("shared_variant", suffixed.MatchKind);
        Assert.Single(suffixed.PackagePaths);
        Assert.Equal("reviewed_alias", reviewedAlias.MatchKind);
        Assert.Single(reviewedAlias.PackagePaths);
        Assert.Equal("missing", missing.MatchKind);
        Assert.Empty(missing.PackagePaths);
    }

    [Fact]
    public void Committed_contract_is_exact_build_candidate_not_approved_extraction()
    {
        var contract = AssetContract.Load(Path.Combine(
            TestPaths.RepositoryRoot(),
            "tools",
            "pal-data-pack",
            "contracts",
            "24181527.asset-contract.json"));

        Assert.Equal("24181527", contract.GameBuildId);
        Assert.False(contract.Reviewed);
        Assert.Equal(
            "241c45de9d5b55b246cd4b39d62b9209faf7758ce0637e1f7a545aa0f75f71f0",
            contract.ApprovedMappingSha256);
        Assert.Throws<DataPackFailure>(() =>
            contract.RequireExtractionIdentity(
                contract.GameBuildId,
                contract.ApprovedMappingSha256));
    }

    [Fact]
    public void Reviewed_contract_requires_exact_build_and_mapping_hash()
    {
        var contract = AssetContract.Parse(
            """
            {
              "schema_major": 1,
              "game_build_id": "24181527",
              "reviewed": true,
              "review_id": "review:fixture",
              "approved_mapping_sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
              "tables": [
                {
                  "capability": "pals",
                  "package_path": "Pal/Content/Pal/DataTable/Character/DT_PalMonsterParameter",
                  "required": true,
                  "required_properties": ["CharacterID"]
                }
              ]
            }
            """);

        contract.RequireExtractionIdentity(
            "24181527",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");

        var failure = Assert.Throws<DataPackFailure>(() =>
            contract.RequireExtractionIdentity(
                "24181528",
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"));
        Assert.Equal(DataPackExitCode.AssetContractMismatch, failure.ExitCode);
    }

    [Fact]
    public void Item_catalog_contract_is_reviewed_and_pins_all_source_row_counts()
    {
        var contract = AssetContract.Load(Path.Combine(
            TestPaths.RepositoryRoot(),
            "tools",
            "pal-data-pack",
            "contracts",
            "24181527.item-catalog-contract.json"));

        Assert.True(contract.Reviewed);
        Assert.Equal("24181527", contract.GameBuildId);
        Assert.Equal(5, contract.Tables.Count);
        Assert.All(contract.Tables, table => Assert.NotNull(table.ExpectedRowCount));
        Assert.Equal(
            [2466, 1414, 1044, 1994, 1924],
            contract.Tables.Select(table => table.ExpectedRowCount!.Value).ToArray());
        contract.RequireExtractionIdentity(
            "24181527",
            "241c45de9d5b55b246cd4b39d62b9209faf7758ce0637e1f7a545aa0f75f71f0");
    }

    [Fact]
    public void World_catalog_contract_is_reviewed_and_pins_all_source_row_counts()
    {
        var contract = AssetContract.Load(Path.Combine(
            TestPaths.RepositoryRoot(),
            "tools",
            "pal-data-pack",
            "contracts",
            "24181527.world-catalog-contract.json"));

        Assert.True(contract.Reviewed);
        Assert.Equal("24181527", contract.GameBuildId);
        Assert.Equal(12, contract.Tables.Count);
        Assert.All(contract.Tables, table => Assert.NotNull(table.ExpectedRowCount));
        Assert.Equal(
            [498, 562, 588, 632, 16, 38, 38, 3, 549, 835, 587, 616],
            contract.Tables.Select(table => table.ExpectedRowCount!.Value).ToArray());
        contract.RequireExtractionIdentity(
            "24181527",
            "241c45de9d5b55b246cd4b39d62b9209faf7758ce0637e1f7a545aa0f75f71f0");
    }

    [Fact]
    public void Map_spawn_contract_pins_placement_and_weighted_group_units()
    {
        var contract = AssetContract.Load(Path.Combine(
            TestPaths.RepositoryRoot(),
            "tools",
            "pal-data-pack",
            "contracts",
            "24181527.map-spawn-contract.json"));

        Assert.True(contract.Reviewed);
        Assert.Equal("24181527", contract.GameBuildId);
        Assert.Equal(2, contract.Tables.Count);
        Assert.Equal(
            [8253, 1691],
            contract.Tables.Select(table => table.ExpectedRowCount!.Value).ToArray());
        Assert.All(contract.Tables, table =>
        {
            Assert.True(table.Required);
            Assert.NotEmpty(table.RequiredProperties);
        });
        contract.RequireExtractionIdentity(
            "24181527",
            "241c45de9d5b55b246cd4b39d62b9209faf7758ce0637e1f7a545aa0f75f71f0");
    }

    [Theory]
    [InlineData("24181527", 3137)]
    [InlineData("24467282", 3143)]
    [InlineData("24575825", 3175)]
    public void Korean_ui_contract_pins_the_original_Korean_game_table(
        string buildId,
        int expectedRowCount)
    {
        var contract = AssetContract.Load(Path.Combine(
            TestPaths.RepositoryRoot(),
            "tools",
            "pal-data-pack",
            "contracts",
            $"{buildId}.korean-ui-contract.json"));

        Assert.True(contract.Reviewed);
        Assert.Equal(buildId, contract.GameBuildId);
        var table = Assert.Single(contract.Tables);
        Assert.Equal("localization", table.Capability);
        Assert.Equal(
            "Pal/Content/L10N/ko/Pal/DataTable/Text/DT_UI_Common_Text_Common",
            table.PackagePath);
        Assert.Equal(expectedRowCount, table.ExpectedRowCount);
        Assert.Equal(["TextData"], table.RequiredProperties);
        contract.RequireExtractionIdentity(
            buildId,
            "241c45de9d5b55b246cd4b39d62b9209faf7758ce0637e1f7a545aa0f75f71f0");
    }

    [Theory]
    [InlineData("24467282", 3143, 616)]
    [InlineData("24575825", 3175, 617)]
    public void Poi_terminology_contract_pins_reviewed_Korean_source_tables(
        string buildId,
        int uiRowCount,
        int mapObjectRowCount)
    {
        var contract = AssetContract.Load(Path.Combine(
            TestPaths.RepositoryRoot(),
            "tools",
            "pal-data-pack",
            "contracts",
            $"{buildId}.poi-terminology-l10n-contract.json"));

        Assert.True(contract.Reviewed);
        Assert.Equal(buildId, contract.GameBuildId);
        Assert.Equal(
            [uiRowCount, 156, mapObjectRowCount, 199, 42, 1994, 67, 132, 167, 549],
            contract.Tables.Select(table => table.ExpectedRowCount!.Value).ToArray());
        Assert.All(
            contract.Tables,
            table =>
            {
                Assert.Equal("localization", table.Capability);
                Assert.StartsWith("Pal/Content/L10N/ko/", table.PackagePath);
                Assert.True(table.Required);
                Assert.Equal(["TextData"], table.RequiredProperties);
            });
        contract.RequireExtractionIdentity(
            buildId,
            "241c45de9d5b55b246cd4b39d62b9209faf7758ce0637e1f7a545aa0f75f71f0");
    }

    [Fact]
    public void Korean_catalog_contract_pins_only_original_Korean_game_tables()
    {
        var contract = AssetContract.Load(Path.Combine(
            TestPaths.RepositoryRoot(),
            "tools",
            "pal-data-pack",
            "contracts",
            "24181527.korean-catalog-contract.json"));

        Assert.True(contract.Reviewed);
        Assert.Equal(4, contract.Tables.Count);
        Assert.All(
            contract.Tables,
            table =>
            {
                Assert.StartsWith("Pal/Content/L10N/ko/", table.PackagePath);
                Assert.Equal(["TextData"], table.RequiredProperties);
                Assert.NotNull(table.ExpectedRowCount);
            });
        Assert.Equal(
            [322, 310, 1157, 439],
            contract.Tables.Select(table => table.ExpectedRowCount!.Value).ToArray());
        contract.RequireExtractionIdentity(
            "24181527",
            "241c45de9d5b55b246cd4b39d62b9209faf7758ce0637e1f7a545aa0f75f71f0");
    }

    [Fact]
    public void Contract_rejects_duplicate_capability_table_paths()
    {
        Assert.Throws<DataPackFailure>(() => AssetContract.Parse(
            """
            {
              "schema_major": 1,
              "game_build_id": "24181527",
              "reviewed": true,
              "review_id": "review:fixture",
              "approved_mapping_sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
              "tables": [
                {
                  "capability": "pals",
                  "package_path": "Pal/Content/Pal/DataTable/Character/DT_PalMonsterParameter",
                  "required": true,
                  "required_properties": ["CharacterID"]
                },
                {
                  "capability": "skills",
                  "package_path": "pal/content/pal/datatable/character/dt_palmonsterparameter",
                  "required": true,
                  "required_properties": ["CharacterID"]
                }
              ]
            }
            """));
    }
}
