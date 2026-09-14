using System.Globalization;
using System.Text;
using System.Text.Json;
using System.Text.RegularExpressions;
using CUE4Parse.FileProvider;
using CUE4Parse.UE4.Assets.Exports.Engine;
using CUE4Parse.UE4.Assets.Objects;
using CUE4Parse.UE4.Assets.Objects.Properties;
using CUE4Parse.UE4.Objects.Core.i18N;

namespace PalDataPack;

public sealed record VerifiedWorldCatalogWriteResult(
    string GameBuildId,
    string MappingSha256,
    string ContractReviewId,
    string OutputPath,
    string OutputSha256,
    int BuildingCount,
    int LocalizedBuildingNameCount,
    int TechnologyCount,
    int LocalizedTechnologyNameCount,
    int ProductionMethodCount,
    int ShopGroupCount,
    int ShopProductCount,
    int ShopPoolCount,
    int IgnoredSentinelBuildingMaterialSlotCount,
    int IgnoredUnknownShopPoolEntryCount);

public sealed record VerifiedWorldCatalogDocument(
    int SchemaVersion,
    string GameBuildId,
    string MappingSha256,
    string ContractReviewId,
    bool Verified,
    int IgnoredSentinelBuildingMaterialSlotCount,
    int IgnoredUnknownShopPoolEntryCount,
    IReadOnlyList<WorldCatalogSource> Sources,
    IReadOnlyList<VerifiedBuildingRecord> Buildings,
    IReadOnlyList<VerifiedTechnologyRecord> Technologies,
    IReadOnlyList<VerifiedProductionMethodRecord> ProductionMethods,
    IReadOnlyList<VerifiedShopGroupRecord> ShopGroups,
    IReadOnlyList<VerifiedShopPoolRecord> ShopPools);

public sealed record WorldCatalogSource(
    string Capability,
    string PackagePath,
    int RowCount);

public sealed record VerifiedBuildingMaterial(
    string ItemId,
    string NameKo,
    int Quantity);

public sealed record VerifiedBuildingRecord(
    string BuildingId,
    string NameKo,
    string? DescriptionKo,
    string Category,
    string Subcategory,
    string UiCategory,
    string RequiredEnergyType,
    int RequiredBuildWorkAmount,
    int ConsumeEnergySpeed,
    int BuildCapacity,
    int InstallMaxNumInBaseCamp,
    int Rank,
    int SortId,
    int? Hp,
    int? Defense,
    string? MaterialType,
    string? MaterialSubtype,
    string? IconSourcePath,
    bool BelongsToBaseCamp,
    bool InstallOnlyNearPalbox,
    bool InstallOnlyInDoor,
    bool InstallOnlyOnBase,
    bool ProhibitedInRaidBossArea,
    bool Paintable,
    bool InDevelopment,
    bool LocalizationFallback,
    IReadOnlyList<VerifiedBuildingMaterial> Materials);

public sealed record VerifiedTechnologyRecord(
    string TechnologyId,
    string NameKo,
    string? DescriptionKo,
    string IconName,
    int Level,
    int Cost,
    int Tier,
    bool IsBossTechnology,
    string? RequiredTowerBoss,
    string? RequiredResearchId,
    string? RequiredTechnologyId,
    bool LocalizationFallback,
    IReadOnlyList<string> UnlockBuildingIds,
    IReadOnlyList<string> UnlockRecipeIds,
    IReadOnlyList<string> UnlockItemIds);

public sealed record VerifiedProductionMethodRecord(
    string MethodId,
    string BuildingId,
    string BuildingNameKo,
    string ItemId,
    string ItemNameKo,
    int RequiredWorkAmount,
    int AutoWorkAmountPerSecond);

public sealed record VerifiedShopProductRecord(
    string ProductId,
    string ItemId,
    string ItemNameKo,
    int Quantity,
    int Price,
    string ProductType,
    int? Stock);

public sealed record VerifiedShopGroupRecord(
    string ShopGroupId,
    string CurrencyItemId,
    string CurrencyNameKo,
    IReadOnlyList<VerifiedShopProductRecord> Products);

public sealed record VerifiedShopPoolEntry(
    string ShopGroupId,
    int Weight);

public sealed record VerifiedShopPoolRecord(
    string ShopPoolId,
    IReadOnlyList<VerifiedShopPoolEntry> Entries);

public static partial class VerifiedWorldCatalogExtractor
{
    private const string BuildingsPath =
        "Pal/Content/Pal/DataTable/MapObject/Building/DT_BuildObjectDataTable_Common";
    private const string BuildingIconsPath =
        "Pal/Content/Pal/DataTable/MapObject/Building/DT_BuildObjectIconDataTable_Common";
    private const string TechnologiesPath =
        "Pal/Content/Pal/DataTable/Technology/DT_TechnologyRecipeUnlock_Common";
    private const string MapObjectMasterPath =
        "Pal/Content/Pal/DataTable/MapObject/DT_MapObjectMasterDataTable_Common";
    private const string ProductionPath =
        "Pal/Content/Pal/DataTable/MapObject/DT_MapObjectItemProductDataTable_Common";
    private const string ShopCreatePath =
        "Pal/Content/Pal/DataTable/ItemShop/DT_ItemShopCreateData_Common";
    private const string ShopLotteryPath =
        "Pal/Content/Pal/DataTable/ItemShop/DT_ItemShopLotteryData_Common";
    private const string ShopSettingsPath =
        "Pal/Content/Pal/DataTable/ItemShop/DT_ItemShopSettingData_Common";
    private const string BuildingDescriptionsPath =
        "Pal/Content/L10N/ko/Pal/DataTable/Text/DT_BuildObjectDescText_Common";
    private const string TechnologyNamesPath =
        "Pal/Content/L10N/ko/Pal/DataTable/Text/DT_TechnologyNameText_Common";
    private const string TechnologyDescriptionsPath =
        "Pal/Content/L10N/ko/Pal/DataTable/Text/DT_TechnologyDescText_Common";
    private const string MapObjectNamesPath =
        "Pal/Content/L10N/ko/Pal/DataTable/Text/DT_MapObjectNameText_Common";
    private const int MaximumItemCatalogBytes = 32 * 1024 * 1024;
    private const int MaximumOutputBytes = 16 * 1024 * 1024;

    private static readonly JsonSerializerOptions JsonOptions = new()
    {
        PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower,
        WriteIndented = false,
    };

    [GeneratedRegex(@"<?itemName\s+id=\|([^|]+)\|/?>", RegexOptions.CultureInvariant)]
    private static partial Regex ItemNameMarkup();

    [GeneratedRegex(@"<?mapObjectName\s+id=\|([^|]+)\|/?>", RegexOptions.CultureInvariant)]
    private static partial Regex MapObjectNameMarkup();

    [GeneratedRegex(@"<[^>]+>", RegexOptions.CultureInvariant)]
    private static partial Regex OtherMarkup();

    [GeneratedRegex(@"\s+", RegexOptions.CultureInvariant)]
    private static partial Regex Whitespace();

    public static VerifiedWorldCatalogWriteResult ExtractToFile(
        SteamInstall install,
        string mappingPath,
        AssetContract contract,
        string itemCatalogPath,
        string outputPath)
    {
        SecureInput.EnsureRegularFile(mappingPath);
        var mappingSha256 = Hashing.Sha256File(mappingPath);
        contract.RequireExtractionIdentity(install.BuildId, mappingSha256);
        RequireExactTables(contract);
        var itemCatalog = ReadItemCatalog(itemCatalogPath, install.BuildId, mappingSha256);
        var items = itemCatalog.Items;

        var phase = "mount";
        try
        {
            var provider = SourceContractProbe.OpenProvider(install.InstallPath, mappingPath);
            phase = "contracted tables";
            var tables = LoadAndValidateTables(provider, contract);
            phase = "Korean localization";
            var mapObjectNames = ReadLocalization(tables[MapObjectNamesPath]);
            var buildingDescriptions = ReadLocalization(tables[BuildingDescriptionsPath]);
            var technologyNames = ReadLocalization(tables[TechnologyNamesPath]);
            var technologyDescriptions = ReadLocalization(tables[TechnologyDescriptionsPath]);
            phase = "building rows";
            var buildings = ReadBuildings(
                tables,
                items,
                mapObjectNames,
                buildingDescriptions,
                out var ignoredSentinelBuildingMaterialSlotCount);
            var buildingNames = buildings.ToDictionary(
                value => value.BuildingId,
                value => value.NameKo,
                StringComparer.OrdinalIgnoreCase);
            phase = "technology rows";
            var technologies = ReadTechnologies(
                tables[TechnologiesPath],
                items,
                itemCatalog.RecipeOutputs,
                mapObjectNames,
                technologyNames,
                technologyDescriptions);
            phase = "base production rows";
            var productionMethods = ReadProductionMethods(
                tables[ProductionPath],
                items,
                buildingNames);
            phase = "shop rows";
            var shopGroups = ReadShopGroups(
                tables[ShopCreatePath],
                tables[ShopSettingsPath],
                items);
            phase = "shop pool rows";
            var shopPools = ReadShopPools(
                tables[ShopLotteryPath],
                shopGroups,
                out var ignoredUnknownShopPoolEntryCount);

            phase = "serialization";
            var document = new VerifiedWorldCatalogDocument(
                SchemaVersion: 1,
                GameBuildId: $"steam:{install.BuildId}",
                MappingSha256: mappingSha256,
                ContractReviewId: contract.ReviewId,
                Verified: true,
                IgnoredSentinelBuildingMaterialSlotCount:
                    ignoredSentinelBuildingMaterialSlotCount,
                IgnoredUnknownShopPoolEntryCount: ignoredUnknownShopPoolEntryCount,
                Sources: contract.Tables
                    .OrderBy(value => value.PackagePath, StringComparer.Ordinal)
                    .Select(value => new WorldCatalogSource(
                        value.Capability,
                        value.PackagePath,
                        tables[value.PackagePath].RowMap.Count))
                    .ToArray(),
                Buildings: buildings,
                Technologies: technologies,
                ProductionMethods: productionMethods,
                ShopGroups: shopGroups,
                ShopPools: shopPools);
            var bytes = JsonSerializer.SerializeToUtf8Bytes(document, JsonOptions);
            if (bytes.Length > MaximumOutputBytes)
            {
                throw Failure("verified world catalog exceeds its output bound");
            }
            phase = "atomic write";
            var writtenPath = AtomicWrite(outputPath, bytes);
            return new VerifiedWorldCatalogWriteResult(
                document.GameBuildId,
                mappingSha256,
                contract.ReviewId,
                writtenPath,
                Hashing.Sha256Hex(bytes),
                buildings.Count,
                buildings.Count(value => !value.LocalizationFallback),
                technologies.Count,
                technologies.Count(value => !value.LocalizationFallback),
                productionMethods.Count,
                shopGroups.Count,
                shopGroups.Sum(value => value.Products.Count),
                shopPools.Count,
                ignoredSentinelBuildingMaterialSlotCount,
                ignoredUnknownShopPoolEntryCount);
        }
        catch (DataPackFailure)
        {
            throw;
        }
        catch (Exception error)
        {
            throw new DataPackFailure(
                DataPackExitCode.MountOrSerialization,
                $"exact-Build world extraction failed during {phase} "
                + $"({error.GetType().Name})");
        }
    }

    private static IReadOnlyList<VerifiedBuildingRecord> ReadBuildings(
        IReadOnlyDictionary<string, UDataTable> tables,
        IReadOnlyDictionary<string, CatalogItem> items,
        IReadOnlyDictionary<string, string> names,
        IReadOnlyDictionary<string, string> descriptions,
        out int ignoredSentinelBuildingMaterialSlotCount)
    {
        ignoredSentinelBuildingMaterialSlotCount = 0;
        var masters = tables[MapObjectMasterPath].RowMap.ToDictionary(
            row => row.Key.Text,
            row => Properties(row.Value),
            StringComparer.OrdinalIgnoreCase);
        var icons = tables[BuildingIconsPath].RowMap.ToDictionary(
            row => row.Key.Text,
            row => OptionalString(Properties(row.Value), "SoftIcon"),
            StringComparer.OrdinalIgnoreCase);
        var records = new List<VerifiedBuildingRecord>(
            tables[BuildingsPath].RowMap.Count);
        foreach (var row in tables[BuildingsPath].RowMap
            .OrderBy(value => value.Key.Text, StringComparer.Ordinal))
        {
            var properties = Properties(row.Value);
            var buildingId = Identifier(row.Key.Text, "building");
            var mapObjectId = Identifier(Name(properties, "MapObjectId"), "map object");
            masters.TryGetValue(mapObjectId, out var master);
            var nameKey = master is not null
                && OptionalString(master, "OverrideNameMsgID") is { } overrideName
                && overrideName != "None"
                    ? overrideName
                    : $"MAPOBJECT_NAME_{mapObjectId}";
            var hasName = names.TryGetValue(nameKey, out var localizedName)
                && !string.IsNullOrWhiteSpace(localizedName);
            var overrideDescription = Name(properties, "OverrideDescMsgID");
            var descriptionKey = overrideDescription == "None"
                ? $"BUILDOBJECT_DESC_{buildingId}"
                : overrideDescription;
            descriptions.TryGetValue(descriptionKey, out var description);
            var materials = new List<VerifiedBuildingMaterial>(4);
            for (var slot = 1; slot <= 4; slot++)
            {
                var itemId = Name(properties, $"Material{slot}_Id");
                var quantity = Int(properties, $"Material{slot}_Count");
                if (itemId == "None")
                {
                    if (quantity != 0)
                    {
                        ignoredSentinelBuildingMaterialSlotCount++;
                    }
                    continue;
                }
                var item = RequireItem(items, itemId, "building material");
                materials.Add(new VerifiedBuildingMaterial(
                    item.ItemId,
                    item.NameKo,
                    Positive(quantity, "building material quantity")));
            }
            icons.TryGetValue(buildingId, out var iconPath);
            records.Add(new VerifiedBuildingRecord(
                buildingId,
                hasName ? NormalizeText(localizedName!) : buildingId,
                string.IsNullOrWhiteSpace(description)
                    ? null
                    : NormalizeMarkup(description, items, names),
                StripEnum(Name(properties, "TypeA")),
                StripEnum(Name(properties, "TypeB")),
                StripEnum(Name(properties, "TypeUIDisplay")),
                StripEnum(Name(properties, "RequiredEnergyType")),
                NonNegative(Int(properties, "RequiredBuildWorkAmount"), "build work"),
                NonNegative(Int(properties, "ConsumeEnergySpeed"), "energy use"),
                NonNegative(Int(properties, "BuildCapacity"), "build capacity"),
                NonNegative(Int(properties, "InstallMaxNumInBaseCamp"), "base limit"),
                NonNegative(Int(properties, "Rank"), "building rank"),
                Int(properties, "SortId"),
                master is null ? null : Int(master, "Hp"),
                master is null ? null : Int(master, "Defense"),
                master is null ? null : StripEnum(Name(master, "MaterialType")),
                master is null ? null : StripEnum(Name(master, "MaterialSubType")),
                iconPath is "None" or "" ? null : iconPath,
                master is not null && Bool(master, "bBelongToBaseCamp"),
                Bool(properties, "bIsInstallOnlyHubAround"),
                Bool(properties, "bIsInstallOnlyInDoor"),
                Bool(properties, "bIsInstallOnlyOnBase"),
                Bool(properties, "bIsProhibitedInRaidBossArea"),
                Bool(properties, "bIsPaintable"),
                master is not null && Bool(master, "bInDevelop"),
                !hasName,
                materials));
        }
        return records;
    }

    private static IReadOnlyList<VerifiedTechnologyRecord> ReadTechnologies(
        UDataTable table,
        IReadOnlyDictionary<string, CatalogItem> items,
        IReadOnlyDictionary<string, string> recipeOutputs,
        IReadOnlyDictionary<string, string> mapObjectNames,
        IReadOnlyDictionary<string, string> names,
        IReadOnlyDictionary<string, string> descriptions)
    {
        var records = new List<VerifiedTechnologyRecord>(table.RowMap.Count);
        foreach (var row in table.RowMap.OrderBy(value => value.Key.Text, StringComparer.Ordinal))
        {
            var properties = Properties(row.Value);
            var id = Identifier(row.Key.Text, "technology");
            var nameKey = Name(properties, "Name");
            var hasName = names.TryGetValue(nameKey, out var localizedName)
                && !string.IsNullOrWhiteSpace(localizedName);
            var descriptionKey = Name(properties, "Description");
            descriptions.TryGetValue(descriptionKey, out var localizedDescription);
            if (string.IsNullOrWhiteSpace(localizedDescription)
                && descriptionKey.StartsWith("ITEM_DESC_", StringComparison.Ordinal)
                && items.TryGetValue(
                    descriptionKey["ITEM_DESC_".Length..],
                    out var describedItem))
            {
                localizedDescription = describedItem.DescriptionKo;
            }
            var unlockRecipeIds = NameArray(properties, "UnlockItemRecipes");
            var unlockItemIds = unlockRecipeIds
                .Select(recipeId => recipeOutputs.TryGetValue(recipeId, out var outputItemId)
                    ? outputItemId
                    : throw Failure(
                        $"technology unlock references an unknown recipe: {recipeId}"))
                .Distinct(StringComparer.Ordinal)
                .ToArray();
            records.Add(new VerifiedTechnologyRecord(
                id,
                hasName
                    ? NormalizeMarkup(localizedName!, items, mapObjectNames)
                    : items.TryGetValue(id, out var item) ? item.NameKo : id,
                string.IsNullOrWhiteSpace(localizedDescription)
                    ? null
                    : NormalizeMarkup(localizedDescription, items, mapObjectNames),
                Name(properties, "IconName"),
                NonNegative(Int(properties, "LevelCap"), "technology level"),
                NonNegative(Int(properties, "Cost"), "technology cost"),
                NonNegative(Int(properties, "Tier"), "technology tier"),
                Bool(properties, "IsBossTechnology"),
                NullableName(properties, "RequireDefeatTowerBoss", stripEnum: true),
                NullableName(properties, "RequireResearchId"),
                NullableName(properties, "RequireTechnology"),
                !hasName && !items.ContainsKey(id),
                NameArray(properties, "UnlockBuildObjects"),
                unlockRecipeIds,
                unlockItemIds));
        }
        return records;
    }

    private static IReadOnlyList<VerifiedProductionMethodRecord> ReadProductionMethods(
        UDataTable table,
        IReadOnlyDictionary<string, CatalogItem> items,
        IReadOnlyDictionary<string, string> buildingNames)
    {
        return table.RowMap
            .OrderBy(value => value.Key.Text, StringComparer.Ordinal)
            .Select(row =>
            {
                var properties = Properties(row.Value);
                var buildingId = Identifier(row.Key.Text, "production building");
                var item = RequireItem(items, Name(properties, "Product_Id"), "production item");
                return new VerifiedProductionMethodRecord(
                    $"map-production:{buildingId}",
                    buildingId,
                    buildingNames.TryGetValue(buildingId, out var name)
                        ? name
                        : buildingId,
                    item.ItemId,
                    item.NameKo,
                    Positive(Int(properties, "RequiredWorkAmount"), "production work amount"),
                    NonNegative(Int(properties, "AutoWorkAmountBySec"), "automatic work rate"));
            })
            .ToArray();
    }

    private static IReadOnlyList<VerifiedShopGroupRecord> ReadShopGroups(
        UDataTable createTable,
        UDataTable settingsTable,
        IReadOnlyDictionary<string, CatalogItem> items)
    {
        var currencies = settingsTable.RowMap.ToDictionary(
            row => row.Key.Text,
            row => Name(Properties(row.Value), "CurrencyItemID"),
            StringComparer.OrdinalIgnoreCase);
        var records = new List<VerifiedShopGroupRecord>(createTable.RowMap.Count);
        foreach (var row in createTable.RowMap.OrderBy(value => value.Key.Text, StringComparer.Ordinal))
        {
            var groupId = Identifier(row.Key.Text, "shop group");
            var currencyId = currencies.TryGetValue(groupId, out var configuredCurrency)
                ? configuredCurrency
                : "Money";
            var currency = RequireItem(items, currencyId, "shop currency");
            var products = StructArray(Properties(row.Value), "productDataArray")
                .Select((entry, index) =>
                {
                    var item = RequireItem(items, Name(entry, "StaticItemId"), "shop product");
                    var overridePrice = Int(entry, "OverridePrice");
                    var itemPrice = NonNegative(item.Price, "item price");
                    var rawStock = Int(entry, "Stock");
                    return new VerifiedShopProductRecord(
                        $"shop:{groupId}:{index + 1}",
                        item.ItemId,
                        item.NameKo,
                        Positive(Int(entry, "ProductNum"), "shop quantity"),
                        overridePrice > 0 ? overridePrice : itemPrice,
                        StripEnum(Name(entry, "ProductType")),
                        rawStock > 0 ? rawStock : null);
                })
                .ToArray();
            records.Add(new VerifiedShopGroupRecord(
                groupId,
                currency.ItemId,
                currency.NameKo,
                products));
        }
        return records;
    }

    private static IReadOnlyList<VerifiedShopPoolRecord> ReadShopPools(
        UDataTable table,
        IReadOnlyList<VerifiedShopGroupRecord> shopGroups,
        out int ignoredUnknownShopPoolEntryCount)
    {
        ignoredUnknownShopPoolEntryCount = 0;
        var knownGroups = shopGroups
            .Select(value => value.ShopGroupId)
            .ToHashSet(StringComparer.OrdinalIgnoreCase);
        var records = new List<VerifiedShopPoolRecord>(table.RowMap.Count);
        foreach (var row in table.RowMap.OrderBy(
            value => value.Key.Text,
            StringComparer.Ordinal))
        {
            var poolId = Identifier(row.Key.Text, "shop pool");
            var entries = new List<VerifiedShopPoolEntry>();
            foreach (var entry in StructArray(
                Properties(row.Value),
                "lotteryDataArray"))
            {
                var groupId = Identifier(Name(entry, "ShopGroupName"), "shop group");
                if (!knownGroups.Contains(groupId))
                {
                    ignoredUnknownShopPoolEntryCount++;
                    continue;
                }
                entries.Add(new VerifiedShopPoolEntry(
                    groupId,
                    Positive(Int(entry, "Weight"), "shop pool weight")));
            }
            if (entries.Count > 0)
            {
                records.Add(new VerifiedShopPoolRecord(poolId, entries));
            }
        }
        return records;
    }

    private static Dictionary<string, UDataTable> LoadAndValidateTables(
        DefaultFileProvider provider,
        AssetContract contract)
    {
        var tables = new Dictionary<string, UDataTable>(StringComparer.Ordinal);
        foreach (var tableContract in contract.Tables)
        {
            var table = provider.LoadPackageObject<UDataTable>(tableContract.PackagePath);
            if (tableContract.ExpectedRowCount is not { } expected
                || table.RowMap.Count != expected)
            {
                throw Failure($"exact row count is not pinned: {tableContract.PackagePath}");
            }
            var observed = table.RowMap.Values
                .SelectMany(row => row.Properties)
                .Select(property => property.Name.Text)
                .ToHashSet(StringComparer.Ordinal);
            if (!tableContract.RequiredProperties.All(observed.Contains))
            {
                throw Failure($"contracted table schema drift: {tableContract.PackagePath}");
            }
            tables.Add(tableContract.PackagePath, table);
        }
        return tables;
    }

    private static ItemCatalogInput ReadItemCatalog(
        string path,
        string buildId,
        string mappingSha256)
    {
        var bytes = SecureInput.ReadBoundedRegularFile(path, MaximumItemCatalogBytes);
        try
        {
            var document = JsonSerializer.Deserialize<ItemCatalogDocument>(bytes, JsonOptions)
                ?? throw Failure("item catalog is empty");
            if (document.SchemaVersion != 1
                || !document.Verified
                || document.GameBuildId != $"steam:{buildId}"
                || document.MappingSha256 != mappingSha256)
            {
                throw Failure("item catalog does not match the exact Build and mapping");
            }
            var items = document.Items.ToDictionary(
                item => item.ItemId,
                item => item,
                StringComparer.OrdinalIgnoreCase);
            var recipes = document.Recipes.ToDictionary(
                recipe => recipe.RecipeId,
                recipe => RequireItem(items, recipe.OutputItemId, "recipe output").ItemId,
                StringComparer.OrdinalIgnoreCase);
            return new ItemCatalogInput(items, recipes);
        }
        catch (JsonException)
        {
            throw Failure("item catalog JSON is invalid");
        }
    }

    private static Dictionary<string, string> ReadLocalization(UDataTable table)
    {
        // World rows and localized rows use case-insensitive Unreal names. Keep
        // exact text while accepting a unique case-only key; TryAdd rejects a
        // future ambiguous pair instead of selecting one silently.
        var result = new Dictionary<string, string>(
            table.RowMap.Count,
            StringComparer.OrdinalIgnoreCase);
        foreach (var row in table.RowMap)
        {
            var text = RequiredTag(Properties(row.Value), "TextData").GetValue<FText>()?.Text;
            if (string.IsNullOrWhiteSpace(text) || !result.TryAdd(row.Key.Text, text))
            {
                throw Failure($"localized text is invalid: {row.Key.Text}");
            }
        }
        return result;
    }

    private static IReadOnlyList<IReadOnlyDictionary<string, FPropertyTagType?>> StructArray(
        IReadOnlyDictionary<string, FPropertyTagType?> properties,
        string name)
    {
        if (RequiredTag(properties, name) is not ArrayProperty { Value: { } array })
        {
            throw Failure($"required structure array is invalid: {name}");
        }
        return array.Properties.Select((element, index) =>
        {
            if (element is not StructProperty { Value.StructType: FStructFallback fallback })
            {
                throw Failure($"structure array element is invalid: {name}/{index}");
            }
            return (IReadOnlyDictionary<string, FPropertyTagType?>)Properties(fallback);
        }).ToArray();
    }

    private static IReadOnlyList<string> NameArray(
        IReadOnlyDictionary<string, FPropertyTagType?> properties,
        string name)
    {
        if (RequiredTag(properties, name) is not ArrayProperty { Value: { } array })
        {
            throw Failure($"required name array is invalid: {name}");
        }
        return array.Properties
            .Select(value => Convert.ToString(value.GenericValue, CultureInfo.InvariantCulture))
            .Select(value => !string.IsNullOrWhiteSpace(value)
                ? Identifier(value, name)
                : throw Failure($"name array element is empty: {name}"))
            .ToArray();
    }

    private static Dictionary<string, FPropertyTagType?> Properties(FStructFallback row) =>
        row.Properties.ToDictionary(
            property => property.Name.Text,
            property => property.Tag,
            StringComparer.Ordinal);

    private static FPropertyTagType RequiredTag(
        IReadOnlyDictionary<string, FPropertyTagType?> properties,
        string name) =>
        properties.TryGetValue(name, out var tag) && tag is not null
            ? tag
            : throw Failure($"required world property is missing: {name}");

    private static string Name(
        IReadOnlyDictionary<string, FPropertyTagType?> properties,
        string name)
    {
        var value = Convert.ToString(
            RequiredTag(properties, name).GenericValue,
            CultureInfo.InvariantCulture);
        return !string.IsNullOrWhiteSpace(value)
            ? value
            : throw Failure($"required name property is empty: {name}");
    }

    private static string? OptionalString(
        IReadOnlyDictionary<string, FPropertyTagType?> properties,
        string name)
    {
        if (!properties.TryGetValue(name, out var tag) || tag is null)
        {
            return null;
        }
        var value = Convert.ToString(tag.GenericValue, CultureInfo.InvariantCulture);
        return string.IsNullOrWhiteSpace(value) ? null : value;
    }

    private static int Int(
        IReadOnlyDictionary<string, FPropertyTagType?> properties,
        string name)
    {
        var value = Convert.ToString(
            RequiredTag(properties, name).GenericValue,
            CultureInfo.InvariantCulture);
        return int.TryParse(value, NumberStyles.Integer, CultureInfo.InvariantCulture, out var parsed)
            ? parsed
            : throw Failure($"required integer property is invalid: {name}");
    }

    private static bool Bool(
        IReadOnlyDictionary<string, FPropertyTagType?> properties,
        string name)
    {
        var value = Convert.ToString(
            RequiredTag(properties, name).GenericValue,
            CultureInfo.InvariantCulture);
        return bool.TryParse(value, out var parsed)
            ? parsed
            : throw Failure($"required Boolean property is invalid: {name}");
    }

    private static string? NullableName(
        IReadOnlyDictionary<string, FPropertyTagType?> properties,
        string name,
        bool stripEnum = false)
    {
        var value = Name(properties, name);
        if (stripEnum)
        {
            value = StripEnum(value);
        }
        return value == "None" ? null : Identifier(value, name);
    }

    private static CatalogItem RequireItem(
        IReadOnlyDictionary<string, CatalogItem> items,
        string itemId,
        string relation) =>
        items.TryGetValue(itemId, out var item)
            ? item
            : throw Failure($"{relation} references an unknown item: {itemId}");

    private static string NormalizeMarkup(
        string value,
        IReadOnlyDictionary<string, CatalogItem> items,
        IReadOnlyDictionary<string, string> mapObjectNames)
    {
        var text = ItemNameMarkup().Replace(
            value,
            match => items.TryGetValue(match.Groups[1].Value, out var item)
                ? item.NameKo
                : match.Groups[1].Value);
        text = MapObjectNameMarkup().Replace(
            text,
            match => mapObjectNames.TryGetValue(
                    $"MAPOBJECT_NAME_{match.Groups[1].Value}",
                    out var name)
                ? name
                : match.Groups[1].Value);
        text = OtherMarkup().Replace(text, " ");
        text = text.Replace('|', ' ');
        return NormalizeText(text);
    }

    private static string NormalizeText(string value) =>
        Whitespace().Replace(value, " ").Trim();

    private static string StripEnum(string value)
    {
        var separator = value.IndexOf("::", StringComparison.Ordinal);
        return separator >= 0 ? value[(separator + 2)..] : value;
    }

    private static string Identifier(string value, string field)
    {
        CatalogContract.RequireIdentifier(value, field);
        return value;
    }

    private static int NonNegative(int value, string field) =>
        value >= 0 ? value : throw Failure($"{field} cannot be negative");

    private static int Positive(int value, string field) =>
        value > 0 ? value : throw Failure($"{field} must be positive");

    private static string AtomicWrite(string outputPath, ReadOnlySpan<byte> bytes)
    {
        var fullPath = Path.GetFullPath(outputPath);
        var directory = Path.GetDirectoryName(fullPath)
            ?? throw Failure("world catalog output directory is invalid");
        Directory.CreateDirectory(directory);
        SecureInput.EnsurePlainDirectory(directory);
        var temporaryPath = Path.Combine(
            directory,
            $".{Path.GetFileName(fullPath)}.{Guid.NewGuid():N}.tmp");
        try
        {
            using (var stream = new FileStream(
                temporaryPath,
                FileMode.CreateNew,
                FileAccess.Write,
                FileShare.None,
                64 * 1024,
                FileOptions.WriteThrough))
            {
                stream.Write(bytes);
                stream.Flush(flushToDisk: true);
            }
            File.Move(temporaryPath, fullPath, overwrite: true);
            return fullPath;
        }
        finally
        {
            if (File.Exists(temporaryPath))
            {
                File.Delete(temporaryPath);
            }
        }
    }

    private static void RequireExactTables(AssetContract contract)
    {
        var expected = new[]
        {
            BuildingsPath,
            BuildingIconsPath,
            TechnologiesPath,
            MapObjectMasterPath,
            ProductionPath,
            ShopCreatePath,
            ShopLotteryPath,
            ShopSettingsPath,
            BuildingDescriptionsPath,
            TechnologyNamesPath,
            TechnologyDescriptionsPath,
            MapObjectNamesPath,
        };
        if (contract.Tables.Count != expected.Length
            || !contract.Tables.Select(value => value.PackagePath)
                .Order(StringComparer.Ordinal)
                .SequenceEqual(expected.Order(StringComparer.Ordinal), StringComparer.Ordinal)
            || contract.Tables.Any(value =>
                !value.Required || value.ExpectedRowCount is null))
        {
            throw Failure("world extraction requires the closed twelve-table contract");
        }
    }

    private sealed record ItemCatalogDocument(
        int SchemaVersion,
        string GameBuildId,
        string MappingSha256,
        bool Verified,
        IReadOnlyList<CatalogItem> Items,
        IReadOnlyList<CatalogRecipe> Recipes);

    private sealed record CatalogItem(
        string ItemId,
        string NameKo,
        string? DescriptionKo,
        int Price);

    private sealed record CatalogRecipe(
        string RecipeId,
        string OutputItemId);

    private sealed record ItemCatalogInput(
        IReadOnlyDictionary<string, CatalogItem> Items,
        IReadOnlyDictionary<string, string> RecipeOutputs);

    private static DataPackFailure Failure(string message) =>
        new(DataPackExitCode.SourceContractMismatch, message);
}
