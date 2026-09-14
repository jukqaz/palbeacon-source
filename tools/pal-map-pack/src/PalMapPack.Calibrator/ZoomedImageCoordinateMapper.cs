using System.Drawing;

namespace PalMapPack.Calibrator;

public static class ZoomedImageCoordinateMapper
{
    public static bool HasSameAspectRatio(
        Size displayedImageSize,
        Size coordinateSpaceSize) =>
        displayedImageSize.Width > 0
        && displayedImageSize.Height > 0
        && coordinateSpaceSize.Width > 0
        && coordinateSpaceSize.Height > 0
        && (long)displayedImageSize.Width * coordinateSpaceSize.Height
            == (long)displayedImageSize.Height * coordinateSpaceSize.Width;

    public static bool TryMap(
        Size controlSize,
        Size displayedImageSize,
        Size coordinateSpaceSize,
        Point controlPoint,
        out PointF imagePoint)
    {
        imagePoint = default;
        if (controlSize.Width <= 0
            || controlSize.Height <= 0
            || displayedImageSize.Width <= 0
            || displayedImageSize.Height <= 0
            || coordinateSpaceSize.Width <= 0
            || coordinateSpaceSize.Height <= 0)
        {
            return false;
        }

        var scale = Math.Min(
            (float)controlSize.Width / displayedImageSize.Width,
            (float)controlSize.Height / displayedImageSize.Height);
        var displayedWidth = (int)(displayedImageSize.Width * scale);
        var displayedHeight = (int)(displayedImageSize.Height * scale);
        var left = (controlSize.Width - displayedWidth) / 2;
        var top = (controlSize.Height - displayedHeight) / 2;
        if (controlPoint.X < left
            || controlPoint.X >= left + displayedWidth
            || controlPoint.Y < top
            || controlPoint.Y >= top + displayedHeight)
        {
            return false;
        }

        imagePoint = new PointF(
            (float)((controlPoint.X - left) * coordinateSpaceSize.Width / (double)displayedWidth),
            (float)((controlPoint.Y - top) * coordinateSpaceSize.Height / (double)displayedHeight));
        return true;
    }
}
