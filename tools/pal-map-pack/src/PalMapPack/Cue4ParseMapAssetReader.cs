using CUE4Parse.FileProvider;
using CUE4Parse.MappingsProvider.Usmap;
using CUE4Parse.UE4.Assets.Exports;
using CUE4Parse.UE4.Assets.Exports.Engine;
using CUE4Parse.UE4.Assets.Exports.Texture;
using CUE4Parse.UE4.Assets.Objects;
using CUE4Parse.UE4.Objects.Core.Math;
using CUE4Parse.UE4.Objects.UObject;
using CUE4Parse.UE4.Versions;
using CUE4Parse_Conversion.Textures;

namespace PalMapPack;

/// <summary>
/// Pinned, local-only CUE4Parse boundary for exact-Build inventory and extraction.
/// </summary>
public sealed class Cue4ParseMapAssetReader : IMapAssetReader
{
    private static readonly string[] PoiCandidateTerms =
    [
        "Boss",
        "Camp",
        "Captured",
        "Chest",
        "Dungeon",
        "Effigy",
        "Egg",
        "FastTravel",
        "Fish",
        "Fruit",
        "Junk",
        "Map",
        "Mining",
        "Ore",
        "PalSpawner",
        "Respawn",
        "Resource",
        "Spawner",
        "Statue",
        "Tower",
        "Warp",
    ];

    public static IReadOnlyList<string> DiscoverPoiCandidatePackages(
        string installPath,
        string mappingsPath)
    {
        ArgumentException.ThrowIfNullOrWhiteSpace(installPath);
        ArgumentException.ThrowIfNullOrWhiteSpace(mappingsPath);
        if (!Directory.Exists(installPath) || !File.Exists(mappingsPath))
        {
            throw Failure("POI candidate discovery inputs are missing");
        }

        var provider = OpenProvider(installPath, mappingsPath);
        return provider.Files.Keys
            .Where(path =>
                path.StartsWith("Pal/Content/", StringComparison.OrdinalIgnoreCase)
                && (path.EndsWith(".uasset", StringComparison.OrdinalIgnoreCase)
                    || path.EndsWith(".umap", StringComparison.OrdinalIgnoreCase))
                && PoiCandidateTerms.Any(term =>
                    path.Contains(term, StringComparison.OrdinalIgnoreCase)))
            .Select(path => path.Replace('\\', '/'))
            .Order(StringComparer.OrdinalIgnoreCase)
            .ToArray();
    }

    public AssetProbeResult Probe(AssetProbeRequest request)
    {
        ArgumentNullException.ThrowIfNull(request);
        try
        {
            ValidateProbeIdentity(request);
            var provider = OpenProvider(
                request.InstallPath,
                request.MappingsPath);
            var regions = ReadRegions(provider, request.Contract);
            var inventory = ReadInventory(provider, request.Contract.Sentinels);
            var pois = Cue4ParsePoiAdapter.Read(provider, request.Contract.PoiSources);
            return new AssetProbeResult(regions, inventory, pois.Summary);
        }
        catch (MapPackFailure)
        {
            throw;
        }
        catch (Exception)
        {
            throw Failure("CUE4Parse could not decode the exact-Build asset contract");
        }
    }

    public MapAssetSet Read(AssetReadRequest request)
    {
        ArgumentNullException.ThrowIfNull(request);
        if (!string.Equals(
                request.GameBuildId,
                request.Contract.GameBuildId,
                StringComparison.Ordinal)
            || !request.Contract.Reviewed
            || string.IsNullOrWhiteSpace(request.Contract.ReviewId)
            || string.IsNullOrWhiteSpace(request.Contract.ApprovedMappingSha256)
            || string.IsNullOrWhiteSpace(request.Contract.ApprovedSealedHoldoutSha256)
            || !File.Exists(request.MappingsPath)
            || !string.Equals(
                Hashing.Sha256File(request.MappingsPath),
                request.Contract.ApprovedMappingSha256,
                StringComparison.Ordinal))
        {
            throw new MapPackFailure(
                ExitCodes.AssetContractMismatch,
                "extraction requires a reviewed exact-Build contract and approved mapping hash");
        }
        if (request.ContainerPaths.Count == 0)
        {
            throw Failure("extraction requires the complete source container set");
        }
        var installPath = FindInstallRoot(request.ContainerPaths);
        try
        {
            var provider = OpenProvider(installPath, request.MappingsPath);
            var regions = ReadRegions(provider, request.Contract);
            var pois = Cue4ParsePoiAdapter.Read(provider, request.Contract.PoiSources);
            return new MapAssetSet(regions, pois.Rows);
        }
        catch (MapPackFailure)
        {
            throw;
        }
        catch (Exception)
        {
            throw Failure("CUE4Parse could not decode the exact-Build asset contract");
        }
    }

    private static DefaultFileProvider OpenProvider(string installPath, string mappingsPath)
    {
        var provider = new DefaultFileProvider(
            installPath,
            SearchOption.AllDirectories,
            new VersionContainer(EGame.GAME_UE5_1),
            StringComparer.OrdinalIgnoreCase)
        {
            MappingsContainer = new FileUsmapTypeMappingsProvider(mappingsPath),
        };
        provider.Initialize();
        if (provider.Mount() <= 0)
        {
            throw Failure("CUE4Parse mounted no containers");
        }
        provider.LoadVirtualPaths();
        return provider;
    }

    private static MapRegionSet ReadRegions(
        DefaultFileProvider provider,
        AssetContract contract)
    {
        var table = provider.LoadPackageObject<UDataTable>(contract.WorldMapTablePath);
        var expected = contract.MapRegions.ToDictionary(
            region => (region.MapId, region.RegionId));
        if (expected.Count != contract.MapRegions.Count
            || table.RowMap.Count != contract.MapRegions.Select(region => region.MapId).Distinct().Count())
        {
            throw Failure("world-map region contract has duplicate or missing rows");
        }

        var decodedRegions = new List<MapRegionAsset>(expected.Count);
        foreach (var row in table.RowMap)
        {
            var mapId = row.Key.Text;
            var values = row.Value.Properties.ToDictionary(
                property => property.Name.Text,
                property => property.Tag,
                StringComparer.Ordinal);
            var required = new[]
            {
                "landScapeRealPositionMin",
                "landScapeRealPositionMax",
                "textureDataMap",
                "WorldMapPriority",
            };
            if (required.Any(name => !values.ContainsKey(name)))
            {
                throw Failure("world-map row is missing contract properties");
            }

            var minimum = values["landScapeRealPositionMin"]!.GetValue<FVector>()!;
            var maximum = values["landScapeRealPositionMax"]!.GetValue<FVector>()!;
            var priority = values["WorldMapPriority"]!.GetValue<int>();
            var textureData = values["textureDataMap"]!.GetValue<UScriptMap>()!;
            foreach (var entry in textureData.Properties)
            {
                var regionId = entry.Key.GetValue<FName>()!.Text;
                if (!expected.TryGetValue((mapId, regionId), out var expectedRegion)
                    || entry.Value is null)
                {
                    throw Failure("world-map row contains an uncontracted region");
                }
                var regionValues = entry.Value.GetValue<FStructFallback>()!
                    .Properties
                    .ToDictionary(
                        property => property.Name.Text,
                        property => property.Tag,
                        StringComparer.Ordinal);
                if (!regionValues.TryGetValue("Texture", out var textureTag)
                    || !regionValues.TryGetValue("blockSize", out var blockSizeTag)
                    || !regionValues.TryGetValue("gridPosition", out var gridPositionTag))
                {
                    throw Failure("world-map region is missing texture geometry");
                }
                var texturePath = textureTag!.GetValue<FSoftObjectPath>()!;
                var blockSize = blockSizeTag!.GetValue<FVector2D>()!;
                var gridPosition = gridPositionTag!.GetValue<FVector2D>()!;
                var actual = expectedRegion with
                {
                    SourceTexturePath = texturePath.AssetPathName.Text,
                    WorldMinX = minimum.X,
                    WorldMinY = minimum.Y,
                    WorldMaxX = maximum.X,
                    WorldMaxY = maximum.Y,
                    BlockSizeX = blockSize.X,
                    BlockSizeY = blockSize.Y,
                    GridPositionX = gridPosition.X,
                    GridPositionY = gridPosition.Y,
                    Priority = priority,
                };
                if (actual != expectedRegion
                    || !texturePath.TryLoad<UTexture2D>(out var texture)
                    || texture is null)
                {
                    throw Failure("world-map region does not match the exact-Build contract");
                }
                var decoded = texture.Decode()
                    ?? throw Failure("contracted map texture did not decode");
                if (decoded.Width != expectedRegion.MapWidthPx
                    || decoded.Height != expectedRegion.MapHeightPx)
                {
                    throw Failure("contracted map texture dimensions changed");
                }
                var rgba = ValidateDecodedRgba(
                    decoded.Width,
                    decoded.Height,
                    decoded.Data,
                    decoded.PixelFormat);
                if (expectedRegion.MapAssetSha256 is { } expectedMapAssetSha256
                    && Hashing.MapAssetSha256(rgba) != expectedMapAssetSha256)
                {
                    throw Failure("contracted map texture content hash changed");
                }
                decodedRegions.Add(new MapRegionAsset(
                    mapId,
                    regionId,
                    texturePath.AssetPathName.Text,
                    minimum.X,
                    minimum.Y,
                    maximum.X,
                    maximum.Y,
                    blockSize.X,
                    blockSize.Y,
                    gridPosition.X,
                    gridPosition.Y,
                    priority,
                    rgba));
            }
        }
        if (decodedRegions.Count != expected.Count)
        {
            throw Failure("not every contracted map region was decoded");
        }
        return new MapRegionSet(decodedRegions);
    }

    private static AssetInventory ReadInventory(
        DefaultFileProvider provider,
        IReadOnlyList<AssetSentinel> sentinels)
    {
        var entries = new List<AssetInventoryEntry>(sentinels.Count);
        foreach (var sentinel in sentinels)
        {
            var export = provider.LoadPackageObject<UObject>(sentinel.PackagePath);
            var properties = new List<string>();
            switch (export)
            {
                case UTexture2D texture:
                    {
                        var decoded = texture.Decode()
                            ?? throw Failure("sentinel texture did not decode");
                        EnsureDecodedRgba(
                            decoded.Width,
                            decoded.Height,
                            decoded.Data,
                            decoded.PixelFormat);
                        if (decoded.Width > 0)
                        {
                            properties.Add("SizeX");
                        }
                        if (decoded.Height > 0)
                        {
                            properties.Add("SizeY");
                        }
                        properties.Add("PixelFormat");
                        break;
                    }
                case UDataTable table:
                    if (table.RowMap.Count > 0)
                    {
                        properties.Add("RowMap");
                    }
                    break;
            }
            entries.Add(new AssetInventoryEntry(
                sentinel.PackagePath,
                export.ExportType,
                properties));
        }
        return new AssetInventory(entries);
    }

    public static RgbaMap ValidateDecodedRgba(
        int width,
        int height,
        byte[] data,
        EPixelFormat pixelFormat)
    {
        EnsureDecodedRgba(width, height, data, pixelFormat);
        return new RgbaMap(width, height, data);
    }

    private static void EnsureDecodedRgba(
        int width,
        int height,
        byte[] data,
        EPixelFormat pixelFormat)
    {
        if (pixelFormat != EPixelFormat.PF_R8G8B8A8
            || width is <= 0 or > 8192
            || height is <= 0 or > 8192
            || data.Length != checked(width * height * 4))
        {
            throw Failure("decoded map texture is not canonical PF_R8G8B8A8");
        }
    }

    private static void ValidateProbeIdentity(AssetProbeRequest request)
    {
        if (request.GameBuildId != request.Contract.GameBuildId
            || !Directory.Exists(request.InstallPath)
            || !File.Exists(request.MappingsPath)
            || request.Contract.MappingCandidate is null
            || !string.Equals(
                Hashing.Sha256File(request.MappingsPath),
                request.Contract.MappingCandidate.Sha256,
                StringComparison.Ordinal))
        {
            throw new MapPackFailure(
                ExitCodes.AssetContractMismatch,
                "probe inputs do not match the candidate exact-Build contract");
        }
        _ = new MapRegionSet(request.Contract.MapRegions.Select(region => new MapRegionAsset(
            region.MapId,
            region.RegionId,
            region.SourceTexturePath,
            region.WorldMinX,
            region.WorldMinY,
            region.WorldMaxX,
            region.WorldMaxY,
            region.BlockSizeX,
            region.BlockSizeY,
            region.GridPositionX,
            region.GridPositionY,
            region.Priority,
            new RgbaMap(1, 1, new byte[4]))));
    }

    private static string FindInstallRoot(IReadOnlyList<string> containerPaths)
    {
        var full = containerPaths.Select(Path.GetFullPath).ToArray();
        var marker = $"{Path.DirectorySeparatorChar}Pal{Path.DirectorySeparatorChar}Content{Path.DirectorySeparatorChar}Paks{Path.DirectorySeparatorChar}";
        var index = full[0].IndexOf(marker, StringComparison.OrdinalIgnoreCase);
        if (index <= 0 || full.Any(path => !path.StartsWith(full[0][..index], StringComparison.OrdinalIgnoreCase)))
        {
            throw Failure("source containers do not share one Palworld install root");
        }
        return full[0][..index];
    }

    private static MapPackFailure Failure(string message) =>
        new(ExitCodes.MountSerialization, message);
}
