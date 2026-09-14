using System.Drawing;

namespace PalMapPack.Calibrator;

public readonly record struct PrecisionMapViewport(
    Size PreviewSize,
    Size CoordinateSpaceSize,
    RectangleF PreviewSourceRectangle);

public static class PrecisionMapCoordinateMapper
{
    public const int DefaultPreviewSpanPx = 64;

    public static bool TryCreateViewport(
        Size previewSize,
        Size coordinateSpaceSize,
        PointF focusPoint,
        out PrecisionMapViewport viewport,
        int previewSpanPx = DefaultPreviewSpanPx)
    {
        viewport = default;
        if (previewSize.Width <= 0
            || previewSize.Height <= 0
            || coordinateSpaceSize.Width <= 0
            || coordinateSpaceSize.Height <= 0
            || previewSpanPx <= 0
            || !float.IsFinite(focusPoint.X)
            || !float.IsFinite(focusPoint.Y)
            || focusPoint.X < 0
            || focusPoint.X >= coordinateSpaceSize.Width
            || focusPoint.Y < 0
            || focusPoint.Y >= coordinateSpaceSize.Height)
        {
            return false;
        }

        var sourceWidth = Math.Min(previewSpanPx, previewSize.Width);
        var sourceHeight = Math.Min(previewSpanPx, previewSize.Height);
        var previewX = focusPoint.X * previewSize.Width / coordinateSpaceSize.Width;
        var previewY = focusPoint.Y * previewSize.Height / coordinateSpaceSize.Height;
        var left = Math.Clamp(
            previewX - (sourceWidth / 2f),
            0,
            previewSize.Width - sourceWidth);
        var top = Math.Clamp(
            previewY - (sourceHeight / 2f),
            0,
            previewSize.Height - sourceHeight);
        viewport = new PrecisionMapViewport(
            previewSize,
            coordinateSpaceSize,
            new RectangleF(left, top, sourceWidth, sourceHeight));
        return true;
    }

    public static bool TryMap(
        Size controlSize,
        PrecisionMapViewport viewport,
        Point controlPoint,
        out PointF coordinatePoint)
    {
        coordinatePoint = default;
        if (!IsValid(viewport)
            || controlSize.Width <= 0
            || controlSize.Height <= 0
            || controlPoint.X < 0
            || controlPoint.X >= controlSize.Width
            || controlPoint.Y < 0
            || controlPoint.Y >= controlSize.Height)
        {
            return false;
        }

        var source = viewport.PreviewSourceRectangle;
        var previewX = source.X
            + (controlPoint.X * source.Width / controlSize.Width);
        var previewY = source.Y
            + (controlPoint.Y * source.Height / controlSize.Height);
        coordinatePoint = new PointF(
            previewX * viewport.CoordinateSpaceSize.Width / viewport.PreviewSize.Width,
            previewY * viewport.CoordinateSpaceSize.Height / viewport.PreviewSize.Height);
        return true;
    }

    public static bool TryProject(
        Size controlSize,
        PrecisionMapViewport viewport,
        PointF coordinatePoint,
        out PointF controlPoint)
    {
        controlPoint = default;
        if (!IsValid(viewport)
            || controlSize.Width <= 0
            || controlSize.Height <= 0
            || !float.IsFinite(coordinatePoint.X)
            || !float.IsFinite(coordinatePoint.Y))
        {
            return false;
        }

        var previewX = coordinatePoint.X
            * viewport.PreviewSize.Width
            / viewport.CoordinateSpaceSize.Width;
        var previewY = coordinatePoint.Y
            * viewport.PreviewSize.Height
            / viewport.CoordinateSpaceSize.Height;
        var source = viewport.PreviewSourceRectangle;
        if (previewX < source.Left
            || previewX >= source.Right
            || previewY < source.Top
            || previewY >= source.Bottom)
        {
            return false;
        }

        controlPoint = new PointF(
            (previewX - source.X) * controlSize.Width / source.Width,
            (previewY - source.Y) * controlSize.Height / source.Height);
        return true;
    }

    public static double MaxCoordinateUnitsPerControlPixel(
        Size controlSize,
        PrecisionMapViewport viewport)
    {
        if (!IsValid(viewport)
            || controlSize.Width <= 0
            || controlSize.Height <= 0)
        {
            return double.PositiveInfinity;
        }

        var source = viewport.PreviewSourceRectangle;
        var unitsPerPixelX = source.Width
            * viewport.CoordinateSpaceSize.Width
            / viewport.PreviewSize.Width
            / controlSize.Width;
        var unitsPerPixelY = source.Height
            * viewport.CoordinateSpaceSize.Height
            / viewport.PreviewSize.Height
            / controlSize.Height;
        return Math.Max(unitsPerPixelX, unitsPerPixelY);
    }

    private static bool IsValid(PrecisionMapViewport viewport)
    {
        var source = viewport.PreviewSourceRectangle;
        return viewport.PreviewSize.Width > 0
            && viewport.PreviewSize.Height > 0
            && viewport.CoordinateSpaceSize.Width > 0
            && viewport.CoordinateSpaceSize.Height > 0
            && float.IsFinite(source.X)
            && float.IsFinite(source.Y)
            && float.IsFinite(source.Width)
            && float.IsFinite(source.Height)
            && source.X >= 0
            && source.Y >= 0
            && source.Width > 0
            && source.Height > 0
            && source.Right <= viewport.PreviewSize.Width
            && source.Bottom <= viewport.PreviewSize.Height;
    }
}
