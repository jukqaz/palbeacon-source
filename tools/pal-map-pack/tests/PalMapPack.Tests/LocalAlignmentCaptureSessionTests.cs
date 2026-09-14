using System.Drawing;
using PalMapPack.Calibrator;

namespace PalMapPack.Tests;

public sealed class LocalAlignmentCaptureSessionTests
{
    [Fact]
    public void TwoClicksAdvanceThroughPrecisionConfirmationToOneExport()
    {
        var session = new LocalAlignmentCaptureSession();
        Assert.Equal(LocalAlignmentCapturePhase.AwaitingOverview, session.Phase);
        Assert.Null(session.OverviewPoint);
        Assert.Null(session.ConfirmedPoint);

        session.CaptureOverview(new PointF(100, 200));
        Assert.Equal(
            LocalAlignmentCapturePhase.AwaitingPrecisionConfirmation,
            session.Phase);
        Assert.Equal(new PointF(100, 200), session.OverviewPoint);

        session.ConfirmPrecision(new PointF(100.25f, 199.75f));
        Assert.Equal(LocalAlignmentCapturePhase.ReadyToExport, session.Phase);
        Assert.Equal(new PointF(100.25f, 199.75f), session.ConfirmedPoint);
        var lease = session.ReserveConfirmedPointForExport();
        Assert.Equal(new PointF(100.25f, 199.75f), lease.ObservedMarker);
        Assert.Throws<InvalidOperationException>(() =>
            session.ReserveConfirmedPointForExport());

        session.CompleteExport(lease);
        Assert.Equal(LocalAlignmentCapturePhase.Exported, session.Phase);
        Assert.Throws<InvalidOperationException>(() =>
            session.ReserveConfirmedPointForExport());
        Assert.Throws<InvalidOperationException>(() =>
            session.CompleteExport(lease));
        Assert.Throws<InvalidOperationException>(() =>
            session.ReleaseExport(lease));
    }

    [Fact]
    public void OverviewRejectsNonFiniteAndOutOfMapPoints()
    {
        foreach (var point in InvalidMapPoints())
        {
            var session = new LocalAlignmentCaptureSession();
            Assert.Throws<ArgumentOutOfRangeException>(() =>
                session.CaptureOverview(point));
            Assert.Equal(LocalAlignmentCapturePhase.AwaitingOverview, session.Phase);
        }
    }

    [Fact]
    public void PrecisionRejectsInvalidPointsAndPointsOutsideTheOverviewViewport()
    {
        var invalid = new[]
        {
            new PointF(float.NaN, 200),
            new PointF(100, float.PositiveInfinity),
            new PointF(-1, 200),
            new PointF(100, 2_048),
            new PointF(67.99f, 200),
            new PointF(132, 200),
            new PointF(100, 167.99f),
            new PointF(100, 232),
        };

        foreach (var point in invalid)
        {
            var session = new LocalAlignmentCaptureSession();
            session.CaptureOverview(new PointF(100, 200));
            Assert.Throws<ArgumentOutOfRangeException>(() =>
                session.ConfirmPrecision(point));
            Assert.Equal(
                LocalAlignmentCapturePhase.AwaitingPrecisionConfirmation,
                session.Phase);
        }
    }

    [Fact]
    public void PrecisionViewportClampsAtMapEdges()
    {
        var session = new LocalAlignmentCaptureSession();
        session.CaptureOverview(PointF.Empty);
        session.ConfirmPrecision(new PointF(63.999f, 63.999f));
        Assert.Equal(LocalAlignmentCapturePhase.ReadyToExport, session.Phase);

        var outside = new LocalAlignmentCaptureSession();
        outside.CaptureOverview(PointF.Empty);
        Assert.Throws<ArgumentOutOfRangeException>(() =>
            outside.ConfirmPrecision(new PointF(64, 0)));
    }

    [Fact]
    public void StateMachineRejectsOutOfOrderAndRepeatedOperations()
    {
        var session = new LocalAlignmentCaptureSession();
        Assert.Throws<InvalidOperationException>(() =>
            session.ConfirmPrecision(new PointF(100, 100)));
        Assert.Throws<InvalidOperationException>(() =>
            session.ReserveConfirmedPointForExport());

        session.CaptureOverview(new PointF(100, 200));
        Assert.Throws<InvalidOperationException>(() =>
            session.CaptureOverview(new PointF(101, 201)));
        Assert.Throws<InvalidOperationException>(() =>
            session.ReserveConfirmedPointForExport());

        session.ConfirmPrecision(new PointF(100, 200));
        Assert.Throws<InvalidOperationException>(() =>
            session.ConfirmPrecision(new PointF(100, 200)));
    }

    [Fact]
    public void OnlyTheActiveOpaqueLeaseCanCompleteOrReleaseAnExport()
    {
        var first = ConfirmedSession();
        var active = first.ReserveConfirmedPointForExport();
        var second = ConfirmedSession();
        var foreign = second.ReserveConfirmedPointForExport();

        Assert.Throws<InvalidOperationException>(() =>
            first.CompleteExport(foreign));
        Assert.Throws<InvalidOperationException>(() =>
            first.ReleaseExport(foreign));
        Assert.Equal(LocalAlignmentCapturePhase.ReadyToExport, first.Phase);

        first.ReleaseExport(active);
        var replacement = first.ReserveConfirmedPointForExport();
        Assert.Throws<InvalidOperationException>(() =>
            first.CompleteExport(active));
        Assert.Throws<InvalidOperationException>(() =>
            first.ReleaseExport(active));
        Assert.Equal(LocalAlignmentCapturePhase.ReadyToExport, first.Phase);

        first.CompleteExport(replacement);
        Assert.Equal(LocalAlignmentCapturePhase.Exported, first.Phase);
        second.ReleaseExport(foreign);
    }

    private static LocalAlignmentCaptureSession ConfirmedSession()
    {
        var session = new LocalAlignmentCaptureSession();
        session.CaptureOverview(new PointF(100, 200));
        session.ConfirmPrecision(new PointF(100.25f, 199.75f));
        return session;
    }

    private static PointF[] InvalidMapPoints() =>
    [
        new(float.NaN, 0),
        new(0, float.NaN),
        new(float.PositiveInfinity, 0),
        new(0, float.NegativeInfinity),
        new(-0.01f, 0),
        new(0, -0.01f),
        new(2_048, 0),
        new(0, 2_048),
    ];
}
