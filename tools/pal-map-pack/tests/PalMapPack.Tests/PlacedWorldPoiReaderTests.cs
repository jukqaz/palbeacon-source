using PalMapPack;

namespace PalMapPack.Tests;

public sealed class PlacedWorldPoiReaderTests
{
    private const string PlacementPackage = "Pal/Content/Pal/Maps/MainWorld_5/PL_MainWorld5";

    [Fact]
    public void NormalizeEmitsExactPlacedActorsWithLocalizedAndGenericLabels()
    {
        var fastGuid = "FastTravel_001";
        var dungeonGuid = Guid.ParseExact("00112233445566778899aabbccddeeff", "N");
        var result = PlacedWorldPoiReader.Normalize(
            [
                NoiseActor("BP_LevelObject_TowerFastTravelPoint_Component_C"),
                FastActor("fast-export", fastGuid, "root-fast"),
                DungeonActor("dungeon-export", "BP_DungeonPortalMarker_Forest_C", dungeonGuid, "root-dungeon"),
            ],
            [
                Component("root-fast", 100, -200, 300),
                Component("root-dungeon", -400, 500, -600),
            ],
            new DictionaryFastTravelNameResolver(new Dictionary<string, string>(StringComparer.Ordinal)
            {
                [fastGuid] = "작은 부락",
            }),
            Contract(expectedFastTravelCount: 1, expectedDungeonPortalCount: 1));

        Assert.Equal(1, result.FastTravelCount);
        Assert.Equal(1, result.DungeonPortalCount);
        Assert.Equal(2, result.PoiRows.Count);

        var fast = Assert.Single(result.PoiRows, row => row.Kind == "PointFastTravel");
        Assert.Equal("fast-travel:FastTravel_001", fast.RowKey);
        Assert.Equal("작은 부락", fast.DisplayName);
        Assert.Equal((100, -200), (fast.WorldX, fast.WorldY));
        Assert.Equal(PlacementPackage, fast.SourceAssetPath);
        Assert.Equal(new[] { "fast-export" }, fast.SourceRowKeys);
        Assert.Null(fast.ExplicitExclusionReason);
        Assert.True(fast.CrossChecked);

        var dungeon = Assert.Single(result.PoiRows, row => row.Kind == "PointDungeonPortal");
        Assert.Equal("dungeon:00112233445566778899aabbccddeeff", dungeon.RowKey);
        Assert.Equal("던전", dungeon.DisplayName);
        Assert.Equal((-400, 500), (dungeon.WorldX, dungeon.WorldY));
        Assert.Equal(PlacementPackage, dungeon.SourceAssetPath);
        Assert.Equal(new[] { "dungeon-export" }, dungeon.SourceRowKeys);
        Assert.Null(dungeon.ExplicitExclusionReason);
        Assert.True(dungeon.CrossChecked);
    }

    [Theory]
    [InlineData("BP_DungeonPortalMarker_Desert_C")]
    [InlineData("BP_DungeonPortalMarker_Forest_C")]
    [InlineData("BP_DungeonPortalMarker_Grass1_C")]
    [InlineData("BP_DungeonPortalMarker_Sakura_C")]
    [InlineData("BP_DungeonPortalMarker_Skyland_C")]
    [InlineData("BP_DungeonPortalMarker_Snow_C")]
    [InlineData("BP_DungeonPortalMarker_Viking_B_C")]
    [InlineData("BP_DungeonPortalMarker_Viking_C")]
    [InlineData("BP_DungeonPortalMarker_Viking_C_C")]
    [InlineData("BP_DungeonPortalMarker_Volcano_C")]
    [InlineData("BP_DungeonPortalMarker_Yakushima_C")]
    public void NormalizeAcceptsEveryAndOnlyWhitelistedDungeonExportType(string exportType)
    {
        Assert.Equal(11, PlacedWorldPoiReader.DungeonExportTypes.Count);
        var result = PlacedWorldPoiReader.Normalize(
            [DungeonActor("dungeon", exportType, Guid.NewGuid(), "root")],
            [Component("root", 1, 2, 3)],
            new DictionaryFastTravelNameResolver(),
            Contract(expectedFastTravelCount: 0, expectedDungeonPortalCount: 1));

        Assert.Single(result.PoiRows);
        Assert.Equal("PointDungeonPortal", result.PoiRows[0].Kind);
    }

    [Fact]
    public void NormalizeIgnoresNearMatchesComponentsAndDungeonExitActors()
    {
        var result = PlacedWorldPoiReader.Normalize(
            [
                NoiseActor("BP_LevelObject_TowerFastTravelPoint_C_suffix"),
                NoiseActor("bp_levelobject_towerfasttravelpoint_c"),
                NoiseActor("BP_DungeonPortalMarker_Forest_Component_C"),
                NoiseActor("BP_DungeonExit_C"),
            ],
            [],
            new DictionaryFastTravelNameResolver(),
            Contract(expectedFastTravelCount: 0, expectedDungeonPortalCount: 0));

        Assert.Empty(result.PoiRows);
    }

    [Fact]
    public void NormalizeRequiresExactFastTravelPointIdAndRootComponentProperties()
    {
        var valid = FastActor("fast", "FastTravel_001", "root");
        AssertContractFailure(
            [valid with { Properties = Without(valid.Properties, "FastTravelPointID") }],
            [Component("root", 1, 2, 3)],
            names: new Dictionary<string, string> { ["FastTravel_001"] = "이름" },
            expectedFastTravelCount: 1,
            expectedDungeonPortalCount: 0);
        AssertContractFailure(
            [valid with { Properties = Without(valid.Properties, "RootComponent") }],
            [Component("root", 1, 2, 3)],
            names: new Dictionary<string, string> { ["FastTravel_001"] = "이름" },
            expectedFastTravelCount: 1,
            expectedDungeonPortalCount: 0);

        var wrongCase = valid.Properties.ToDictionary(pair => pair.Key, pair => pair.Value, StringComparer.Ordinal);
        wrongCase.Remove("FastTravelPointID");
        wrongCase["FastTravelPointId"] = new PlacedWorldPoiNameValue("FastTravel_001");
        AssertContractFailure(
            [valid with { Properties = wrongCase }],
            [Component("root", 1, 2, 3)],
            names: new Dictionary<string, string> { ["FastTravel_001"] = "이름" },
            expectedFastTravelCount: 1,
            expectedDungeonPortalCount: 0);
    }

    [Fact]
    public void NormalizeRequiresExactFguidRootComponentAndRelativeLocationProperties()
    {
        var valid = DungeonActor(
            "dungeon",
            "BP_DungeonPortalMarker_Forest_C",
            Guid.ParseExact("00112233445566778899aabbccddeeff", "N"),
            "root");
        AssertContractFailure(
            [valid with { Properties = Without(valid.Properties, "LevelObjectInstanceId") }],
            [Component("root", 1, 2, 3)],
            expectedFastTravelCount: 0,
            expectedDungeonPortalCount: 1);
        AssertContractFailure(
            [valid with { Properties = Without(valid.Properties, "RootComponent") }],
            [Component("root", 1, 2, 3)],
            expectedFastTravelCount: 0,
            expectedDungeonPortalCount: 1);
        AssertContractFailure(
            [valid],
            [Component("root", 1, 2, 3) with { Properties = new Dictionary<string, PlacedWorldPoiSourceValue>() }],
            expectedFastTravelCount: 0,
            expectedDungeonPortalCount: 1);

        var wrongGuidType = valid.Properties.ToDictionary(pair => pair.Key, pair => pair.Value, StringComparer.Ordinal);
        wrongGuidType["LevelObjectInstanceId"] = new PlacedWorldPoiNameValue("00112233445566778899aabbccddeeff");
        AssertContractFailure(
            [valid with { Properties = wrongGuidType }],
            [Component("root", 1, 2, 3)],
            expectedFastTravelCount: 0,
            expectedDungeonPortalCount: 1);
    }

    [Fact]
    public void NormalizeFailsClosedOnDuplicateIdsRootReferencesOrCoordinates()
    {
        AssertContractFailure(
            [
                FastActor("fast-a", "FastTravel_001", "root-a"),
                FastActor("fast-b", "FastTravel_001", "root-b"),
            ],
            [Component("root-a", 1, 2, 3), Component("root-b", 4, 5, 6)],
            names: new Dictionary<string, string> { ["FastTravel_001"] = "이름" },
            expectedFastTravelCount: 2,
            expectedDungeonPortalCount: 0);

        var duplicateDungeonId = Guid.ParseExact("00112233445566778899aabbccddeeff", "N");
        AssertContractFailure(
            [
                DungeonActor("dungeon-a", "BP_DungeonPortalMarker_Forest_C", duplicateDungeonId, "root-a"),
                DungeonActor("dungeon-b", "BP_DungeonPortalMarker_Snow_C", duplicateDungeonId, "root-b"),
            ],
            [Component("root-a", 1, 2, 3), Component("root-b", 4, 5, 6)],
            expectedFastTravelCount: 0,
            expectedDungeonPortalCount: 2);

        AssertContractFailure(
            [
                FastActor("fast", "FastTravel_001", "root"),
                DungeonActor("dungeon", "BP_DungeonPortalMarker_Forest_C", Guid.NewGuid(), "root"),
            ],
            [Component("root", 1, 2, 3)],
            names: new Dictionary<string, string> { ["FastTravel_001"] = "이름" },
            expectedFastTravelCount: 1,
            expectedDungeonPortalCount: 1);

        AssertContractFailure(
            [
                FastActor("fast", "FastTravel_001", "root-a"),
                DungeonActor("dungeon", "BP_DungeonPortalMarker_Forest_C", Guid.NewGuid(), "root-b"),
            ],
            [Component("root-a", 1, 2, 3), Component("root-b", 1, 2, 3)],
            names: new Dictionary<string, string> { ["FastTravel_001"] = "이름" },
            expectedFastTravelCount: 1,
            expectedDungeonPortalCount: 1);
    }

    [Fact]
    public void NormalizeRequiresCompleteFastTravelLocalizationAndGenericDungeonLabel()
    {
        AssertContractFailure(
            [FastActor("fast", "FastTravel_001", "root")],
            [Component("root", 1, 2, 3)],
            expectedFastTravelCount: 1,
            expectedDungeonPortalCount: 0);

        var error = Assert.Throws<MapPackFailure>(() => PlacedWorldPoiReader.Normalize(
            [],
            [],
            new DictionaryFastTravelNameResolver(),
            new PlacedWorldPoiContract(PlacementPackage, 0, 0, " ")));
        Assert.Equal(ExitCodes.AssetContractMismatch, error.ExitCode);
    }

    [Fact]
    public void NormalizeUsesConfigurableExpectedCountsAndFailsOnBuildShapeDrift()
    {
        var buildContract = PlacedWorldPoiContract.ForBuild24181527("던전");
        Assert.Equal(152, buildContract.ExpectedFastTravelCount);
        Assert.Equal(157, buildContract.ExpectedDungeonPortalCount);

        var actors = new[]
        {
            FastActor("fast", "FastTravel_001", "root-fast"),
            DungeonActor("dungeon", "BP_DungeonPortalMarker_Forest_C", Guid.NewGuid(), "root-dungeon"),
        };
        var components = new[]
        {
            Component("root-fast", 1, 2, 3),
            Component("root-dungeon", 4, 5, 6),
        };
        var resolver = new DictionaryFastTravelNameResolver(new Dictionary<string, string>
        {
            ["FastTravel_001"] = "이름",
        });

        var success = PlacedWorldPoiReader.Normalize(actors, components, resolver, Contract(1, 1));
        Assert.Equal((1, 1), (success.FastTravelCount, success.DungeonPortalCount));

        var error = Assert.Throws<MapPackFailure>(() => PlacedWorldPoiReader.Normalize(
            actors,
            components,
            resolver,
            Contract(expectedFastTravelCount: 152, expectedDungeonPortalCount: 157)));
        Assert.Equal(ExitCodes.AssetContractMismatch, error.ExitCode);
    }

    [Fact]
    public void NormalizeRejectsEmptyGuidAndNonfiniteCoordinates()
    {
        AssertContractFailure(
            [DungeonActor("dungeon", "BP_DungeonPortalMarker_Forest_C", Guid.Empty, "root")],
            [Component("root", 1, 2, 3)],
            expectedFastTravelCount: 0,
            expectedDungeonPortalCount: 1);
        AssertContractFailure(
            [FastActor("fast", "FastTravel_001", "root")],
            [Component("root", double.NaN, 2, 3)],
            names: new Dictionary<string, string> { ["FastTravel_001"] = "이름" },
            expectedFastTravelCount: 1,
            expectedDungeonPortalCount: 0);
    }

    private static PlacedWorldPoiContract Contract(
        int expectedFastTravelCount,
        int expectedDungeonPortalCount) =>
        new(PlacementPackage, expectedFastTravelCount, expectedDungeonPortalCount, "던전");

    private static PlacedWorldActorSource FastActor(
        string rawExportName,
        string fastTravelPointId,
        string rootComponent) =>
        new(
            rawExportName,
            "BP_LevelObject_TowerFastTravelPoint_C",
            new Dictionary<string, PlacedWorldPoiSourceValue>(StringComparer.Ordinal)
            {
                ["FastTravelPointID"] = new PlacedWorldPoiNameValue(fastTravelPointId),
                ["RootComponent"] = new PlacedWorldPoiObjectReferenceValue(rootComponent),
            });

    private static PlacedWorldActorSource DungeonActor(
        string rawExportName,
        string exportType,
        Guid levelObjectInstanceId,
        string rootComponent) =>
        new(
            rawExportName,
            exportType,
            new Dictionary<string, PlacedWorldPoiSourceValue>(StringComparer.Ordinal)
            {
                ["LevelObjectInstanceId"] = new PlacedWorldPoiGuidValue(levelObjectInstanceId),
                ["RootComponent"] = new PlacedWorldPoiObjectReferenceValue(rootComponent),
            });

    private static PlacedWorldActorSource NoiseActor(string exportType) =>
        new("noise", exportType, new Dictionary<string, PlacedWorldPoiSourceValue>());

    private static PlacedWorldComponentSource Component(
        string objectReference,
        double x,
        double y,
        double z) =>
        new(
            objectReference,
            new Dictionary<string, PlacedWorldPoiSourceValue>(StringComparer.Ordinal)
            {
                ["RelativeLocation"] = new PlacedWorldPoiVectorValue(x, y, z),
            });

    private static IReadOnlyDictionary<string, PlacedWorldPoiSourceValue> Without(
        IReadOnlyDictionary<string, PlacedWorldPoiSourceValue> source,
        string key) =>
        source.Where(pair => pair.Key != key)
            .ToDictionary(pair => pair.Key, pair => pair.Value, StringComparer.Ordinal);

    private static void AssertContractFailure(
        IEnumerable<PlacedWorldActorSource> actors,
        IEnumerable<PlacedWorldComponentSource> components,
        IReadOnlyDictionary<string, string>? names = null,
        int expectedFastTravelCount = 0,
        int expectedDungeonPortalCount = 0)
    {
        var error = Assert.Throws<MapPackFailure>(() => PlacedWorldPoiReader.Normalize(
            actors,
            components,
            new DictionaryFastTravelNameResolver(names),
            Contract(expectedFastTravelCount, expectedDungeonPortalCount)));
        Assert.Equal(ExitCodes.AssetContractMismatch, error.ExitCode);
    }

    private sealed class DictionaryFastTravelNameResolver(
        IReadOnlyDictionary<string, string>? names = null) : IPlacedWorldFastTravelNameResolver
    {
        private readonly IReadOnlyDictionary<string, string> _names =
            names ?? new Dictionary<string, string>();

        public string? Resolve(string fastTravelPointId) =>
            _names.TryGetValue(fastTravelPointId, out var name) ? name : null;
    }
}
