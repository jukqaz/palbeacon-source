using System.Drawing;
using System.Runtime.CompilerServices;

[assembly: InternalsVisibleTo("PalMapPack.Tests")]

namespace PalMapPack.Calibrator;

public enum LocalAlignmentCapturePhase
{
    AwaitingOverview = 0,
    AwaitingPrecisionConfirmation = 1,
    ReadyToExport = 2,
    Exported = 3,
}

internal sealed class LocalAlignmentExportLease
{
    internal LocalAlignmentExportLease(PointF observedMarker)
    {
        ObservedMarker = observedMarker;
    }

    internal PointF ObservedMarker { get; }
}

public sealed class LocalAlignmentCaptureSession
{
    public const int MapWidthPx = 2_048;
    public const int MapHeightPx = 2_048;

    private static readonly Size MapSize = new(MapWidthPx, MapHeightPx);
    private static readonly Size PrecisionControlSize = new(256, 256);
    private readonly Lock _sync = new();
    private LocalAlignmentCapturePhase _phase =
        LocalAlignmentCapturePhase.AwaitingOverview;
    private PointF? _overviewPoint;
    private PointF? _confirmedPoint;
    private PrecisionMapViewport _precisionViewport;
    private LocalAlignmentExportLease? _activeExportLease;

    public LocalAlignmentCapturePhase Phase
    {
        get
        {
            lock (_sync)
            {
                return _phase;
            }
        }
    }

    public PointF? OverviewPoint
    {
        get
        {
            lock (_sync)
            {
                return _overviewPoint;
            }
        }
    }

    public PointF? ConfirmedPoint
    {
        get
        {
            lock (_sync)
            {
                return _confirmedPoint;
            }
        }
    }

    public void CaptureOverview(PointF mapPoint)
    {
        lock (_sync)
        {
            if (_phase != LocalAlignmentCapturePhase.AwaitingOverview)
            {
                throw new InvalidOperationException(
                    "the overview marker can be captured only once");
            }

            mapPoint = RequireMapPoint(mapPoint);
            if (!PrecisionMapCoordinateMapper.TryCreateViewport(
                    MapSize,
                    MapSize,
                    mapPoint,
                    out _precisionViewport))
            {
                throw new ArgumentOutOfRangeException(
                    nameof(mapPoint),
                    "overview marker must create a valid precision viewport");
            }

            _overviewPoint = mapPoint;
            _phase = LocalAlignmentCapturePhase.AwaitingPrecisionConfirmation;
        }
    }

    public void ConfirmPrecision(PointF mapPoint)
    {
        lock (_sync)
        {
            if (_phase
                != LocalAlignmentCapturePhase.AwaitingPrecisionConfirmation)
            {
                throw new InvalidOperationException(
                    "precision confirmation requires one overview capture");
            }

            mapPoint = RequireMapPoint(mapPoint);
            if (!PrecisionMapCoordinateMapper.TryProject(
                    PrecisionControlSize,
                    _precisionViewport,
                    mapPoint,
                    out _))
            {
                throw new ArgumentOutOfRangeException(
                    nameof(mapPoint),
                    "precision marker must be inside the overview precision viewport");
            }

            _confirmedPoint = mapPoint;
            _phase = LocalAlignmentCapturePhase.ReadyToExport;
        }
    }

    internal LocalAlignmentExportLease ReserveConfirmedPointForExport()
    {
        lock (_sync)
        {
            if (_phase != LocalAlignmentCapturePhase.ReadyToExport
                || _confirmedPoint is not PointF confirmed
                || _activeExportLease is not null)
            {
                throw new InvalidOperationException(
                    "one confirmed marker is required for export");
            }

            var lease = new LocalAlignmentExportLease(confirmed);
            _activeExportLease = lease;
            return lease;
        }
    }

    internal void CompleteExport(LocalAlignmentExportLease lease)
    {
        ArgumentNullException.ThrowIfNull(lease);
        lock (_sync)
        {
            if (_phase != LocalAlignmentCapturePhase.ReadyToExport
                || !ReferenceEquals(_activeExportLease, lease))
            {
                throw new InvalidOperationException(
                    "only the active export lease can complete publication");
            }

            _phase = LocalAlignmentCapturePhase.Exported;
            _activeExportLease = null;
        }
    }

    internal void ReleaseExport(LocalAlignmentExportLease lease)
    {
        ArgumentNullException.ThrowIfNull(lease);
        lock (_sync)
        {
            if (_phase != LocalAlignmentCapturePhase.ReadyToExport
                || !ReferenceEquals(_activeExportLease, lease))
            {
                throw new InvalidOperationException(
                    "only the active export lease can release publication");
            }

            _activeExportLease = null;
        }
    }

    private static PointF RequireMapPoint(PointF mapPoint)
    {
        if (!float.IsFinite(mapPoint.X)
            || !float.IsFinite(mapPoint.Y)
            || mapPoint.X < 0
            || mapPoint.X >= MapWidthPx
            || mapPoint.Y < 0
            || mapPoint.Y >= MapHeightPx)
        {
            throw new ArgumentOutOfRangeException(
                nameof(mapPoint),
                "marker coordinates must be finite and inside the 2048px map");
        }

        return new PointF(
            mapPoint.X == 0 ? 0 : mapPoint.X,
            mapPoint.Y == 0 ? 0 : mapPoint.Y);
    }
}
