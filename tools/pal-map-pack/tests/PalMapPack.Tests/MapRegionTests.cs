using PalMapPack;

namespace PalMapPack.Tests;

public sealed class MapRegionTests
{
    [Fact]
    public void SelectsTheUniqueHighestPriorityRegionContainingTheWorldCoordinate()
    {
        var regions = new MapRegionSet(
        [
            Region(
                "MainMap",
                "FirstRegion",
                "/Game/Pal/Texture/UI/Map/T_WorldMap.T_WorldMap",
                -1_099_400,
                -724_400,
                349_400,
                724_400,
                priority: 0),
            Region(
                "Tree",
                "DummyRegion",
                "/Game/Pal/Texture/UI/Map/T_TreeMap.T_TreeMap",
                347_351.5,
                -818_197,
                689_148.5,
                -476_400,
                priority: 1),
        ]);

        var selected = regions.Select(348_000, -600_000);

        Assert.Equal("Tree", selected.MapId);
        Assert.Equal("DummyRegion", selected.RegionId);
        Assert.Equal("/Game/Pal/Texture/UI/Map/T_TreeMap.T_TreeMap", selected.SourceTexturePath);
        Assert.Equal(1.0, selected.BlockSizeX);
        Assert.Equal(1.0, selected.BlockSizeY);
        Assert.Equal(0.0, selected.GridPositionX);
        Assert.Equal(0.0, selected.GridPositionY);
        Assert.Equal(1, selected.Priority);
    }

    [Fact]
    public void MissingOutsideNonFiniteAndPriorityTiesFailClosed()
    {
        Assert.Throws<MapPackFailure>(() => new MapRegionSet([]));

        var main = Region(
            "MainMap",
            "FirstRegion",
            "/Game/Pal/Texture/UI/Map/T_WorldMap.T_WorldMap",
            -1_099_400,
            -724_400,
            349_400,
            724_400,
            priority: 0);
        var tie = Region(
            "Tree",
            "DummyRegion",
            "/Game/Pal/Texture/UI/Map/T_TreeMap.T_TreeMap",
            347_351.5,
            -818_197,
            689_148.5,
            -476_400,
            priority: 0);
        var regions = new MapRegionSet([main, tie]);

        Assert.Throws<MapPackFailure>(() => regions.Select(348_000, -600_000));
        Assert.Throws<MapPackFailure>(() => regions.Select(900_000, 900_000));
        Assert.Throws<MapPackFailure>(() => regions.Select(double.NaN, 0));
    }

    private static MapRegionAsset Region(
        string mapId,
        string regionId,
        string texture,
        double minX,
        double minY,
        double maxX,
        double maxY,
        int priority) =>
        new(
            mapId,
            regionId,
            texture,
            minX,
            minY,
            maxX,
            maxY,
            1,
            1,
            0,
            0,
            priority,
            new RgbaMap(2, 2, new byte[16]));
}
