using SixLabors.ImageSharp;
using SixLabors.ImageSharp.PixelFormats;

namespace PalDataPack.Tests;

public sealed class GameIconExtractorTests
{
    [Fact]
    public void AlphaBoundsPreserveTheVisibleSourceRectangle()
    {
        using var image = new Image<Rgba32>(5, 4, Color.Transparent);
        image[1, 2] = Color.White;
        image[3, 3] = Color.Red;

        var bounds = GameIconExtractor.FindAlphaBounds(image);

        Assert.Equal(new GameIconAlphaBounds(1, 2, 3, 2), bounds);
    }

    [Fact]
    public void FullyTransparentSourceHasNoInventedBounds()
    {
        using var image = new Image<Rgba32>(3, 3, Color.Transparent);

        Assert.Null(GameIconExtractor.FindAlphaBounds(image));
    }
}
