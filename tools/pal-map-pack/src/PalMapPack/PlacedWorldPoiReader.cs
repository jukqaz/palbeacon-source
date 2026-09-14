using System.Collections.Frozen;

namespace PalMapPack;

/// <summary>
/// Transport-neutral values accepted from an exact-Build placed-world adapter.
/// CUE4Parse exports are decoded into these values before normalization.
/// </summary>
public abstract record PlacedWorldPoiSourceValue;

public sealed record PlacedWorldPoiNameValue(string Text) : PlacedWorldPoiSourceValue;

public sealed record PlacedWorldPoiGuidValue(Guid Value) : PlacedWorldPoiSourceValue;

public sealed record PlacedWorldPoiObjectReferenceValue(string ObjectReference) : PlacedWorldPoiSourceValue;

public sealed record PlacedWorldPoiVectorValue(double X, double Y, double Z) : PlacedWorldPoiSourceValue;

public sealed record PlacedWorldActorSource(
    string RawExportName,
    string ExportType,
    IReadOnlyDictionary<string, PlacedWorldPoiSourceValue> Properties);

public sealed record PlacedWorldComponentSource(
    string ObjectReference,
    IReadOnlyDictionary<string, PlacedWorldPoiSourceValue> Properties);

public interface IPlacedWorldFastTravelNameResolver
{
    string? Resolve(string fastTravelPointId);
}

public sealed record PlacedWorldPoiContract(
    string SourceAssetPath,
    int ExpectedFastTravelCount,
    int ExpectedDungeonPortalCount,
    string GenericDungeonDisplayName)
{
    public static PlacedWorldPoiContract ForBuild24181527(string genericDungeonDisplayName) =>
        new(
            PlacedWorldPoiReader.PlacementPackagePath,
            PlacedWorldPoiReader.Build24181527FastTravelCount,
            PlacedWorldPoiReader.Build24181527DungeonPortalCount,
            genericDungeonDisplayName);
}

public sealed record PlacedWorldPoiResult(
    IReadOnlyList<RawPoiRow> PoiRows,
    int FastTravelCount,
    int DungeonPortalCount);

/// <summary>
/// Fail-closed normalization for fast-travel and dungeon actors observed in Build 24181527.
/// </summary>
public static class PlacedWorldPoiReader
{
    public const string PlacementPackagePath = "Pal/Content/Pal/Maps/MainWorld_5/PL_MainWorld5";
    public const string FastTravelExportType = "BP_LevelObject_TowerFastTravelPoint_C";
    public const int Build24181527FastTravelCount = 152;
    public const int Build24181527DungeonPortalCount = 157;

    public static IReadOnlySet<string> DungeonExportTypes { get; } = new[]
    {
        "BP_DungeonPortalMarker_Desert_C",
        "BP_DungeonPortalMarker_Forest_C",
        "BP_DungeonPortalMarker_Grass1_C",
        "BP_DungeonPortalMarker_Sakura_C",
        "BP_DungeonPortalMarker_Skyland_C",
        "BP_DungeonPortalMarker_Snow_C",
        "BP_DungeonPortalMarker_Viking_B_C",
        "BP_DungeonPortalMarker_Viking_C",
        "BP_DungeonPortalMarker_Viking_C_C",
        "BP_DungeonPortalMarker_Volcano_C",
        "BP_DungeonPortalMarker_Yakushima_C",
    }.ToFrozenSet(StringComparer.Ordinal);

    public static PlacedWorldPoiResult Normalize(
        IEnumerable<PlacedWorldActorSource> actors,
        IEnumerable<PlacedWorldComponentSource> components,
        IPlacedWorldFastTravelNameResolver localizedFastTravelNames,
        PlacedWorldPoiContract contract)
    {
        ArgumentNullException.ThrowIfNull(actors);
        ArgumentNullException.ThrowIfNull(components);
        ArgumentNullException.ThrowIfNull(localizedFastTravelNames);
        ArgumentNullException.ThrowIfNull(contract);
        ValidateContract(contract);

        var componentsByReference = IndexComponents(components);
        var fastTravel = new List<DecodedPlacedPoi>();
        var dungeons = new List<DecodedPlacedPoi>();
        var seenFastTravelIds = new HashSet<string>(StringComparer.Ordinal);
        var seenDungeonIds = new HashSet<Guid>();
        var seenRootComponents = new HashSet<string>(StringComparer.Ordinal);
        var seenCoordinates = new HashSet<CoordinateKey>();

        foreach (var actor in actors)
        {
            if (actor is null
                || string.IsNullOrWhiteSpace(actor.RawExportName)
                || string.IsNullOrWhiteSpace(actor.ExportType))
            {
                throw Failure("placed-world actor identity is missing");
            }

            var isFastTravel = string.Equals(
                actor.ExportType,
                FastTravelExportType,
                StringComparison.Ordinal);
            var isDungeon = DungeonExportTypes.Contains(actor.ExportType);
            if (!isFastTravel && !isDungeon)
            {
                continue;
            }

            var rootReference = RequiredValue<PlacedWorldPoiObjectReferenceValue>(
                actor.Properties,
                "RootComponent",
                "placed-world actor RootComponent is missing or has the wrong type");
            if (string.IsNullOrWhiteSpace(rootReference.ObjectReference)
                || !seenRootComponents.Add(rootReference.ObjectReference))
            {
                throw Failure("placed-world actor RootComponent reference is missing or duplicated");
            }
            if (!componentsByReference.TryGetValue(rootReference.ObjectReference, out var rootComponent))
            {
                throw Failure("placed-world actor RootComponent did not resolve exactly once");
            }

            var relativeLocation = RequiredValue<PlacedWorldPoiVectorValue>(
                rootComponent.Properties,
                "RelativeLocation",
                "placed-world root component RelativeLocation is missing or has the wrong type");
            if (!double.IsFinite(relativeLocation.X)
                || !double.IsFinite(relativeLocation.Y)
                || !double.IsFinite(relativeLocation.Z))
            {
                throw Failure("placed-world root component RelativeLocation is non-finite");
            }
            var coordinate = CoordinateKey.Create(relativeLocation);
            if (!seenCoordinates.Add(coordinate))
            {
                throw Failure("placed-world POI coordinates are duplicated");
            }

            if (isFastTravel)
            {
                var fastTravelPointId = RequiredValue<PlacedWorldPoiNameValue>(
                    actor.Properties,
                    "FastTravelPointID",
                    "fast-travel actor FastTravelPointID is missing or has the wrong type");
                if (string.IsNullOrWhiteSpace(fastTravelPointId.Text)
                    || !seenFastTravelIds.Add(fastTravelPointId.Text))
                {
                    throw Failure("fast-travel FastTravelPointID is missing or duplicated");
                }
                fastTravel.Add(new DecodedPlacedPoi(
                    "fast-travel:" + fastTravelPointId.Text,
                    "PointFastTravel",
                    fastTravelPointId.Text,
                    actor.RawExportName,
                    relativeLocation.X,
                    relativeLocation.Y));
                continue;
            }

            var dungeonId = RequiredValue<PlacedWorldPoiGuidValue>(
                actor.Properties,
                "LevelObjectInstanceId",
                "dungeon actor LevelObjectInstanceId FGuid is missing or has the wrong type");
            if (dungeonId.Value == Guid.Empty || !seenDungeonIds.Add(dungeonId.Value))
            {
                throw Failure("dungeon LevelObjectInstanceId FGuid is empty or duplicated");
            }
            dungeons.Add(new DecodedPlacedPoi(
                "dungeon:" + dungeonId.Value.ToString("N"),
                "PointDungeonPortal",
                null,
                actor.RawExportName,
                relativeLocation.X,
                relativeLocation.Y));
        }

        if (fastTravel.Count != contract.ExpectedFastTravelCount
            || dungeons.Count != contract.ExpectedDungeonPortalCount)
        {
            throw Failure("placed-world POI counts do not match the exact-Build contract");
        }

        var rows = new List<RawPoiRow>(fastTravel.Count + dungeons.Count);
        foreach (var poi in fastTravel)
        {
            var displayName = localizedFastTravelNames.Resolve(poi.NameLookupKey!);
            if (string.IsNullOrWhiteSpace(displayName))
            {
                throw Failure("fast-travel localized display-name coverage is incomplete");
            }
            rows.Add(ToRawPoiRow(poi, displayName, contract.SourceAssetPath));
        }
        rows.AddRange(dungeons.Select(poi =>
            ToRawPoiRow(poi, contract.GenericDungeonDisplayName, contract.SourceAssetPath)));

        return new PlacedWorldPoiResult(
            rows.OrderBy(row => row.RowKey, StringComparer.Ordinal).ToArray(),
            fastTravel.Count,
            dungeons.Count);
    }

    private static Dictionary<string, PlacedWorldComponentSource> IndexComponents(
        IEnumerable<PlacedWorldComponentSource> components)
    {
        var indexed = new Dictionary<string, PlacedWorldComponentSource>(StringComparer.Ordinal);
        foreach (var component in components)
        {
            if (component is null
                || string.IsNullOrWhiteSpace(component.ObjectReference)
                || component.Properties is null
                || !indexed.TryAdd(component.ObjectReference, component))
            {
                throw Failure("placed-world component reference is missing or duplicated");
            }
        }
        return indexed;
    }

    private static T RequiredValue<T>(
        IReadOnlyDictionary<string, PlacedWorldPoiSourceValue>? properties,
        string propertyName,
        string failureMessage)
        where T : PlacedWorldPoiSourceValue
    {
        if (properties is null
            || !properties.TryGetValue(propertyName, out var value)
            || value is not T typedValue)
        {
            throw Failure(failureMessage);
        }
        return typedValue;
    }

    private static RawPoiRow ToRawPoiRow(
        DecodedPlacedPoi poi,
        string displayName,
        string sourceAssetPath) =>
        new(
            sourceAssetPath,
            poi.RowKey,
            poi.Kind,
            displayName,
            poi.WorldX,
            poi.WorldY,
            null,
            true,
            [poi.RawExportName]);

    private static void ValidateContract(PlacedWorldPoiContract contract)
    {
        if (!string.Equals(contract.SourceAssetPath, PlacementPackagePath, StringComparison.Ordinal)
            || contract.ExpectedFastTravelCount < 0
            || contract.ExpectedDungeonPortalCount < 0
            || string.IsNullOrWhiteSpace(contract.GenericDungeonDisplayName))
        {
            throw Failure("placed-world POI contract is invalid");
        }
    }

    private static MapPackFailure Failure(string message) =>
        new(ExitCodes.AssetContractMismatch, message);

    private sealed record DecodedPlacedPoi(
        string RowKey,
        string Kind,
        string? NameLookupKey,
        string RawExportName,
        double WorldX,
        double WorldY);

    private sealed record CoordinateKey(long XBits, long YBits, long ZBits)
    {
        public static CoordinateKey Create(PlacedWorldPoiVectorValue location) =>
            new(CanonicalBits(location.X), CanonicalBits(location.Y), CanonicalBits(location.Z));

        private static long CanonicalBits(double value) =>
            BitConverter.DoubleToInt64Bits(value == 0 ? 0 : value);
    }
}
