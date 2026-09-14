namespace PalMapPack;

public sealed class RgbaMap
{
    public RgbaMap(int width, int height, byte[] rgba)
    {
        if (width <= 0 || height <= 0 || width > 8193 || height > 8193)
        {
            throw new ArgumentOutOfRangeException(nameof(width), "map dimensions are invalid");
        }
        if (rgba.Length != checked(width * height * 4))
        {
            throw new ArgumentException("RGBA8 byte length does not match map dimensions", nameof(rgba));
        }
        Width = width;
        Height = height;
        Rgba = rgba.ToArray();
    }

    public int Width { get; }
    public int Height { get; }
    public byte[] Rgba { get; }
}

public sealed record MapRegionAsset(
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
    RgbaMap Map);

public sealed class MapRegionSet
{
    private readonly MapRegionAsset[] _regions;

    public MapRegionSet(IEnumerable<MapRegionAsset> regions)
    {
        ArgumentNullException.ThrowIfNull(regions);
        _regions = regions.ToArray();
        if (_regions.Length == 0)
        {
            throw Failure("map asset set contains no regions");
        }
        if (_regions.Any(region =>
                string.IsNullOrWhiteSpace(region.MapId)
                || string.IsNullOrWhiteSpace(region.RegionId)
                || string.IsNullOrWhiteSpace(region.SourceTexturePath)
                || region.Map is null
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
                || region.BlockSizeY <= 0))
        {
            throw Failure("map region metadata is invalid");
        }
        if (_regions
            .GroupBy(
                region => $"{region.MapId}\0{region.RegionId}",
                StringComparer.OrdinalIgnoreCase)
            .Any(group => group.Count() != 1))
        {
            throw Failure("map region identifiers must be unique");
        }
    }

    public IReadOnlyList<MapRegionAsset> All => _regions;

    public MapRegionAsset Select(double worldX, double worldY)
    {
        if (!Finite(worldX, worldY))
        {
            throw Failure("map region selection coordinate is non-finite");
        }
        var candidates = _regions
            .Where(region =>
                worldX >= region.WorldMinX
                && worldX <= region.WorldMaxX
                && worldY >= region.WorldMinY
                && worldY <= region.WorldMaxY)
            .ToArray();
        if (candidates.Length == 0)
        {
            throw Failure("world coordinate is outside every map region");
        }
        var highestPriority = candidates.Max(region => region.Priority);
        var selected = candidates.Where(region => region.Priority == highestPriority).ToArray();
        if (selected.Length != 1)
        {
            throw Failure("map region selection has a highest-priority tie");
        }
        return selected[0];
    }

    private static bool Finite(params double[] values) => values.All(double.IsFinite);

    private static MapPackFailure Failure(string message) =>
        new(ExitCodes.AssetContractMismatch, message);
}

public sealed record RawPoiRow(
    string SourceAssetPath,
    string RowKey,
    string Kind,
    string DisplayName,
    double WorldX,
    double WorldY,
    string? ExplicitExclusionReason,
    bool CrossChecked,
    IReadOnlyList<string>? SourceRowKeys = null,
    string? EntityId = null);

public sealed record MapAssetSet(MapRegionSet Regions, IReadOnlyList<RawPoiRow> PoiRows)
{
    public MapRegionAsset MainMap =>
        Regions.All.SingleOrDefault(region => region.MapId == "MainMap")
        ?? throw new MapPackFailure(
            ExitCodes.AssetContractMismatch,
            "map asset set requires exactly one explicit MainMap region");
}

public sealed record AssetReadRequest(
    string GameBuildId,
    string MappingsPath,
    AssetContract Contract,
    IReadOnlyList<string> ContainerPaths);

public sealed record AssetProbeRequest(
    string InstallPath,
    string GameBuildId,
    string MappingsPath,
    AssetContract Contract);

public sealed record AssetProbeResult(
    MapRegionSet Regions,
    AssetInventory Inventory,
    PoiProbeSummary? PoiSummary = null);

public sealed record PoiProbeSummary(
    int WorldExportCount,
    int FastTravelTextRowCount,
    int FastTravelCount,
    int DungeonCount,
    int BossRawCount,
    int BossSemanticCount,
    int PalBossCount,
    int HumanBossCount,
    int BossRegionCount,
    int BossDuplicateCount);

public interface IMapAssetReader
{
    MapAssetSet Read(AssetReadRequest request);
}
