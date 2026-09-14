using System.Globalization;
using System.Text;
using System.Text.Json;
using System.Text.RegularExpressions;
using CUE4Parse.FileProvider;
using CUE4Parse.UE4.Assets.Exports.Engine;
using CUE4Parse.UE4.Assets.Objects.Properties;
using CUE4Parse.UE4.Objects.Core.i18N;

namespace PalDataPack;

public sealed record VerifiedItemCatalogWriteResult(
    string GameBuildId,
    string MappingSha256,
    string ContractReviewId,
    string OutputPath,
    string OutputSha256,
    int ItemCount,
    int LegalItemCount,
    int LocalizedNameCount,
    int LocalizedDescriptionCount,
    int RecipeCount,
    int IgnoredSentinelIngredientSlotCount,
    int IgnoredSentinelDropSlotCount,
    int IgnoredZeroProbabilityDropSlotCount,
    int PalDropCount);

public sealed record VerifiedItemCatalogDocument(
    int SchemaVersion,
    string GameBuildId,
    string MappingSha256,
    string ContractReviewId,
    bool Verified,
    int IgnoredSentinelIngredientSlotCount,
    int IgnoredSentinelDropSlotCount,
    int IgnoredZeroProbabilityDropSlotCount,
    IReadOnlyList<ItemCatalogSource> Sources,
    IReadOnlyList<VerifiedItemRecord> Items,
    IReadOnlyList<VerifiedRecipeRecord> Recipes,
    IReadOnlyList<VerifiedPalDropRecord> PalDrops);

public sealed record ItemCatalogSource(
    string Capability,
    string PackagePath,
    int RowCount);

public sealed record VerifiedItemRecord(
    string ItemId,
    string NameKo,
    string? DescriptionKo,
    string TypeA,
    string TypeB,
    int Price,
    int WeightMilli,
    int MaximumStackCount,
    int Rarity,
    int Rank,
    string IconName,
    bool LegalInGame,
    bool LocalizationFallback);

public sealed record VerifiedRecipeIngredient(
    string ItemId,
    int Quantity);

public sealed record VerifiedRecipeRecord(
    string RecipeId,
    string OutputItemId,
    int OutputQuantity,
    IReadOnlyList<VerifiedRecipeIngredient> Ingredients,
    int WorkAmount,
    string? UnlockItemId);

public sealed record VerifiedPalDropRecord(
    string MethodId,
    string SourceRowId,
    int Slot,
    string PalId,
    int Level,
    string ItemId,
    int MinimumQuantity,
    int MaximumQuantity,
    int ProbabilityPpm);

public static partial class VerifiedItemCatalogExtractor
{
    private const string ItemsPath =
        "Pal/Content/Pal/DataTable/Item/DT_ItemDataTable";
    private const string RecipesPath =
        "Pal/Content/Pal/DataTable/Item/DT_ItemRecipeDataTable";
    private const string DropsPath =
        "Pal/Content/Pal/DataTable/Character/DT_PalDropItem";
    private const string KoreanNamesPath =
        "Pal/Content/L10N/ko/Pal/DataTable/Text/DT_ItemNameText_Common";
    private const string KoreanDescriptionsPath =
        "Pal/Content/L10N/ko/Pal/DataTable/Text/DT_ItemDescriptionText_Common";
    private const int MaximumOutputBytes = 32 * 1024 * 1024;

    private static readonly JsonSerializerOptions JsonOptions = new()
    {
        PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower,
        WriteIndented = false,
    };

    [GeneratedRegex(@"<mapObjectName\s+id=\|([^|]+)\|/>", RegexOptions.CultureInvariant)]
    private static partial Regex MapObjectNameMarkup();

    [GeneratedRegex(@"<itemName\s+id=\|([^|]+)\|/>", RegexOptions.CultureInvariant)]
    private static partial Regex ItemNameMarkup();

    [GeneratedRegex(@"<[^>]+>", RegexOptions.CultureInvariant)]
    private static partial Regex OtherMarkup();

    [GeneratedRegex(@"\s+", RegexOptions.CultureInvariant)]
    private static partial Regex Whitespace();

    public static VerifiedItemCatalogWriteResult ExtractToFile(
        SteamInstall install,
        string mappingPath,
        AssetContract contract,
        string outputPath)
    {
        SecureInput.EnsureRegularFile(mappingPath);
        var mappingSha256 = Hashing.Sha256File(mappingPath);
        contract.RequireExtractionIdentity(install.BuildId, mappingSha256);
        RequireExactTables(contract);

        var phase = "mount";
        try
        {
            var provider = SourceContractProbe.OpenProvider(
                install.InstallPath,
                mappingPath);
            phase = "contracted tables";
            var sourceTables = LoadAndValidateTables(provider, contract);
            phase = "Korean item names";
            var names = ReadLocalization(sourceTables[KoreanNamesPath], "ITEM_NAME_");
            phase = "Korean item descriptions";
            var descriptions = ReadLocalization(
                sourceTables[KoreanDescriptionsPath],
                "ITEM_DESC_");
            phase = "item rows";
            var items = ReadItems(sourceTables[ItemsPath], names, descriptions);
            var itemIds = items.ToDictionary(
                item => item.ItemId,
                item => item.ItemId,
                StringComparer.OrdinalIgnoreCase);
            phase = "recipe rows";
            var recipes = ReadRecipes(
                sourceTables[RecipesPath],
                itemIds,
                out var ignoredSentinelIngredientSlotCount);
            phase = "Pal drop rows";
            var drops = ReadDrops(
                sourceTables[DropsPath],
                itemIds,
                out var ignoredSentinelDropSlotCount,
                out var ignoredZeroProbabilityDropSlotCount);

            phase = "serialization";
            var document = new VerifiedItemCatalogDocument(
                SchemaVersion: 1,
                GameBuildId: $"steam:{install.BuildId}",
                MappingSha256: mappingSha256,
                ContractReviewId: contract.ReviewId,
                Verified: true,
                IgnoredSentinelIngredientSlotCount: ignoredSentinelIngredientSlotCount,
                IgnoredSentinelDropSlotCount: ignoredSentinelDropSlotCount,
                IgnoredZeroProbabilityDropSlotCount: ignoredZeroProbabilityDropSlotCount,
                Sources: contract.Tables
                    .OrderBy(table => table.PackagePath, StringComparer.Ordinal)
                    .Select(table => new ItemCatalogSource(
                        table.Capability,
                        table.PackagePath,
                        sourceTables[table.PackagePath].RowMap.Count))
                    .ToArray(),
                Items: items,
                Recipes: recipes,
                PalDrops: drops);
            var bytes = JsonSerializer.SerializeToUtf8Bytes(document, JsonOptions);
            if (bytes.Length > MaximumOutputBytes)
            {
                throw Failure("verified item catalog exceeds its output bound");
            }
            phase = "atomic write";
            var writtenPath = AtomicWrite(outputPath, bytes);
            return new VerifiedItemCatalogWriteResult(
                document.GameBuildId,
                mappingSha256,
                contract.ReviewId,
                writtenPath,
                Hashing.Sha256Hex(bytes),
                items.Count,
                items.Count(item => item.LegalInGame),
                items.Count(item => !item.LocalizationFallback),
                items.Count(item => item.DescriptionKo is not null),
                recipes.Count,
                ignoredSentinelIngredientSlotCount,
                ignoredSentinelDropSlotCount,
                ignoredZeroProbabilityDropSlotCount,
                drops.Count);
        }
        catch (DataPackFailure)
        {
            throw;
        }
        catch (Exception error)
        {
            throw new DataPackFailure(
                DataPackExitCode.MountOrSerialization,
                $"exact-Build item extraction failed during {phase} "
                + $"({error.GetType().Name})");
        }
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

    private static IReadOnlyList<VerifiedItemRecord> ReadItems(
        UDataTable table,
        IReadOnlyDictionary<string, string> names,
        IReadOnlyDictionary<string, string> descriptions)
    {
        var records = new List<VerifiedItemRecord>(table.RowMap.Count);
        foreach (var row in table.RowMap.OrderBy(row => row.Key.Text, StringComparer.Ordinal))
        {
            try
            {
                var itemId = RequireIdentifier(row.Key.Text, "item");
                var properties = Properties(row.Value);
                var overrideName = Name(properties, "OverrideName");
                var overrideDescription = Name(properties, "OverrideDescription");
                var nameKey = overrideName == "None"
                    ? itemId
                    : StripPrefix(overrideName, "ITEM_NAME_");
                var descriptionKey = overrideDescription == "None"
                    ? itemId
                    : StripPrefix(overrideDescription, "ITEM_DESC_");
                var hasLocalizedName = names.TryGetValue(nameKey, out var localizedName)
                    && !string.IsNullOrWhiteSpace(localizedName);
                descriptions.TryGetValue(descriptionKey, out var localizedDescription);
                records.Add(new VerifiedItemRecord(
                    itemId,
                    hasLocalizedName ? NormalizeText(localizedName!) : itemId,
                    string.IsNullOrWhiteSpace(localizedDescription)
                        ? null
                        : NormalizeDescription(localizedDescription, names),
                    StripEnum(Name(properties, "TypeA")),
                    StripEnum(Name(properties, "TypeB")),
                    NonNegative(Int(properties, "Price"), "item price"),
                    NonNegative(ScaleMilli(Float(properties, "Weight")), "item weight"),
                    NonNegative(Int(properties, "MaxStackCount"), "maximum stack count"),
                    NonNegative(Int(properties, "Rarity"), "item rarity"),
                    NonNegative(Int(properties, "Rank"), "item rank"),
                    RequireIdentifier(Name(properties, "IconName"), "icon"),
                    Bool(properties, "bLegalInGame"),
                    !hasLocalizedName));
            }
            catch (DataPackFailure error)
            {
                throw Failure($"item row is invalid: {row.Key.Text}; {error.Message}");
            }
            catch (Exception error)
            {
                throw Failure(
                    $"item row could not be decoded: {row.Key.Text} "
                    + $"({error.GetType().Name})");
            }
        }
        return records;
    }

    private static IReadOnlyList<VerifiedRecipeRecord> ReadRecipes(
        UDataTable table,
        IReadOnlyDictionary<string, string> itemIds,
        out int ignoredSentinelIngredientSlotCount)
    {
        ignoredSentinelIngredientSlotCount = 0;
        var records = new List<VerifiedRecipeRecord>(table.RowMap.Count);
        foreach (var row in table.RowMap.OrderBy(row => row.Key.Text, StringComparer.Ordinal))
        {
            try
            {
                var properties = Properties(row.Value);
                var recipeId = RequireIdentifier(row.Key.Text, "recipe");
                var outputItemId = RequireItem(
                    Name(properties, "Product_Id"),
                    itemIds,
                    "recipe output");
                var outputQuantity = Positive(Int(properties, "Product_Count"), "recipe output");
                var ingredients = new List<VerifiedRecipeIngredient>(5);
                for (var slot = 1; slot <= 5; slot++)
                {
                    var itemId = Name(properties, $"Material{slot}_Id");
                    var quantity = Int(properties, $"Material{slot}_Count");
                    if (itemId == "None")
                    {
                        if (quantity != 0)
                        {
                            ignoredSentinelIngredientSlotCount++;
                        }
                        continue;
                    }
                    ingredients.Add(new VerifiedRecipeIngredient(
                        RequireItem(itemId, itemIds, "recipe ingredient"),
                        Positive(quantity, "recipe ingredient")));
                }
                if (ingredients.Count == 0)
                {
                    throw Failure($"recipe has no ingredients: {recipeId}");
                }
                var unlock = Name(properties, "UnlockItemID");
                records.Add(new VerifiedRecipeRecord(
                    recipeId,
                    outputItemId,
                    outputQuantity,
                    ingredients,
                    NonNegative(Int(properties, "WorkAmount"), "recipe work amount"),
                    unlock == "None" ? null : RequireIdentifier(unlock, "recipe unlock item")));
            }
            catch (DataPackFailure error)
            {
                throw Failure($"recipe row is invalid: {row.Key.Text}; {error.Message}");
            }
            catch (Exception error)
            {
                throw Failure(
                    $"recipe row could not be decoded: {row.Key.Text} "
                    + $"({error.GetType().Name})");
            }
        }
        return records;
    }

    private static IReadOnlyList<VerifiedPalDropRecord> ReadDrops(
        UDataTable table,
        IReadOnlyDictionary<string, string> itemIds,
        out int ignoredSentinelDropSlotCount,
        out int ignoredZeroProbabilityDropSlotCount)
    {
        ignoredSentinelDropSlotCount = 0;
        ignoredZeroProbabilityDropSlotCount = 0;
        var records = new List<VerifiedPalDropRecord>(table.RowMap.Count * 2);
        foreach (var row in table.RowMap.OrderBy(row => row.Key.Text, StringComparer.Ordinal))
        {
            var properties = Properties(row.Value);
            var sourceRowId = RequireIdentifier(row.Key.Text, "drop row");
            var palId = RequireIdentifier(Name(properties, "CharacterID"), "drop source Pal");
            var level = NonNegative(Int(properties, "Level"), "drop level");
            for (var slot = 1; slot <= 10; slot++)
            {
                var itemId = Name(properties, $"ItemId{slot}");
                var rate = Float(properties, $"Rate{slot}");
                var minimum = Int(properties, $"min{slot}");
                var maximum = Int(properties, $"Max{slot}");
                if (itemId == "None")
                {
                    if (rate != 0 || minimum != 0 || maximum != 0)
                    {
                        ignoredSentinelDropSlotCount++;
                    }
                    continue;
                }
                if (rate is < 0 or > 100 || minimum < 0 || maximum < minimum)
                {
                    throw Failure($"drop range is invalid: {sourceRowId}/{slot}");
                }
                if (rate == 0)
                {
                    ignoredZeroProbabilityDropSlotCount++;
                    continue;
                }
                var probabilityPpm = checked((int)Math.Round(
                    rate * 10_000d,
                    MidpointRounding.AwayFromZero));
                records.Add(new VerifiedPalDropRecord(
                    $"pal-drop:{sourceRowId}:{slot}",
                    sourceRowId,
                    slot,
                    palId,
                    level,
                    RequireItem(itemId, itemIds, "Pal drop"),
                    minimum,
                    maximum,
                    probabilityPpm));
            }
        }
        return records;
    }

    private static Dictionary<string, string> ReadLocalization(
        UDataTable table,
        string prefix)
    {
        // Some exact-Build item rows reference localization keys with casing that
        // differs from the localization table (for example FlameThrower versus
        // Flamethrower). Item IDs are case-insensitive in the source tables, so
        // use the same comparison here. TryAdd deliberately rejects a future
        // case-only collision instead of choosing an ambiguous translation.
        var result = new Dictionary<string, string>(
            table.RowMap.Count,
            StringComparer.OrdinalIgnoreCase);
        foreach (var row in table.RowMap)
        {
            if (!row.Key.Text.StartsWith(prefix, StringComparison.Ordinal))
            {
                continue;
            }
            var itemId = row.Key.Text[prefix.Length..];
            var text = RequiredTag(Properties(row.Value), "TextData").GetValue<FText>()?.Text;
            if (string.IsNullOrWhiteSpace(itemId)
                || string.IsNullOrWhiteSpace(text)
                || !result.TryAdd(itemId, text))
            {
                throw Failure($"localized item text is invalid: {row.Key.Text}");
            }
        }
        return result;
    }

    private static Dictionary<string, FPropertyTagType?> Properties(
        CUE4Parse.UE4.Assets.Objects.FStructFallback row) =>
        row.Properties.ToDictionary(
            property => property.Name.Text,
            property => property.Tag,
            StringComparer.Ordinal);

    private static FPropertyTagType RequiredTag(
        IReadOnlyDictionary<string, FPropertyTagType?> properties,
        string name) =>
        properties.TryGetValue(name, out var tag) && tag is not null
            ? tag
            : throw Failure($"required item property is missing: {name}");

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

    private static double Float(
        IReadOnlyDictionary<string, FPropertyTagType?> properties,
        string name)
    {
        var value = Convert.ToString(
            RequiredTag(properties, name).GenericValue,
            CultureInfo.InvariantCulture);
        return double.TryParse(
                value,
                NumberStyles.Float,
                CultureInfo.InvariantCulture,
                out var parsed)
            ? parsed
            : throw Failure($"required numeric property is invalid: {name}");
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

    private static int ScaleMilli(double value)
    {
        if (double.IsNaN(value) || double.IsInfinity(value) || value < 0)
        {
            throw Failure("item weight is invalid");
        }
        return checked((int)Math.Round(value * 1_000d, MidpointRounding.AwayFromZero));
    }

    private static string NormalizeText(string value) =>
        Whitespace().Replace(value, " ").Trim();

    private static string NormalizeDescription(
        string value,
        IReadOnlyDictionary<string, string> localizedNames)
    {
        var text = ItemNameMarkup().Replace(
            value,
            match => localizedNames.TryGetValue(match.Groups[1].Value, out var name)
                ? name
                : match.Groups[1].Value);
        text = MapObjectNameMarkup().Replace(text, "$1");
        text = OtherMarkup().Replace(text, " ");
        text = text.Replace('|', ' ');
        return NormalizeText(text);
    }

    private static string StripEnum(string value)
    {
        var separator = value.IndexOf("::", StringComparison.Ordinal);
        return separator >= 0 ? value[(separator + 2)..] : value;
    }

    private static string StripPrefix(string value, string prefix) =>
        value.StartsWith(prefix, StringComparison.Ordinal)
            ? value[prefix.Length..]
            : value;

    private static string RequireIdentifier(string value, string field)
    {
        CatalogContract.RequireIdentifier(value, field);
        return value;
    }

    private static string RequireItem(
        string itemId,
        IReadOnlyDictionary<string, string> itemIds,
        string relation)
    {
        RequireIdentifier(itemId, relation);
        return itemIds.TryGetValue(itemId, out var canonical)
            ? canonical
            : throw Failure($"{relation} references an unknown item: {itemId}");
    }

    private static int NonNegative(int value, string field) =>
        value >= 0 ? value : throw Failure($"{field} cannot be negative");

    private static int Positive(int value, string field) =>
        value > 0 ? value : throw Failure($"{field} must be positive");

    private static void RequireExactTables(AssetContract contract)
    {
        var expected = new[]
        {
            ItemsPath,
            RecipesPath,
            DropsPath,
            KoreanNamesPath,
            KoreanDescriptionsPath,
        };
        if (contract.Tables.Count != expected.Length
            || !contract.Tables.Select(table => table.PackagePath)
                .Order(StringComparer.Ordinal)
                .SequenceEqual(expected.Order(StringComparer.Ordinal), StringComparer.Ordinal)
            || contract.Tables.Any(table =>
                !table.Required || table.ExpectedRowCount is null))
        {
            throw Failure("item extraction requires the closed five-table contract");
        }
    }

    private static string AtomicWrite(string outputPath, ReadOnlySpan<byte> bytes)
    {
        var fullPath = Path.GetFullPath(outputPath);
        var directory = Path.GetDirectoryName(fullPath)
            ?? throw Failure("item catalog output directory is invalid");
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

    private static DataPackFailure Failure(string message) =>
        new(DataPackExitCode.SourceContractMismatch, message);
}
