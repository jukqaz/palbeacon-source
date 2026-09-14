using PalMapPack;
using SixLabors.ImageSharp;
using SixLabors.ImageSharp.PixelFormats;

namespace PalMapPack.Tests;

public sealed class TilePyramidTests
{
    [Fact]
    public void GutterPixelsReplicateNeighborsAndOuterEdges()
    {
        var map = new RgbaMap(3, 2,
        [
            1, 0, 0, 255, 2, 0, 0, 255, 3, 0, 0, 255,
            4, 0, 0, 255, 5, 0, 0, 255, 6, 0, 0, 255,
        ]);
        var pixels = TilePyramidBuilder.BuildGutteredRgba(map, 1, 0, 1, 2);
        Assert.Equal(5 * 5 * 4, pixels.Length);
        Assert.Equal(1, pixels[0]);
        Assert.Equal(2, pixels[(2 * 5 + 2) * 4]);
        Assert.Equal(3, pixels[(2 * 5 + 4) * 4]);
        Assert.Equal(6, pixels[(4 * 5 + 4) * 4]);
    }

    [Fact]
    public void PyramidPreservesAspectRatioCoversEveryPixelAndWrites516Jpegs()
    {
        var root = Fixture.TempDirectory();
        var result = TilePyramidBuilder.Build(
            Fixture.Assets().MainMap.Map,
            root,
            Fixture.Build,
            "MainMap",
            "FirstRegion");
        Assert.Equal((640u, 520u), (result.Levels[0].WidthPx, result.Levels[0].HeightPx));
        Assert.Equal((320u, 260u), (result.Levels[1].WidthPx, result.Levels[1].HeightPx));
        Assert.Equal(5, result.Tiles.Count);
        Assert.All(result.Tiles, tile =>
        {
            using var image = Image.Load<Rgba32>(Path.Combine(root, tile.RelativePath));
            Assert.Equal(516, image.Width);
            Assert.Equal(516, image.Height);
        });
        Assert.True(result.SourcePixelsCovered);
    }

    [Fact]
    public void SourceDimensionsAboveCapAreRejected()
    {
        var oversized = new RgbaMap(8193, 1, new byte[8193 * 4]);
        Assert.Throws<MapPackFailure>(() =>
            TilePyramidBuilder.Build(
                oversized,
                Fixture.TempDirectory(),
                Fixture.Build,
                "MainMap",
                "FirstRegion"));
    }
}
