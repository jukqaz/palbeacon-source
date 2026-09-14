using System.Globalization;
using CUE4Parse.FileProvider;
using CUE4Parse.UE4.Assets.Exports;
using CUE4Parse.UE4.Assets.Exports.Engine;
using CUE4Parse.UE4.Objects.Core.i18N;
using CUE4Parse.UE4.Objects.Core.Math;
using CUE4Parse.UE4.Objects.Core.Misc;
using CUE4Parse.UE4.Objects.UObject;

namespace PalMapPack;

internal sealed record Cue4ParsePoiReadResult(
    IReadOnlyList<RawPoiRow> Rows,
    PoiProbeSummary Summary);

/// <summary>
/// The only CUE4Parse-specific boundary for Build-pinned POI extraction.
/// All source-shape and accounting drift is rejected before a map pack can be built.
/// </summary>
internal static class Cue4ParsePoiAdapter
{
    public static Cue4ParsePoiReadResult Read(
        DefaultFileProvider provider,
        PoiAssetContract contract)
    {
        ArgumentNullException.ThrowIfNull(provider);
        ArgumentNullException.ThrowIfNull(contract);
        ValidateContract(contract);

        var placed = ReadPlacedWorldPois(provider, contract, out var worldExportCount);
        var bosses = ReadBossPois(provider, contract);
        var rows = placed.PoiRows.Concat(bosses.Rows)
            .OrderBy(row => row.RowKey, StringComparer.Ordinal)
            .ToArray();
        if (rows.Select(row => row.RowKey).Distinct(StringComparer.Ordinal).Count() != rows.Length)
        {
            throw Failure("POI adapters produced duplicate global row identities");
        }

        return new Cue4ParsePoiReadResult(
            rows,
            new PoiProbeSummary(
                worldExportCount,
                contract.ExpectedFastTravelTextRowCount,
                placed.FastTravelCount,
                placed.DungeonPortalCount,
                bosses.Normalized.RawRowCount,
                bosses.Normalized.Pois.Count,
                bosses.Normalized.Pois.Count(poi => poi.Kind == BossPoiKind.PalFieldBoss),
                bosses.Normalized.Pois.Count(poi => poi.Kind == BossPoiKind.HumanFieldBoss),
                bosses.Normalized.Pois.Count(poi => poi.Kind == BossPoiKind.BossRegion),
                bosses.Normalized.DuplicateRowCount));
    }

    private static PlacedWorldPoiResult ReadPlacedWorldPois(
        DefaultFileProvider provider,
        PoiAssetContract contract,
        out int worldExportCount)
    {
        var package = provider.LoadPackage(contract.WorldPackagePath);
        var exports = package.GetExports().ToArray();
        worldExportCount = exports.Length;
        if (worldExportCount != contract.ExpectedWorldExportCount)
        {
            throw Failure("placed-world export count does not match the exact-Build contract");
        }

        var actors = new List<PlacedWorldActorSource>(
            contract.ExpectedFastTravelCount + contract.ExpectedDungeonCount);
        var components = new List<PlacedWorldComponentSource>(
            contract.ExpectedFastTravelCount + contract.ExpectedDungeonCount);
        foreach (var actor in exports)
        {
            if (!string.Equals(
                    actor.ExportType,
                    PlacedWorldPoiReader.FastTravelExportType,
                    StringComparison.Ordinal)
                && !PlacedWorldPoiReader.DungeonExportTypes.Contains(actor.ExportType))
            {
                continue;
            }

            var rootIndex = RequiredProperty(actor, "RootComponent")
                .GetValue<FPackageIndex>();
            if (rootIndex is null
                || rootIndex.IsNull
                || !rootIndex.TryLoad(out UObject? rootComponent)
                || rootComponent is null)
            {
                throw Failure("placed-world RootComponent package index did not resolve");
            }
            var rootReference = rootIndex.Index.ToString(CultureInfo.InvariantCulture);
            var location = RequiredProperty(rootComponent, "RelativeLocation")
                .GetValue<FVector>();
            components.Add(new PlacedWorldComponentSource(
                rootReference,
                new Dictionary<string, PlacedWorldPoiSourceValue>(StringComparer.Ordinal)
                {
                    ["RelativeLocation"] = new PlacedWorldPoiVectorValue(
                        location.X,
                        location.Y,
                        location.Z),
                }));

            var properties = new Dictionary<string, PlacedWorldPoiSourceValue>(
                StringComparer.Ordinal)
            {
                ["RootComponent"] = new PlacedWorldPoiObjectReferenceValue(rootReference),
            };
            if (string.Equals(
                    actor.ExportType,
                    PlacedWorldPoiReader.FastTravelExportType,
                    StringComparison.Ordinal))
            {
                properties["FastTravelPointID"] = new PlacedWorldPoiNameValue(
                    RequiredProperty(actor, "FastTravelPointID").GetValue<FName>().Text);
            }
            else
            {
                var sourceGuid = RequiredProperty(actor, "LevelObjectInstanceId")
                    .GetValue<FGuid>();
                if (!sourceGuid.IsValid()
                    || !Guid.TryParseExact(sourceGuid.ToString(), "N", out var guid))
                {
                    throw Failure("dungeon LevelObjectInstanceId did not decode as canonical FGuid");
                }
                properties["LevelObjectInstanceId"] = new PlacedWorldPoiGuidValue(guid);
            }
            actors.Add(new PlacedWorldActorSource(
                actor.Name,
                actor.ExportType,
                properties));
        }

        var fastTravelNames = ReadExactTextTablePair(
            provider,
            contract.FastTravelNameTablePath,
            contract.FastTravelLocalizedNameTablePath,
            contract.ExpectedFastTravelTextRowCount);
        return PlacedWorldPoiReader.Normalize(
            actors,
            components,
            new TextDictionaryFastTravelResolver(fastTravelNames),
            new PlacedWorldPoiContract(
                contract.WorldPackagePath,
                contract.ExpectedFastTravelCount,
                contract.ExpectedDungeonCount,
                contract.DungeonDisplayName));
    }

    private static BossReadResult ReadBossPois(
        DefaultFileProvider provider,
        PoiAssetContract contract)
    {
        var table = provider.LoadPackageObject<UDataTable>(contract.FieldBossTablePath);
        if (table.RowMap.Count != contract.ExpectedBossRawCount)
        {
            throw Failure("boss POI raw row count does not match the exact-Build contract");
        }
        var sourceRows = table.RowMap.Select(row =>
        {
            var properties = row.Value.Properties.ToDictionary(
                property => property.Name.Text,
                property => property.Tag,
                StringComparer.Ordinal);
            var expectedProperties = new[]
            {
                "CharacterID",
                "Level",
                "Location",
                "SpawnerID",
            };
            if (properties.Count != expectedProperties.Length
                || !properties.Keys.Order(StringComparer.Ordinal)
                    .SequenceEqual(expectedProperties, StringComparer.Ordinal))
            {
                throw Failure("boss POI source row does not have the exact four-property schema");
            }
            return new BossPoiSourceRow(
                row.Key.Text,
                new Dictionary<string, BossPoiSourceValue>(StringComparer.Ordinal)
                {
                    ["SpawnerID"] = new BossPoiNameValue(
                        RequiredTag(properties, "SpawnerID").GetValue<FName>().Text),
                    ["CharacterID"] = new BossPoiNameValue(
                        RequiredTag(properties, "CharacterID").GetValue<FName>().Text),
                    ["Location"] = ToBossVector(
                        RequiredTag(properties, "Location").GetValue<FVector>()),
                    ["Level"] = new BossPoiIntegerValue(
                        RequiredTag(properties, "Level").GetValue<int>()),
                });
        }).ToArray();

        var localizedNames = new BossTextResolver(
            ReadTextTablePair(
                provider,
                contract.PalNameTablePath,
                contract.PalLocalizedNameTablePath),
            ReadTextTablePair(
                provider,
                contract.HumanNameTablePath,
                contract.HumanLocalizedNameTablePath),
            ReadTextTablePair(
                provider,
                contract.WorldMapNameTablePath,
                contract.WorldMapLocalizedNameTablePath));
        var normalized = BossPoiTableReader.Normalize(sourceRows, localizedNames);
        ValidateBossCounts(normalized, contract);

        var rows = new List<RawPoiRow>(normalized.RawRowCount);
        foreach (var poi in normalized.Pois)
        {
            if (poi.Kind == BossPoiKind.BossRegion)
            {
                rows.Add(ToExcludedBossRow(
                    "boss-region:" + poi.LogicalId,
                    "BossRegion",
                    poi,
                    poi.RawRowKeys[0],
                    "boss-region-outside-v1-filter",
                    contract.FieldBossTablePath));
            }
            else
            {
                rows.Add(new RawPoiRow(
                    contract.FieldBossTablePath,
                    poi.LogicalId,
                    poi.Kind == BossPoiKind.HumanFieldBoss ? "WantedTarget" : "FieldBoss",
                    poi.DisplayName,
                    poi.WorldX,
                    poi.WorldY,
                    null,
                    true,
                    poi.RawRowKeys,
                    PalSpeciesId(poi)));
            }

            foreach (var duplicateRowKey in poi.RawRowKeys.Skip(1))
            {
                rows.Add(ToExcludedBossRow(
                    $"boss-duplicate:{poi.LogicalId}:{duplicateRowKey}",
                    "FieldBossDuplicate",
                    poi,
                    duplicateRowKey,
                    "exact-semantic-duplicate",
                    contract.FieldBossTablePath));
            }
        }
        if (rows.Count != normalized.RawRowCount)
        {
            throw Failure("boss POI raw-to-semantic accounting is incomplete");
        }
        return new BossReadResult(rows, normalized);
    }

    private static RawPoiRow ToExcludedBossRow(
        string rowKey,
        string kind,
        NormalizedBossPoi poi,
        string sourceRowKey,
        string reason,
        string sourceAssetPath) =>
        new(
            sourceAssetPath,
            rowKey,
            kind,
            poi.DisplayName,
            poi.WorldX,
            poi.WorldY,
            reason,
            true,
            [sourceRowKey]);

    private static BossPoiVectorValue ToBossVector(FVector location) =>
        new(location.X, location.Y, location.Z);

    private static string? PalSpeciesId(NormalizedBossPoi poi)
    {
        const string bossPrefix = "BOSS_";
        return poi.Kind == BossPoiKind.PalFieldBoss
            && poi.CharacterId.StartsWith(bossPrefix, StringComparison.OrdinalIgnoreCase)
            && poi.CharacterId.Length > bossPrefix.Length
                ? poi.CharacterId[bossPrefix.Length..]
                : null;
    }

    private static Dictionary<string, string> ReadExactTextTablePair(
        DefaultFileProvider provider,
        string basePath,
        string localizedPath,
        int expectedRowCount)
    {
        var baseRows = ReadTextTable(provider, basePath);
        var localizedRows = ReadTextTable(provider, localizedPath);
        if (baseRows.Count != expectedRowCount
            || localizedRows.Count != expectedRowCount
            || !baseRows.Keys.ToHashSet(StringComparer.Ordinal)
                .SetEquals(localizedRows.Keys))
        {
            throw Failure("localized text table pair does not match the exact-Build contract");
        }
        return MergeTextTables(baseRows, localizedRows);
    }

    private static Dictionary<string, string> ReadTextTablePair(
        DefaultFileProvider provider,
        string basePath,
        string localizedPath)
    {
        var baseRows = ReadTextTable(provider, basePath);
        var localizedRows = ReadTextTable(provider, localizedPath);
        if (baseRows.Count == 0
            || localizedRows.Count == 0
            || !baseRows.Keys.ToHashSet(StringComparer.Ordinal)
                .SetEquals(localizedRows.Keys))
        {
            throw Failure("boss localized text table pair has incompatible row identities");
        }
        return MergeTextTables(baseRows, localizedRows);
    }

    private static Dictionary<string, string> ReadTextTable(
        DefaultFileProvider provider,
        string packagePath)
    {
        var table = provider.LoadPackageObject<UDataTable>(packagePath);
        var rows = new Dictionary<string, string>(table.RowMap.Count, StringComparer.Ordinal);
        foreach (var row in table.RowMap)
        {
            var matching = row.Value.Properties.Where(property => string.Equals(
                property.Name.Text,
                "TextData",
                StringComparison.Ordinal)).ToArray();
            var tag = matching.Length == 1 ? matching[0].Tag : null;
            var text = tag?.GetValue<FText>();
            if (text is null
                || !rows.TryAdd(row.Key.Text, text.Text))
            {
                throw Failure("localized text table row does not match the TextData contract");
            }
        }
        return rows;
    }

    private static Dictionary<string, string> MergeTextTables(
        IReadOnlyDictionary<string, string> baseRows,
        IReadOnlyDictionary<string, string> localizedRows)
    {
        var merged = new Dictionary<string, string>(baseRows.Count, StringComparer.Ordinal);
        foreach (var (key, baseValue) in baseRows)
        {
            var value = localizedRows.TryGetValue(key, out var localized)
                && !string.IsNullOrWhiteSpace(localized)
                    ? localized
                    : baseValue;
            if (string.IsNullOrWhiteSpace(value))
            {
                throw Failure("localized text table contains an unresolved display name");
            }
            merged.Add(key, value);
        }
        return merged;
    }

    private static CUE4Parse.UE4.Assets.Objects.Properties.FPropertyTagType RequiredProperty(
        UObject export,
        string propertyName)
    {
        var matching = export.Properties.Where(property =>
            string.Equals(property.Name.Text, propertyName, StringComparison.Ordinal)).ToArray();
        var tag = matching.Length == 1 ? matching[0].Tag : null;
        return tag ?? throw Failure($"required export property is missing: {propertyName}");
    }

    private static CUE4Parse.UE4.Assets.Objects.Properties.FPropertyTagType RequiredTag(
        IReadOnlyDictionary<string, CUE4Parse.UE4.Assets.Objects.Properties.FPropertyTagType?> properties,
        string propertyName) =>
        properties.TryGetValue(propertyName, out var tag) && tag is not null
            ? tag
            : throw Failure($"required table property is missing: {propertyName}");

    private static void ValidateContract(PoiAssetContract contract)
    {
        AssetCatalogProbe.ValidatePoiSources(contract);
        if (!string.Equals(
                contract.WorldPackagePath,
                PlacedWorldPoiReader.PlacementPackagePath,
                StringComparison.Ordinal)
            || contract.DungeonPortalExportTypes.Count
                != PlacedWorldPoiReader.DungeonExportTypes.Count
            || !contract.DungeonPortalExportTypes
                .ToHashSet(StringComparer.Ordinal)
                .SetEquals(PlacedWorldPoiReader.DungeonExportTypes))
        {
            throw Failure("POI exact-Build contract is invalid");
        }
    }

    private static void ValidateBossCounts(
        BossPoiTableResult normalized,
        PoiAssetContract contract)
    {
        if (normalized.RawRowCount != contract.ExpectedBossRawCount
            || normalized.Pois.Count != contract.ExpectedBossSemanticCount
            || normalized.DuplicateRowCount != contract.ExpectedBossDuplicateCount
            || normalized.Pois.Count(poi => poi.Kind == BossPoiKind.PalFieldBoss)
                != contract.ExpectedPalBossCount
            || normalized.Pois.Count(poi => poi.Kind == BossPoiKind.HumanFieldBoss)
                != contract.ExpectedHumanBossCount
            || normalized.Pois.Count(poi => poi.Kind == BossPoiKind.BossRegion)
                != contract.ExpectedBossRegionCount)
        {
            throw Failure("boss POI semantic counts do not match the exact-Build contract");
        }
    }

    private static MapPackFailure Failure(string message) =>
        new(ExitCodes.AssetContractMismatch, message);

    private sealed class TextDictionaryFastTravelResolver(
        IReadOnlyDictionary<string, string> rows) : IPlacedWorldFastTravelNameResolver
    {
        public string? Resolve(string fastTravelPointId) =>
            rows.TryGetValue(fastTravelPointId, out var value) ? value : null;
    }

    private sealed class BossTextResolver(
        IReadOnlyDictionary<string, string> palNames,
        IReadOnlyDictionary<string, string> humanNames,
        IReadOnlyDictionary<string, string> worldMapNames) : IBossPoiLocalizedNameResolver
    {
        public string? Resolve(BossPoiNameTable table, string key)
        {
            var source = table switch
            {
                BossPoiNameTable.Pal => palNames,
                BossPoiNameTable.Human => humanNames,
                BossPoiNameTable.WorldMap => worldMapNames,
                _ => throw Failure("boss POI requested an unknown localized text table"),
            };
            return source.TryGetValue(key, out var value) ? value : null;
        }
    }

    private sealed record BossReadResult(
        IReadOnlyList<RawPoiRow> Rows,
        BossPoiTableResult Normalized);
}
