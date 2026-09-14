using PalMapPack;

namespace PalMapPack.Tests;

internal sealed class FixtureAssetReader(MapAssetSet assets) : IMapAssetReader
{
    public MapAssetSet Read(AssetReadRequest request)
    {
        Assert.Equal("24181527", request.GameBuildId);
        return assets;
    }
}

internal static class Fixture
{
    internal const string Build = "24181527";

    internal static IReadOnlyList<Landmark> ExactLandmarks()
    {
        var rows = new List<Landmark>();
        var coordinates = new[]
        {
            (-400_000.0, -20_000.0),
            (-350_000.0, 20_000.0),
            (-375_000.0, 0.0),
            (-900_000.0, 500_000.0),
            (-700_000.0, 300_000.0),
            (-800_000.0, 400_000.0),
            (0.0, 500_000.0),
            (200_000.0, 300_000.0),
            (100_000.0, 400_000.0),
            (-900_000.0, -500_000.0),
            (-700_000.0, -300_000.0),
            (-800_000.0, -400_000.0),
            (0.0, -500_000.0),
            (200_000.0, -300_000.0),
            (100_000.0, -400_000.0),
        };
        for (var index = 0; index < coordinates.Length; index++)
        {
            var (worldX, worldY) = coordinates[index];
            var mapX = 640.0 * (worldY + 724_400.0) / 1_448_800.0;
            var mapY = 520.0 * (349_400.0 - worldX) / 1_448_800.0;
            rows.Add(new Landmark(
                $"landmark-{index}",
                worldX,
                worldY,
                mapX,
                mapY,
                CoordinateSolver.DeriveZone(worldX, worldY),
                index % 3 == 2
                    ? LandmarkEvidenceSet.SealedHoldout
                    : LandmarkEvidenceSet.Reference,
                Build));
        }
        return rows;
    }

    internal static MapAssetSet Assets()
    {
        const int width = 640;
        const int height = 520;
        var rgba = new byte[checked(width * height * 4)];
        for (var y = 0; y < height; y++)
        {
            for (var x = 0; x < width; x++)
            {
                var offset = (y * width + x) * 4;
                rgba[offset] = (byte)(x % 251);
                rgba[offset + 1] = (byte)(y % 251);
                rgba[offset + 2] = (byte)((x + y) % 251);
                rgba[offset + 3] = 255;
            }
        }

        var map = new RgbaMap(width, height, rgba);
        var treeRgba = rgba.ToArray();
        for (var offset = 0; offset < treeRgba.Length; offset += 4)
        {
            treeRgba[offset] ^= 0xff;
        }
        var treeMap = new RgbaMap(width, height, treeRgba);
        return new MapAssetSet(
            new MapRegionSet(
            [
                new MapRegionAsset(
                    "MainMap",
                    "FirstRegion",
                    "/Game/Pal/Texture/UI/Map/T_WorldMap.T_WorldMap",
                    -1_099_400,
                    -724_400,
                    349_400,
                    724_400,
                    1,
                    1,
                    0,
                    0,
                    0,
                    map),
                new MapRegionAsset(
                    "Tree",
                    "DummyRegion",
                    "/Game/Pal/Texture/UI/Map/T_TreeMap.T_TreeMap",
                    347_351.5,
                    -818_197,
                    689_148.5,
                    -476_400,
                    1,
                    1,
                    0,
                    0,
                    1,
                    treeMap),
            ]),
            new[]
            {
                new RawPoiRow("Pal/World", "fast-1", "PointFastTravel", "Fast", 0, 0, null, true),
                new RawPoiRow("Pal/World", "dungeon-1", "PointDungeonPortal", "Dungeon", 10, 10, null, true),
                new RawPoiRow(
                    "Pal/Boss",
                    "boss-1",
                    "FieldBoss",
                    "Boss",
                    -10,
                    -10,
                    null,
                    true,
                    null,
                    "Anubis"),
                new RawPoiRow(
                    "Pal/Boss",
                    "wanted-1",
                    "WantedTarget",
                    "Wanted",
                    -20,
                    -20,
                    null,
                    true),
                new RawPoiRow("Pal/World", "other-1", "Settlement", "Other", 50, 50, "outside-v1-scope", true),
            });
    }

    internal static SourceContainerSnapshot Container(string name = "Pal/Content/Paks/fixture.pak") =>
        new(name, 3, 638_000_000_000_000_000, Hashing.Sha256Hex([1, 2, 3]));

    internal static IReadOnlyList<SourceContainerPath> ContainerPaths(
        string name = "Pal/Content/Paks/fixture.pak")
    {
        var root = Path.Combine(Path.GetTempPath(), "pal-map-pack-fixture-install");
        return
        [
            new SourceContainerPath(
                Path.GetFullPath(Path.Combine(
                    root,
                    name.Replace('/', Path.DirectorySeparatorChar))),
                name),
        ];
    }

    internal static IReadOnlyList<MapRegionDocument> ManifestRegions() =>
        Assets().Regions.All.Select(region =>
        {
            var prefix = RegionPackPaths.Prefix(region.MapId, region.RegionId);
            return new MapRegionDocument(
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
                region.Map.Width,
                region.Map.Height,
                Hashing.MapAssetSha256(region.Map),
                [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
                $"{prefix}/transform.json",
                Hashing.Sha256Hex(System.Text.Encoding.UTF8.GetBytes($"{region.MapId}-transform")),
                $"{prefix}/tile-index.json",
                Hashing.Sha256Hex(System.Text.Encoding.UTF8.GetBytes($"{region.MapId}-tiles")),
                Hashing.Sha256Hex(System.Text.Encoding.UTF8.GetBytes($"{region.MapId}-tile-index")));
        }).ToArray();

    internal static IReadOnlyDictionary<(string MapId, string RegionId), double[][]> RegionTransforms()
    {
        var assets = Assets();
        return assets.Regions.All.ToDictionary(
            region => (region.MapId, region.RegionId),
            region =>
            {
                return CoordinateSolver.DeriveAuthoritativeBoundsTransform(region);
            });
    }

    internal static AssetContract ApprovedContract()
    {
        var assets = Assets();
        var main = assets.Regions.All.Single(region => region.MapId == "MainMap");
        var tree = assets.Regions.All.Single(region => region.MapId == "Tree");
        var mainHash = Hashing.MapAssetSha256(main.Map);
        var preview = CalibrationPreviewBuilder.BuildDeterministicBmp(
            main.Map,
            mainHash,
            320,
            260).Contract;
        return new(
            Build,
            true,
            "reviewed-fixture",
            new[]
            {
                new MapRegionContract(
                    "MainMap",
                    "FirstRegion",
                    "/Game/Pal/Texture/UI/Map/T_WorldMap.T_WorldMap",
                    -1_099_400,
                    -724_400,
                    349_400,
                    724_400,
                    1,
                    1,
                    0,
                    0,
                    640,
                    520,
                    0,
                    mainHash,
                    preview),
                new MapRegionContract(
                    "Tree",
                    "DummyRegion",
                    "/Game/Pal/Texture/UI/Map/T_TreeMap.T_TreeMap",
                    347_351.5,
                    -818_197,
                    689_148.5,
                    -476_400,
                    1,
                    1,
                    0,
                    0,
                    640,
                    520,
                    1,
                    Hashing.MapAssetSha256(tree.Map)),
            },
            "Pal/Content/Pal/DataTable/WorldMapUIData/DT_WorldMapUIData",
            new PoiAssetContract(
                "Pal/Content/Pal/Maps/MainWorld_5/PL_MainWorld5",
                31_159,
                "Pal/Content/Pal/DataTable/Text/DT_MapRespawnPointInfoText",
                "Pal/Content/L10N/ko/Pal/DataTable/Text/DT_MapRespawnPointInfoText",
                199,
                152,
                157,
                "던전",
                new[]
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
                },
                "Pal/Content/Pal/DataTable/UI/DT_BossSpawnerLoactionData",
                "Pal/Content/Pal/DataTable/Text/DT_PalNameText_Common",
                "Pal/Content/L10N/ko/Pal/DataTable/Text/DT_PalNameText_Common",
                "Pal/Content/Pal/DataTable/Text/DT_HumanNameText_Common",
                "Pal/Content/L10N/ko/Pal/DataTable/Text/DT_HumanNameText_Common",
                "Pal/Content/Pal/DataTable/Text/DT_WorldMap_Common_Text_Common",
                "Pal/Content/L10N/ko/Pal/DataTable/Text/DT_WorldMap_Common_Text_Common",
                159,
                126,
                90,
                33,
                3,
                33),
            new[] { "PointFastTravel", "PointDungeonPortal", "FieldBoss" },
            new[] { new AssetSentinel("map", "Texture2D", new[] { "SizeX", "SizeY" }) },
            new MappingCandidateProvenance(
                "fixture/repository",
                "fixture-commit",
                "2026-07-10",
                7,
                Hashing.Sha256Hex("mapping"u8),
                "candidate_unverified"),
            ApprovedMappingSha256: Hashing.Sha256Hex("mapping"u8),
            ApprovedSealedHoldoutSha256: CoordinateSolver.ValidateAuthoritativeBounds(
                assets.MainMap,
                ExactLandmarks(),
                Build).SealedHoldoutSha256);
    }

    internal static AssetInventory Inventory() =>
        new(new[] { new AssetInventoryEntry("map", "Texture2D", new[] { "SizeX", "SizeY" }) });

    internal static string TempDirectory([System.Runtime.CompilerServices.CallerMemberName] string label = "")
    {
        var path = Path.Combine(Path.GetTempPath(), "pal-map-pack-tests", $"{label}-{Guid.NewGuid():N}");
        Directory.CreateDirectory(path);
        return path;
    }
}

internal sealed class CountingInputGuard : IExtractionInputGuard
{
    public int Checks { get; private set; }

    public void EnsureUnchanged() => Checks++;
}
