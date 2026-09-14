using SixLabors.ImageSharp;
using SixLabors.ImageSharp.Formats.Jpeg;
using SixLabors.ImageSharp.PixelFormats;
using SixLabors.ImageSharp.Processing;

namespace PalMapPack;

public sealed record TileLevelDocument(
    byte Level,
    uint WidthPx,
    uint HeightPx,
    uint GridWidth,
    uint GridHeight);

public sealed record MapRectangle(double MinX, double MinY, double MaxX, double MaxY);

public sealed record TileDescriptorDocument(
    byte Level,
    uint Y,
    uint X,
    string RelativePath,
    long SizeBytes,
    string Sha256,
    MapRectangle MapRect);

public sealed record TileIndexDocument(
    uint SchemaVersion,
    string GameBuildId,
    string MapId,
    string RegionId,
    uint MapWidthPx,
    uint MapHeightPx,
    uint TileCoreSizePx,
    uint TileGutterPx,
    uint LevelCount,
    uint TileCount,
    IReadOnlyList<TileLevelDocument> Levels,
    IReadOnlyList<TileDescriptorDocument> Tiles);

public sealed record TilePyramidResult(
    string MapId,
    string RegionId,
    int MapWidthPx,
    int MapHeightPx,
    IReadOnlyList<TileLevelDocument> Levels,
    IReadOnlyList<TileDescriptorDocument> Tiles,
    bool SourcePixelsCovered)
{
    public TileIndexDocument ToDocument(string buildId) =>
        new(
            2,
            buildId,
            MapId,
            RegionId,
            checked((uint)MapWidthPx),
            checked((uint)MapHeightPx),
            TilePyramidBuilder.TileCoreSize,
            TilePyramidBuilder.TileGutter,
            checked((uint)Levels.Count),
            checked((uint)Tiles.Count),
            Levels,
            Tiles);
}

public static class TilePyramidBuilder
{
    public const uint TileCoreSize = 512;
    public const uint TileGutter = 2;
    private const int TileOutputSize = (int)(TileCoreSize + 2 * TileGutter);

    public static TilePyramidResult Build(
        RgbaMap source,
        string stagingRoot,
        string buildId,
        string mapId,
        string regionId)
    {
        ArgumentNullException.ThrowIfNull(source);
        if (source.Width > 8192 || source.Height > 8192)
        {
            throw new MapPackFailure(ExitCodes.PackIntegrity, "map dimensions exceed the 8192px cap");
        }
        if (string.IsNullOrEmpty(buildId) || buildId.Any(character => !char.IsAsciiDigit(character)))
        {
            throw new MapPackFailure(ExitCodes.PackIntegrity, "tile pyramid Build identity is invalid");
        }
        var regionPrefix = RegionPackPaths.Prefix(mapId, regionId);

        Directory.CreateDirectory(stagingRoot);
        var levels = new List<TileLevelDocument>();
        var tiles = new List<TileDescriptorDocument>();
        var current = source;
        byte level = 0;

        while (true)
        {
            var gridWidth = checked((uint)((current.Width + (int)TileCoreSize - 1) / TileCoreSize));
            var gridHeight = checked((uint)((current.Height + (int)TileCoreSize - 1) / TileCoreSize));
            levels.Add(new TileLevelDocument(
                level,
                checked((uint)current.Width),
                checked((uint)current.Height),
                gridWidth,
                gridHeight));
            var levelDirectory = Path.Combine(
                stagingRoot,
                regionPrefix.Replace('/', Path.DirectorySeparatorChar),
                "tiles",
                level.ToString(System.Globalization.CultureInfo.InvariantCulture));
            Directory.CreateDirectory(levelDirectory);
            for (uint y = 0; y < gridHeight; y++)
            {
                for (uint x = 0; x < gridWidth; x++)
                {
                    var relativePath = $"{regionPrefix}/tiles/{level}/{y}_{x}.jpg";
                    var outputPath = Path.Combine(stagingRoot, relativePath.Replace('/', Path.DirectorySeparatorChar));
                    var rgba = BuildGutteredRgba(
                        current,
                        checked((int)x),
                        checked((int)y),
                        checked((int)TileCoreSize),
                        checked((int)TileGutter));
                    using (var image = Image.LoadPixelData<Rgba32>(rgba, TileOutputSize, TileOutputSize))
                    {
                        image.Save(outputPath, new JpegEncoder { Quality = 85 });
                    }

                    var scale = 1UL << level;
                    var span = (ulong)TileCoreSize * scale;
                    var minX = (ulong)x * span;
                    var minY = (ulong)y * span;
                    var maxX = Math.Min(minX + span, checked((ulong)source.Width));
                    var maxY = Math.Min(minY + span, checked((ulong)source.Height));
                    var info = new FileInfo(outputPath);
                    tiles.Add(new TileDescriptorDocument(
                        level,
                        y,
                        x,
                        relativePath,
                        info.Length,
                        Hashing.Sha256File(outputPath),
                        new MapRectangle(minX, minY, maxX, maxY)));
                }
            }

            if (current.Width <= TileCoreSize && current.Height <= TileCoreSize)
            {
                break;
            }
            current = ResizeHalf(current);
            level = checked((byte)(level + 1));
        }

        var covered = tiles.Where(tile => tile.Level == 0)
            .Sum(tile => (tile.MapRect.MaxX - tile.MapRect.MinX) * (tile.MapRect.MaxY - tile.MapRect.MinY))
            == (double)source.Width * source.Height;
        if (!covered)
        {
            throw new MapPackFailure(ExitCodes.PackIntegrity, "tile pyramid did not cover every source pixel");
        }
        return new TilePyramidResult(
            mapId,
            regionId,
            source.Width,
            source.Height,
            levels,
            tiles,
            true);
    }

    public static byte[] BuildGutteredRgba(
        RgbaMap source,
        int tileX,
        int tileY,
        int coreSize,
        int gutter)
    {
        if (tileX < 0 || tileY < 0 || coreSize <= 0 || gutter < 0)
        {
            throw new ArgumentOutOfRangeException(nameof(tileX));
        }
        var outputSize = checked(coreSize + 2 * gutter);
        var output = new byte[checked(outputSize * outputSize * 4)];
        var originX = checked(tileX * coreSize);
        var originY = checked(tileY * coreSize);
        for (var outputY = 0; outputY < outputSize; outputY++)
        {
            var sourceY = Math.Clamp(originY + outputY - gutter, 0, source.Height - 1);
            for (var outputX = 0; outputX < outputSize; outputX++)
            {
                var sourceX = Math.Clamp(originX + outputX - gutter, 0, source.Width - 1);
                var sourceOffset = checked((sourceY * source.Width + sourceX) * 4);
                var outputOffset = checked((outputY * outputSize + outputX) * 4);
                Buffer.BlockCopy(source.Rgba, sourceOffset, output, outputOffset, 4);
            }
        }
        return output;
    }

    private static RgbaMap ResizeHalf(RgbaMap source)
    {
        var width = (source.Width + 1) / 2;
        var height = (source.Height + 1) / 2;
        using var image = Image.LoadPixelData<Rgba32>(source.Rgba, source.Width, source.Height);
        image.Mutate(context => context.Resize(width, height, KnownResamplers.Lanczos3));
        var bytes = new byte[checked(width * height * 4)];
        image.CopyPixelDataTo(bytes);
        return new RgbaMap(width, height, bytes);
    }
}
