using System.Drawing;
using System.Reflection;
using PalMapPack.Calibrator;

namespace PalMapPack.Tests;

public sealed class LocalAlignmentCaptureFormModelTests
{
    [Fact]
    public void OverviewClickRejectsLetterboxWithoutChangingPhase()
    {
        var model = new LocalAlignmentCaptureFormModel();

        Assert.False(model.TryCaptureOverview(
            new Size(1_000, 600),
            new Point(199, 300)));
        Assert.Equal(
            LocalAlignmentCapturePhase.AwaitingOverview,
            model.Phase);
        Assert.Null(model.OverviewPoint);
        Assert.Null(model.PrecisionViewport);
    }

    [Theory]
    [InlineData(199, 300, false)]
    [InlineData(200, 300, true)]
    [InlineData(799, 300, true)]
    [InlineData(800, 300, false)]
    public void HorizontalLetterboxUsesHalfOpenRenderedMapBounds(
        int x,
        int y,
        bool expected)
    {
        var model = new LocalAlignmentCaptureFormModel();

        Assert.Equal(
            expected,
            model.TryCaptureOverview(
                new Size(1_000, 600),
                new Point(x, y)));
    }

    [Theory]
    [InlineData(300, 199, false)]
    [InlineData(300, 200, true)]
    [InlineData(300, 799, true)]
    [InlineData(300, 800, false)]
    public void VerticalLetterboxUsesHalfOpenRenderedMapBounds(
        int x,
        int y,
        bool expected)
    {
        var model = new LocalAlignmentCaptureFormModel();

        Assert.Equal(
            expected,
            model.TryCaptureOverview(
                new Size(600, 1_000),
                new Point(x, y)));
    }

    [Theory]
    [InlineData(249, 250, false)]
    [InlineData(250, 250, true)]
    [InlineData(749, 250, true)]
    [InlineData(750, 250, false)]
    public void OddClientDimensionsShareExactHalfOpenRenderedBounds(
        int x,
        int y,
        bool expected)
    {
        var model = new LocalAlignmentCaptureFormModel();

        Assert.Equal(
            expected,
            model.TryCaptureOverview(
                new Size(1_001, 500),
                new Point(x, y)));
    }

    [Theory]
    [InlineData(0, 500)]
    [InlineData(500, 0)]
    [InlineData(-1, 500)]
    [InlineData(500, -1)]
    public void InvalidOverviewControlSizeRejectsClick(int width, int height)
    {
        var model = new LocalAlignmentCaptureFormModel();

        Assert.False(model.TryCaptureOverview(
            new Size(width, height),
            Point.Empty));
    }

    [Fact]
    public void OverviewClickInsideRenderedMapCreatesPrecisionViewport()
    {
        var model = new LocalAlignmentCaptureFormModel();

        Assert.True(model.TryCaptureOverview(
            new Size(1_000, 600),
            new Point(500, 300)));

        Assert.Equal(
            LocalAlignmentCapturePhase.AwaitingPrecisionConfirmation,
            model.Phase);
        Assert.Equal(new PointF(1_024, 1_024), model.OverviewPoint);
        Assert.NotNull(model.PrecisionViewport);
        Assert.Equal(
            new RectangleF(992, 992, 64, 64),
            model.PrecisionViewport!.Value.PreviewSourceRectangle);
        Assert.False(model.CanCapture);
    }

    [Fact]
    public void PrecisionConfirmationRejectsClickOutsidePrecisionControl()
    {
        var model = CreateModelWithOverview();

        Assert.False(model.TryConfirmPrecision(
            new Size(256, 256),
            new Point(256, 128)));

        Assert.Equal(
            LocalAlignmentCapturePhase.AwaitingPrecisionConfirmation,
            model.Phase);
        Assert.Null(model.ConfirmedPoint);
        Assert.False(model.CanCapture);
    }

    [Fact]
    public void CaptureEnablesOnlyAfterPrecisionConfirmation()
    {
        var model = CreateModelWithOverview();

        Assert.False(model.CanCapture);
        Assert.True(model.TryConfirmPrecision(
            new Size(256, 256),
            new Point(128, 128)));

        Assert.Equal(LocalAlignmentCapturePhase.ReadyToExport, model.Phase);
        Assert.Equal(new PointF(1_024, 1_024), model.ConfirmedPoint);
        Assert.True(model.CanCapture);
    }

    [Fact]
    public void EdgeClampedPrecisionAcceptsLastControlPixel()
    {
        var model = new LocalAlignmentCaptureFormModel();
        Assert.True(model.TryCaptureOverview(
            new Size(2_048, 2_048),
            Point.Empty));
        Assert.Equal(
            new RectangleF(0, 0, 64, 64),
            model.PrecisionViewport!.Value.PreviewSourceRectangle);

        Assert.True(model.TryConfirmPrecision(
            new Size(256, 256),
            new Point(255, 255)));

        var confirmed = Assert.IsType<PointF>(model.ConfirmedPoint);
        Assert.Equal(63.75f, confirmed.X);
        Assert.Equal(63.75f, confirmed.Y);
        Assert.True(model.CanCapture);
    }

    [Fact]
    public void SecondOverviewOrPrecisionClickIsRejected()
    {
        var model = CreateModelWithOverview();

        Assert.False(model.TryCaptureOverview(
            new Size(1_000, 600),
            new Point(501, 300)));
        Assert.True(model.TryConfirmPrecision(
            new Size(256, 256),
            new Point(128, 128)));
        Assert.False(model.TryConfirmPrecision(
            new Size(256, 256),
            new Point(129, 128)));
    }

    [Fact]
    public void ResetReturnsToFreshTwoClickCapture()
    {
        var model = CreateModelWithOverview();
        Assert.True(model.TryConfirmPrecision(
            new Size(256, 256),
            new Point(128, 128)));

        model.Reset();

        Assert.Equal(
            LocalAlignmentCapturePhase.AwaitingOverview,
            model.Phase);
        Assert.Null(model.OverviewPoint);
        Assert.Null(model.ConfirmedPoint);
        Assert.Null(model.PrecisionViewport);
        Assert.False(model.CanCapture);
        Assert.False(model.ExportSucceeded);
    }

    [Fact]
    public void ClosingBeforeOwnerOnlyExportCannotReportSuccess()
    {
        var model = CreateModelWithOverview();
        Assert.True(model.TryConfirmPrecision(
            new Size(256, 256),
            new Point(128, 128)));

        Assert.False(model.ExportSucceeded);
        Assert.Equal(1, model.ExitCode);
        Assert.False(
            typeof(LocalAlignmentCaptureFormModel)
                .GetProperty(nameof(LocalAlignmentCaptureFormModel.ExportSucceeded))!
                .CanWrite);
        Assert.False(
            typeof(LocalAlignmentCaptureFormModel)
                .GetProperty(nameof(LocalAlignmentCaptureFormModel.ExitCode))!
                .CanWrite);
    }

    [Fact]
    public void ModelAndFormDeclareNoLiveOrPredictedCoordinateSurface()
    {
        var forbidden = new[]
        {
            "world",
            "predicted",
            "yaw",
            "pid",
            "hwnd",
            "account",
            "transform",
            "residual",
        };
        var types = new[]
        {
            typeof(LocalAlignmentCaptureFormModel),
            typeof(LocalAlignmentCaptureForm),
        };

        foreach (var type in types)
        {
            var declaredMembers = type
                .GetMembers(
                    BindingFlags.Instance
                    | BindingFlags.Static
                    | BindingFlags.Public
                    | BindingFlags.NonPublic
                    | BindingFlags.DeclaredOnly)
                .Select(member => member.Name)
                .ToArray();
            foreach (var word in forbidden)
            {
                Assert.DoesNotContain(
                    declaredMembers,
                    name => name.Contains(word, StringComparison.OrdinalIgnoreCase));
            }
        }
    }

    [Fact]
    public void ExactMapContractPinsPreviewAndSourceAssetIdentity()
    {
        var contract = LocalAlignmentCaptureMap.ExactContract;

        Assert.Equal(2_048, contract.WidthPx);
        Assert.Equal(2_048, contract.HeightPx);
        Assert.Equal(12_582_966, contract.FileSizeBytes);
        Assert.Equal(
            "aa81bdddbc525898b0a70b238cc936d0f8b0416c4ead922797b066d871938d8b",
            contract.Sha256);
        Assert.Equal(
            "e2a506ee5f7f0bc4d35af596e35bcf07e48e2c541f314f408d82104b4ef4f38e",
            contract.SourceMapAssetSha256);
    }

    [Fact]
    public void EmptyArgumentsKeepNormalCalibratorDispatch()
    {
        var host = new RecordingLaunchHost(standardExitCode: 17);

        var exitCode = PalMapPack.Calibrator.Program.Dispatch(
            Array.Empty<string>(),
            host);

        Assert.Equal(17, exitCode);
        Assert.Equal(1, host.StandardCalls);
        Assert.Null(host.LastExpectedBuild);
        Assert.Equal(0, host.CaptureCalls);
    }

    [Fact]
    public void ExactBuildArgumentKeepsNormalCalibratorDispatch()
    {
        var host = new RecordingLaunchHost(standardExitCode: 23);

        var exitCode = PalMapPack.Calibrator.Program.Dispatch(
            ["--build", "24181527"],
            host);

        Assert.Equal(23, exitCode);
        Assert.Equal(1, host.StandardCalls);
        Assert.Equal("24181527", host.LastExpectedBuild);
        Assert.Equal(0, host.CaptureCalls);
    }

    [Fact]
    public void CaptureArgumentsDispatchOnlyNativeMarkerCapture()
    {
        var host = new RecordingLaunchHost(captureExitCode: 29);
        var mapPath = Path.GetFullPath("main-map.bmp");
        var outputPath = Path.GetFullPath("observation.json");

        var exitCode = PalMapPack.Calibrator.Program.Dispatch(
            [
                "--local-alignment-capture",
                "--build",
                "24181527",
                "--map",
                mapPath,
                "--output",
                outputPath,
            ],
            host);

        Assert.Equal(29, exitCode);
        Assert.Equal(0, host.StandardCalls);
        Assert.Equal(1, host.CaptureCalls);
        Assert.Equal(
            new LocalAlignmentCaptureCommand(
                "24181527",
                mapPath,
                outputPath),
            host.LastCaptureCommand);
        Assert.Equal(
            "Form",
            typeof(LocalAlignmentCaptureForm).BaseType?.Name);
        Assert.True(typeof(LocalAlignmentCaptureForm).IsSealed);
    }

    [Fact]
    public void MisplacedCaptureIntentFailsBeforeAnyNormalDispatch()
    {
        var host = new RecordingLaunchHost();

        Assert.Throws<ArgumentException>(() =>
            PalMapPack.Calibrator.Program.Dispatch(
                [
                    "--build",
                    "24181527",
                    "--local-alignment-capture",
                ],
                host));

        Assert.Equal(0, host.StandardCalls);
        Assert.Equal(0, host.CaptureCalls);
    }

    [Theory]
    [InlineData(1)]
    [InlineData(74)]
    public void CaptureCancelOrErrorExitCodeIsPropagated(int captureExitCode)
    {
        var host = new RecordingLaunchHost(captureExitCode: captureExitCode);

        var exitCode = PalMapPack.Calibrator.Program.Dispatch(
            CaptureArguments(),
            host);

        Assert.Equal(captureExitCode, exitCode);
        Assert.Equal(0, host.StandardCalls);
        Assert.Equal(1, host.CaptureCalls);
    }

    [Fact]
    public void InvalidNormalArgumentsRetainUsageFailureBeforeHostRuns()
    {
        var host = new RecordingLaunchHost();

        Assert.Throws<ArgumentException>(() =>
            PalMapPack.Calibrator.Program.Dispatch(["--unknown"], host));

        Assert.Equal(0, host.StandardCalls);
        Assert.Equal(0, host.CaptureCalls);
    }

    [Fact]
    public void ErrorExitCodeChangesOnlyForCaptureIntent()
    {
        Assert.Equal(
            0,
            PalMapPack.Calibrator.Program.ErrorExitCode(["--unknown"]));
        Assert.Equal(
            1,
            PalMapPack.Calibrator.Program.ErrorExitCode(CaptureArguments()));
        Assert.Equal(
            1,
            PalMapPack.Calibrator.Program.ErrorExitCode(
                [
                    "--build",
                    "24181527",
                    "--local-alignment-capture",
                ]));
    }

    [Fact]
    public void NormalModeHandlesOnlyItsOriginalExceptionSet()
    {
        var arguments = Array.Empty<string>();

        Assert.True(PalMapPack.Calibrator.Program.ShouldHandle(
            new ArgumentException("invalid argument"),
            arguments));
        Assert.True(PalMapPack.Calibrator.Program.ShouldHandle(
            new InvalidOperationException("invalid state"),
            arguments));
        Assert.True(PalMapPack.Calibrator.Program.ShouldHandle(
            new MapPackFailure(1, "map-pack failure"),
            arguments));
        Assert.False(PalMapPack.Calibrator.Program.ShouldHandle(
            new IOException("file failure"),
            arguments));
        Assert.False(PalMapPack.Calibrator.Program.ShouldHandle(
            new UnauthorizedAccessException("access failure"),
            arguments));
        Assert.False(PalMapPack.Calibrator.Program.ShouldHandle(
            new OutOfMemoryException("image decode failure"),
            arguments));
        Assert.False(PalMapPack.Calibrator.Program.ShouldHandle(
            new Exception("unexpected failure"),
            arguments));
    }

    [Fact]
    public void CaptureModeAdditionallyHandlesItsLocalFileAndImageFailures()
    {
        var arguments = CaptureArguments();

        Assert.True(PalMapPack.Calibrator.Program.ShouldHandle(
            new ArgumentException("invalid argument"),
            arguments));
        Assert.True(PalMapPack.Calibrator.Program.ShouldHandle(
            new InvalidOperationException("invalid state"),
            arguments));
        Assert.True(PalMapPack.Calibrator.Program.ShouldHandle(
            new MapPackFailure(1, "map-pack failure"),
            arguments));
        Assert.True(PalMapPack.Calibrator.Program.ShouldHandle(
            new IOException("file failure"),
            arguments));
        Assert.True(PalMapPack.Calibrator.Program.ShouldHandle(
            new UnauthorizedAccessException("access failure"),
            arguments));
        Assert.True(PalMapPack.Calibrator.Program.ShouldHandle(
            new OutOfMemoryException("image decode failure"),
            arguments));
        Assert.False(PalMapPack.Calibrator.Program.ShouldHandle(
            new Exception("unexpected failure"),
            arguments));
        Assert.Equal(
            1,
            PalMapPack.Calibrator.Program.ErrorExitCode(arguments));
    }

    private static LocalAlignmentCaptureFormModel CreateModelWithOverview()
    {
        var model = new LocalAlignmentCaptureFormModel();
        Assert.True(model.TryCaptureOverview(
            new Size(1_000, 600),
            new Point(500, 300)));
        return model;
    }

    private static string[] CaptureArguments() =>
    [
        "--local-alignment-capture",
        "--build",
        "24181527",
        "--map",
        Path.GetFullPath("main-map.bmp"),
        "--output",
        Path.GetFullPath("observation.json"),
    ];

    private sealed class RecordingLaunchHost : ICalibratorLaunchHost
    {
        private readonly int _standardExitCode;
        private readonly int _captureExitCode;

        public RecordingLaunchHost(
            int standardExitCode = 0,
            int captureExitCode = 0)
        {
            _standardExitCode = standardExitCode;
            _captureExitCode = captureExitCode;
        }

        public int StandardCalls { get; private set; }

        public int CaptureCalls { get; private set; }

        public string? LastExpectedBuild { get; private set; }

        public LocalAlignmentCaptureCommand? LastCaptureCommand { get; private set; }

        public int RunStandard(string? expectedBuild)
        {
            StandardCalls++;
            LastExpectedBuild = expectedBuild;
            return _standardExitCode;
        }

        public int RunLocalAlignmentCapture(
            LocalAlignmentCaptureCommand command)
        {
            CaptureCalls++;
            LastCaptureCommand = command;
            return _captureExitCode;
        }
    }
}
