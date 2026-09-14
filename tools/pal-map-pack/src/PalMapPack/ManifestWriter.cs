using System.Text.Json;
using System.Text.Json.Serialization;

namespace PalMapPack;

public sealed record SourceContainerDocument(
    string RelativeName,
    long SizeBytes,
    string Sha256);

public sealed record MapRegionDocument(
    string MapId,
    string RegionId,
    string SourceTexturePath,
    double WorldMinX,
    double WorldMinY,
    double WorldMaxX,
    double WorldMaxY,
    double BlockSizeX,
    double BlockSizeY,
    double GridPositionX,
    double GridPositionY,
    int Priority,
    int MapWidthPx,
    int MapHeightPx,
    string MapAssetSha256,
    double[][] WorldToMapMatrix,
    string TransformRelativePath,
    string TransformSha256,
    string TileIndexRelativePath,
    string TileSetSha256,
    string TileIndexSha256);

public sealed record MapPackManifestDocument(
    string GameBuildId,
    uint SchemaVersion,
    string SourceInputSetSha256,
    IReadOnlyList<SourceContainerDocument> SourceContainers,
    string SourceContractSha256,
    string MappingSha256,
    IReadOnlyList<MapRegionDocument> MapRegions,
    string PoisSha256,
    [property: JsonPropertyName("cue4parse_version")]
    string Cue4ParseVersion,
    uint TileCoreSizePx,
    uint TileGutterPx,
    string ExtractorVersion,
    string ExtractorCommit,
    uint CoordinateTransformVersion,
    uint PoiSchemaVersion,
    string GeneratedAt);

public sealed record CalibrationDocument(
    uint LandmarkCount,
    uint ReferenceCount,
    uint SealedHoldoutCount,
    uint ReferenceCenterCount,
    uint ReferenceNorthWestCount,
    uint ReferenceNorthEastCount,
    uint ReferenceSouthWestCount,
    uint ReferenceSouthEastCount,
    uint SealedHoldoutCenterCount,
    uint SealedHoldoutNorthWestCount,
    uint SealedHoldoutNorthEastCount,
    uint SealedHoldoutSouthWestCount,
    uint SealedHoldoutSouthEastCount,
    double ReferenceMedianErrorPx,
    double ReferenceMaxErrorPx,
    double SealedHoldoutMedianErrorPx,
    double SealedHoldoutMaxErrorPx,
    string SealedHoldoutSha256,
    IReadOnlyList<ParityPoint> ParityPoints);

public sealed record TransformDocument(
    uint SchemaVersion,
    string GameBuildId,
    string MapId,
    string RegionId,
    int MapWidthPx,
    int MapHeightPx,
    string TransformKind,
    double[][] Matrix,
    CalibrationDocument? Calibration);

public sealed record RuntimePoi(
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
    bool Verified);

public sealed record PoiDocument(
    uint SchemaVersion,
    string GameBuildId,
    uint PoiCount,
    IReadOnlyList<RuntimePoi> Pois);

public static class ManifestWriter
{
    private static readonly JsonSerializerOptions JsonOptions = new()
    {
        PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower,
        WriteIndented = true,
        UnmappedMemberHandling = JsonUnmappedMemberHandling.Disallow,
    };

    public static MapPackManifestDocument Create(
        string buildId,
        IEnumerable<SourceContainerSnapshot> sourceContainers,
        string sourceContractSha256,
        string mappingSha256,
        IReadOnlyList<MapRegionDocument> mapRegions,
        string poisSha256,
        string extractorVersion,
        string extractorCommit,
        DateTimeOffset generatedAt)
    {
        if (string.IsNullOrEmpty(buildId)
            || buildId.Any(character => !char.IsAsciiDigit(character)))
        {
            throw new MapPackFailure(ExitCodes.PackIntegrity, "manifest Build identity is invalid");
        }
        var snapshots = sourceContainers.ToArray();
        var containers = snapshots.OrderBy(item => item.RelativeName, StringComparer.Ordinal)
            .Select(item =>
            {
                Hashing.ValidateRelativeName(item.RelativeName);
                Hashing.ValidateHash(item.Sha256, "source container hash");
                if (item.SizeBytes <= 0)
                {
                    throw new MapPackFailure(ExitCodes.PackIntegrity, "source container is empty");
                }
                return new SourceContainerDocument(item.RelativeName, item.SizeBytes, item.Sha256);
            })
            .ToArray();
        if (containers.Length == 0)
        {
            throw new MapPackFailure(ExitCodes.PackIntegrity, "manifest requires the complete mounted container set");
        }
        ArgumentNullException.ThrowIfNull(mapRegions);
        var regionDocuments = mapRegions
            .OrderBy(region => region.Priority)
            .ThenBy(region => region.MapId, StringComparer.Ordinal)
            .ThenBy(region => region.RegionId, StringComparer.Ordinal)
            .Select(region =>
            {
                var expectedPrefix = RegionPackPaths.Prefix(region.MapId, region.RegionId);
                if (string.IsNullOrWhiteSpace(region.SourceTexturePath)
                    || !Finite(
                        region.WorldMinX,
                        region.WorldMinY,
                        region.WorldMaxX,
                        region.WorldMaxY,
                        region.BlockSizeX,
                        region.BlockSizeY,
                        region.GridPositionX,
                        region.GridPositionY)
                    || region.WorldMinX >= region.WorldMaxX
                    || region.WorldMinY >= region.WorldMaxY
                    || region.BlockSizeX <= 0
                    || region.BlockSizeY <= 0
                    || region.MapWidthPx is <= 0 or > 8192
                    || region.MapHeightPx is <= 0 or > 8192
                    || !ValidMatrix(region.WorldToMapMatrix)
                    || region.TransformRelativePath != $"{expectedPrefix}/transform.json"
                    || region.TileIndexRelativePath != $"{expectedPrefix}/tile-index.json")
                {
                    throw new MapPackFailure(
                        ExitCodes.PackIntegrity,
                        "manifest map region metadata or artifact association is invalid");
                }
                foreach (var (hash, field) in new[]
                {
                    (region.MapAssetSha256, "map asset"),
                    (region.TransformSha256, "transform"),
                    (region.TileSetSha256, "tile set"),
                    (region.TileIndexSha256, "tile index"),
                })
                {
                    Hashing.ValidateHash(hash, field);
                }
                return region;
            })
            .ToArray();
        if (regionDocuments.Length == 0
            || regionDocuments
            .GroupBy(
                region => $"{region.MapId}\0{region.RegionId}",
                StringComparer.OrdinalIgnoreCase)
            .Any(group => group.Count() != 1))
        {
            throw new MapPackFailure(
                ExitCodes.PackIntegrity,
                "manifest map region identifiers must be case-insensitively unique");
        }
        RegionContentPolicy.EnsureManifestNonAliasing(regionDocuments);
        var mainMaps = regionDocuments
            .Where(region => string.Equals(region.MapId, "MainMap", StringComparison.Ordinal))
            .ToArray();
        if (mainMaps.Length != 1)
        {
            throw new MapPackFailure(
                ExitCodes.PackIntegrity,
                "manifest requires exactly one explicit MainMap region");
        }
        foreach (var (hash, field) in new[]
        {
            (sourceContractSha256, "source contract"),
            (mappingSha256, "mapping"),
            (poisSha256, "POIs"),
        })
        {
            Hashing.ValidateHash(hash, field);
        }

        return new MapPackManifestDocument(
            buildId,
            2,
            Hashing.SourceInputSetSha256(snapshots),
            containers,
            sourceContractSha256,
            mappingSha256,
            regionDocuments,
            poisSha256,
            "1.2.2.202607",
            TilePyramidBuilder.TileCoreSize,
            TilePyramidBuilder.TileGutter,
            extractorVersion,
            extractorCommit,
            2,
            2,
            generatedAt.UtcDateTime.ToString(
                "yyyy-MM-dd'T'HH:mm:ss'Z'",
                System.Globalization.CultureInfo.InvariantCulture));
    }

    public static string Serialize(MapPackManifestDocument manifest) => SerializeDocument(manifest);

    public static string SerializeDocument<T>(T document) =>
        JsonSerializer.Serialize(document, JsonOptions) + "\n";

    public static byte[] SerializeBytes<T>(T document) =>
        System.Text.Encoding.UTF8.GetBytes(SerializeDocument(document));

    private static bool Finite(params double[] values) => values.All(double.IsFinite);

    private static bool ValidMatrix(double[][] matrix)
    {
        if (matrix is not { Length: 2 }
            || matrix.Any(row =>
                row is not { Length: 3 }
                || row.Any(value => !double.IsFinite(value))))
        {
            return false;
        }
        var scale = new[]
        {
            Math.Abs(matrix[0][0]),
            Math.Abs(matrix[0][1]),
            Math.Abs(matrix[1][0]),
            Math.Abs(matrix[1][1]),
        }.Max();
        if (scale == 0)
        {
            return false;
        }
        var determinant =
            matrix[0][0] / scale * (matrix[1][1] / scale)
            - matrix[0][1] / scale * (matrix[1][0] / scale);
        return double.IsFinite(determinant)
            && Math.Abs(determinant) > 1e-12;
    }
}

internal static class RegionContentPolicy
{
    public static void EnsureDecodedNonAliasing(MapRegionSet regions)
    {
        var main = Find(
            regions.All,
            region => region.MapId == "MainMap" && region.RegionId == "FirstRegion");
        var tree = Find(
            regions.All,
            region => region.MapId == "Tree" && region.RegionId == "DummyRegion");
        if (Hashing.MapAssetSha256(main.Map) == Hashing.MapAssetSha256(tree.Map))
        {
            throw Failure();
        }
    }

    public static void EnsureManifestNonAliasing(IReadOnlyList<MapRegionDocument> regions)
    {
        if (regions.Count != 2)
        {
            throw Failure();
        }
        var main = Find(
            regions,
            region => region.MapId == "MainMap" && region.RegionId == "FirstRegion");
        var tree = Find(
            regions,
            region => region.MapId == "Tree" && region.RegionId == "DummyRegion");
        if (!Authoritative(
                main,
                "/Game/Pal/Texture/UI/Map/T_WorldMap.T_WorldMap",
                -1_099_400,
                -724_400,
                349_400,
                724_400,
                0)
            || !Authoritative(
                tree,
                "/Game/Pal/Texture/UI/Map/T_TreeMap.T_TreeMap",
                347_351.5,
                -818_197,
                689_148.5,
                -476_400,
                1)
            || main.MapAssetSha256 == tree.MapAssetSha256
            || main.TileSetSha256 == tree.TileSetSha256)
        {
            throw Failure();
        }
    }

    private static bool Authoritative(
        MapRegionDocument region,
        string texturePath,
        double minX,
        double minY,
        double maxX,
        double maxY,
        int priority) =>
        region.SourceTexturePath == texturePath
        && region.WorldMinX == minX
        && region.WorldMinY == minY
        && region.WorldMaxX == maxX
        && region.WorldMaxY == maxY
        && region.BlockSizeX == 1.0
        && region.BlockSizeY == 1.0
        && region.GridPositionX == 0.0
        && region.GridPositionY == 0.0
        && region.Priority == priority;

    private static T Find<T>(IEnumerable<T> regions, Func<T, bool> predicate)
    {
        var matches = regions.Where(predicate).ToArray();
        return matches.Length == 1 ? matches[0] : throw Failure();
    }

    private static MapPackFailure Failure() =>
        new(
            ExitCodes.PackIntegrity,
            "authoritative MainMap and Tree rows or content are invalid");
}

public static class RegionPackPaths
{
    public static string Prefix(string mapId, string regionId)
    {
        ValidateIdentifier(mapId, "map");
        ValidateIdentifier(regionId, "region");
        return $"regions/{mapId.ToLowerInvariant()}/{regionId.ToLowerInvariant()}";
    }

    private static void ValidateIdentifier(string value, string field)
    {
        if (string.IsNullOrEmpty(value)
            || value.Any(character => !char.IsAsciiLetterOrDigit(character)))
        {
            throw new MapPackFailure(
                ExitCodes.PackIntegrity,
                $"{field} identifier is unsafe");
        }
    }
}
