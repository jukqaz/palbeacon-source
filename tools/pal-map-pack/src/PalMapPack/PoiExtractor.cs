namespace PalMapPack;

public sealed record PoiRecord(
    string Id,
    string Kind,
    string DisplayName,
    string? EntityId,
    string MapId,
    string RegionId,
    double WorldX,
    double WorldY,
    double MapX,
    double MapY,
    string SourceBuildId,
    bool Verified,
    string SourceAssetPath,
    string SourceRowKey,
    IReadOnlyList<string> SourceRowKeys,
    string ExtractionRule);

public sealed record PoiSourceAccounting(
    string SourceAssetPath,
    int SourceTotal,
    int Extracted,
    int ExplicitlyExcluded,
    int Invalid,
    int Duplicate,
    int Unaccounted);

public sealed record PoiExtractionResult(
    IReadOnlyList<PoiRecord> Pois,
    IReadOnlyList<PoiSourceAccounting> SourceAccounting);

public static class PoiExtractor
{
    private static readonly IReadOnlyDictionary<string, (string Kind, string Rule)> Rules =
        new Dictionary<string, (string, string)>(StringComparer.Ordinal)
        {
            ["PointFastTravel"] = ("fast_travel", "main-world-actor:BP_LevelObject_TowerFastTravelPoint_C:v1"),
            ["PointDungeonPortal"] = ("dungeon", "main-world-actor:dungeon-portal-whitelist:v1"),
            ["FieldBoss"] = ("boss", "boss-location-table:semantic-dedup:v1"),
            ["WantedTarget"] = ("wanted", "boss-location-table:human-target-semantic-dedup:v1"),
        };

    public static PoiExtractionResult Extract(
        IEnumerable<RawPoiRow> sourceRows,
        MapRegionSet regions,
        IReadOnlyDictionary<(string MapId, string RegionId), double[][]> transforms,
        string buildId)
    {
        ArgumentNullException.ThrowIfNull(sourceRows);
        ArgumentNullException.ThrowIfNull(regions);
        ArgumentNullException.ThrowIfNull(transforms);
        if (string.IsNullOrEmpty(buildId) || buildId.Any(character => !char.IsAsciiDigit(character)))
        {
            throw Failure("POI source Build identity is invalid");
        }

        var rows = sourceRows.ToArray();
        var seen = new HashSet<string>(StringComparer.Ordinal);
        var pois = new List<PoiRecord>();
        var accounting = rows.GroupBy(row => row.SourceAssetPath, StringComparer.Ordinal)
            .ToDictionary(
                group => group.Key,
                group => new MutableAccounting(group.Count()),
                StringComparer.Ordinal);

        foreach (var row in rows)
        {
            if (string.IsNullOrWhiteSpace(row.SourceAssetPath)
                || string.IsNullOrWhiteSpace(row.RowKey)
                || string.IsNullOrWhiteSpace(row.DisplayName)
                || !double.IsFinite(row.WorldX)
                || !double.IsFinite(row.WorldY))
            {
                throw Failure("POI row is invalid; invalid rows cannot be silently dropped");
            }
            var sourceRowKeys = row.SourceRowKeys ?? [row.RowKey];
            if (sourceRowKeys.Count == 0
                || sourceRowKeys.Any(string.IsNullOrWhiteSpace)
                || sourceRowKeys.Distinct(StringComparer.Ordinal).Count() != sourceRowKeys.Count)
            {
                throw Failure("POI source row provenance is missing or duplicated");
            }
            if (!seen.Add(row.RowKey))
            {
                accounting[row.SourceAssetPath].Duplicate++;
                throw Failure("duplicate POI row key");
            }

            if (!Rules.TryGetValue(row.Kind, out var rule))
            {
                if (string.IsNullOrWhiteSpace(row.ExplicitExclusionReason))
                {
                    accounting[row.SourceAssetPath].Unaccounted++;
                    throw Failure("unknown POI source kind is not explicitly excluded");
                }
                accounting[row.SourceAssetPath].ExplicitlyExcluded++;
                continue;
            }

            if (!string.IsNullOrWhiteSpace(row.ExplicitExclusionReason))
            {
                throw Failure("approved v1 POI source kind cannot be silently excluded");
            }
            if ((row.Kind == "FieldBoss" || row.Kind == "WantedTarget") && !row.CrossChecked)
            {
                throw Failure("field boss row was not cross-checked against the boss table");
            }

            var region = regions.Select(row.WorldX, row.WorldY);
            if (!transforms.TryGetValue((region.MapId, region.RegionId), out var transform))
            {
                throw Failure("selected map region has no bound coordinate transform");
            }
            var projected = CoordinateSolver.Project(transform, row.WorldX, row.WorldY);
            if (projected.X < 0
                || projected.X > region.Map.Width
                || projected.Y < 0
                || projected.Y > region.Map.Height)
            {
                throw Failure("projected POI is outside its selected map region");
            }
            pois.Add(new PoiRecord(
                row.RowKey,
                rule.Kind,
                row.DisplayName,
                row.EntityId,
                region.MapId,
                region.RegionId,
                row.WorldX,
                row.WorldY,
                projected.X,
                projected.Y,
                buildId,
                true,
                row.SourceAssetPath,
                sourceRowKeys[0],
                sourceRowKeys.ToArray(),
                rule.Rule));
            accounting[row.SourceAssetPath].Extracted++;
        }

        var required = new[] { "fast_travel", "boss", "dungeon" };
        if (required.Any(kind => pois.All(poi => poi.Kind != kind)))
        {
            throw Failure("every v1 POI kind must contain at least one extracted row");
        }

        var outputAccounting = accounting.OrderBy(pair => pair.Key, StringComparer.Ordinal)
            .Select(pair => pair.Value.ToImmutable(pair.Key))
            .ToArray();
        if (outputAccounting.Any(item =>
                item.SourceTotal != item.Extracted + item.ExplicitlyExcluded
                || item.Invalid != 0
                || item.Duplicate != 0
                || item.Unaccounted != 0))
        {
            throw Failure("POI source accounting is incomplete");
        }

        return new PoiExtractionResult(
            pois.OrderBy(poi => poi.Kind, StringComparer.Ordinal)
                .ThenBy(poi => poi.Id, StringComparer.Ordinal)
                .ToArray(),
            outputAccounting);
    }

    private static MapPackFailure Failure(string message) =>
        new(ExitCodes.AssetContractMismatch, message);

    private sealed class MutableAccounting(int sourceTotal)
    {
        public int Extracted { get; set; }
        public int ExplicitlyExcluded { get; set; }
        public int Invalid { get; set; }
        public int Duplicate { get; set; }
        public int Unaccounted { get; set; }

        public PoiSourceAccounting ToImmutable(string source) =>
            new(source, sourceTotal, Extracted, ExplicitlyExcluded, Invalid, Duplicate, Unaccounted);
    }
}
