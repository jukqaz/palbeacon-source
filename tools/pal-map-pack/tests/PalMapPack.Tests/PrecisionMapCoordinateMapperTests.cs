using System.Drawing;
using PalMapPack.Calibrator;

namespace PalMapPack.Tests;

public sealed class PrecisionMapCoordinateMapperTests
{
    private static readonly Size PreviewSize = new(2_048, 2_048);
    private static readonly Size CoordinateSpaceSize = new(8_192, 8_192);
    private static readonly Size MagnifierSize = new(256, 256);

    [Fact]
    public void CenteredViewportProvidesOneAuthoritativePixelPerScreenPixel()
    {
        Assert.True(PrecisionMapCoordinateMapper.TryCreateViewport(
            PreviewSize,
            CoordinateSpaceSize,
            new PointF(4_096, 4_096),
            out var viewport));

        Assert.Equal(new RectangleF(992, 992, 64, 64), viewport.PreviewSourceRectangle);
        Assert.Equal(
            1,
            PrecisionMapCoordinateMapper.MaxCoordinateUnitsPerControlPixel(
                MagnifierSize,
                viewport),
            6);
        Assert.True(PrecisionMapCoordinateMapper.TryMap(
            MagnifierSize,
            viewport,
            new Point(128, 128),
            out var center));
        Assert.Equal(4_096, center.X, 6);
        Assert.Equal(4_096, center.Y, 6);
        Assert.True(PrecisionMapCoordinateMapper.TryMap(
            MagnifierSize,
            viewport,
            new Point(129, 127),
            out var neighboringPixel));
        Assert.Equal(4_097, neighboringPixel.X, 6);
        Assert.Equal(4_095, neighboringPixel.Y, 6);
    }

    [Fact]
    public void ViewportClampsAtMapEdgesWithoutLosingPrecision()
    {
        Assert.True(PrecisionMapCoordinateMapper.TryCreateViewport(
            PreviewSize,
            CoordinateSpaceSize,
            new PointF(0, 0),
            out var viewport));

        Assert.Equal(new RectangleF(0, 0, 64, 64), viewport.PreviewSourceRectangle);
        Assert.True(PrecisionMapCoordinateMapper.TryMap(
            MagnifierSize,
            viewport,
            new Point(255, 255),
            out var bottomRight));
        Assert.Equal(255, bottomRight.X, 6);
        Assert.Equal(255, bottomRight.Y, 6);
        Assert.True(PrecisionMapCoordinateMapper.TryProject(
            MagnifierSize,
            viewport,
            new PointF(128, 64),
            out var projected));
        Assert.Equal(128, projected.X, 6);
        Assert.Equal(64, projected.Y, 6);
    }

    [Fact]
    public void ProjectAndMapRoundTripPreservesFractionalCoordinates()
    {
        Assert.True(PrecisionMapCoordinateMapper.TryCreateViewport(
            PreviewSize,
            CoordinateSpaceSize,
            new PointF(3_217.25f, 6_001.5f),
            out var viewport));
        var coordinate = new PointF(3_220.25f, 5_995.5f);

        Assert.True(PrecisionMapCoordinateMapper.TryProject(
            MagnifierSize,
            viewport,
            coordinate,
            out var controlPoint));
        Assert.True(PrecisionMapCoordinateMapper.TryMap(
            MagnifierSize,
            viewport,
            Point.Round(controlPoint),
            out var roundTripped));

        Assert.InRange(Math.Abs(roundTripped.X - coordinate.X), 0, 0.51f);
        Assert.InRange(Math.Abs(roundTripped.Y - coordinate.Y), 0, 0.51f);
    }

    [Fact]
    public void InvalidAndOutOfBoundsInputsAreRejected()
    {
        Assert.False(PrecisionMapCoordinateMapper.TryCreateViewport(
            Size.Empty,
            CoordinateSpaceSize,
            PointF.Empty,
            out _));
        Assert.False(PrecisionMapCoordinateMapper.TryCreateViewport(
            PreviewSize,
            CoordinateSpaceSize,
            new PointF(8_192, 0),
            out _));
        Assert.True(PrecisionMapCoordinateMapper.TryCreateViewport(
            PreviewSize,
            CoordinateSpaceSize,
            new PointF(4_096, 4_096),
            out var viewport));
        Assert.False(PrecisionMapCoordinateMapper.TryMap(
            MagnifierSize,
            viewport,
            new Point(256, 128),
            out _));
        Assert.False(PrecisionMapCoordinateMapper.TryProject(
            MagnifierSize,
            viewport,
            new PointF(0, 0),
            out _));
    }
}
