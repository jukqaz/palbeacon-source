using System.Drawing.Drawing2D;
using PalMapPack;

namespace PalMapPack.Calibrator;

internal static class LocalAlignmentCaptureMap
{
    internal const string ExactSourceMapAssetSha256 =
        "e2a506ee5f7f0bc4d35af596e35bcf07e48e2c541f314f408d82104b4ef4f38e";

    private const long ExactMapFileSize =
        54L
        + LocalAlignmentCaptureSession.MapWidthPx
            * (long)LocalAlignmentCaptureSession.MapHeightPx
            * 3L;

    internal static CalibrationPreviewContract ExactContract { get; } = new(
        LocalAlignmentCaptureSession.MapWidthPx,
        LocalAlignmentCaptureSession.MapHeightPx,
        ExactMapFileSize,
        LocalAlignmentObservationExporter.ExactMapSha256,
        ExactSourceMapAssetSha256,
        CalibrationPreviewBuilder.Algorithm);

    internal static Bitmap Load(string path)
    {
        var bytes = CalibrationPreviewBuilder.VerifyExactFile(
            path,
            ExactContract,
            ExactSourceMapAssetSha256);
        using var stream = new MemoryStream(bytes, writable: false);
        using var loaded = Image.FromStream(
            stream,
            useEmbeddedColorManagement: false,
            validateImageData: true);
        if (loaded.Width != LocalAlignmentCaptureSession.MapWidthPx
            || loaded.Height != LocalAlignmentCaptureSession.MapHeightPx)
        {
            throw new InvalidOperationException(
                "native marker capture requires the exact 2048px map");
        }

        return new Bitmap(loaded);
    }
}

public sealed class LocalAlignmentCaptureFormModel
{
    private static readonly Size ExactMapSize = new(
        LocalAlignmentCaptureSession.MapWidthPx,
        LocalAlignmentCaptureSession.MapHeightPx);

    private LocalAlignmentCaptureSession _session = new();
    private PrecisionMapViewport? _precisionViewport;

    public LocalAlignmentCapturePhase Phase => _session.Phase;

    public PointF? OverviewPoint => _session.OverviewPoint;

    public PointF? ConfirmedPoint => _session.ConfirmedPoint;

    public PrecisionMapViewport? PrecisionViewport => _precisionViewport;

    public bool CanCapture =>
        Phase == LocalAlignmentCapturePhase.ReadyToExport;

    public bool ExportSucceeded =>
        Phase == LocalAlignmentCapturePhase.Exported;

    public int ExitCode => ExportSucceeded ? 0 : 1;

    public Rectangle GetOverviewImageBounds(Size controlSize)
    {
        if (controlSize.Width <= 0 || controlSize.Height <= 0)
        {
            return Rectangle.Empty;
        }

        var side = Math.Min(controlSize.Width, controlSize.Height);
        return new Rectangle(
            (controlSize.Width - side) / 2,
            (controlSize.Height - side) / 2,
            side,
            side);
    }

    public bool TryCaptureOverview(Size controlSize, Point controlPoint)
    {
        if (Phase != LocalAlignmentCapturePhase.AwaitingOverview)
        {
            return false;
        }

        var imageBounds = GetOverviewImageBounds(controlSize);
        if (imageBounds.IsEmpty
            || controlPoint.X < imageBounds.Left
            || controlPoint.X >= imageBounds.Right
            || controlPoint.Y < imageBounds.Top
            || controlPoint.Y >= imageBounds.Bottom)
        {
            return false;
        }

        var point = new PointF(
            (float)((controlPoint.X - imageBounds.Left)
                * LocalAlignmentCaptureSession.MapWidthPx
                / (double)imageBounds.Width),
            (float)((controlPoint.Y - imageBounds.Top)
                * LocalAlignmentCaptureSession.MapHeightPx
                / (double)imageBounds.Height));
        if (!PrecisionMapCoordinateMapper.TryCreateViewport(
                ExactMapSize,
                ExactMapSize,
                point,
                out var viewport))
        {
            return false;
        }

        _session.CaptureOverview(point);
        _precisionViewport = viewport;
        return true;
    }

    public bool TryConfirmPrecision(Size controlSize, Point controlPoint)
    {
        if (Phase
                != LocalAlignmentCapturePhase.AwaitingPrecisionConfirmation
            || _precisionViewport is not { } viewport
            || !PrecisionMapCoordinateMapper.TryMap(
                controlSize,
                viewport,
                controlPoint,
                out var point))
        {
            return false;
        }

        _session.ConfirmPrecision(point);
        return true;
    }

    public bool TryProjectOverview(
        Size controlSize,
        PointF mapPoint,
        out PointF controlPoint)
    {
        controlPoint = default;
        if (!float.IsFinite(mapPoint.X)
            || !float.IsFinite(mapPoint.Y)
            || mapPoint.X < 0
            || mapPoint.X >= LocalAlignmentCaptureSession.MapWidthPx
            || mapPoint.Y < 0
            || mapPoint.Y >= LocalAlignmentCaptureSession.MapHeightPx)
        {
            return false;
        }

        var imageBounds = GetOverviewImageBounds(controlSize);
        if (imageBounds.IsEmpty)
        {
            return false;
        }

        controlPoint = new PointF(
            imageBounds.Left
                + mapPoint.X * imageBounds.Width
                    / LocalAlignmentCaptureSession.MapWidthPx,
            imageBounds.Top
                + mapPoint.Y * imageBounds.Height
                    / LocalAlignmentCaptureSession.MapHeightPx);
        return true;
    }

    public void Reset()
    {
        _session = new LocalAlignmentCaptureSession();
        _precisionViewport = null;
    }

    public void Export(
        string outputPath,
        ILocalAlignmentNonceSource nonceSource)
    {
        if (!CanCapture)
        {
            throw new InvalidOperationException(
                "precision confirmation is required before capture");
        }

        LocalAlignmentObservationExporter.WriteNew(
            outputPath,
            _session,
            nonceSource);
    }
}

public sealed class LocalAlignmentCaptureForm : Form
{
    private readonly LocalAlignmentCaptureCommand _command;
    private readonly LocalAlignmentCaptureFormModel _model = new();
    private readonly Bitmap _image;
    private readonly MapCanvas _overview = new()
    {
        Dock = DockStyle.Fill,
        BackColor = Color.FromArgb(18, 20, 23),
        Cursor = Cursors.Cross,
        TabStop = true,
    };
    private readonly MapCanvas _precision = new()
    {
        Dock = DockStyle.Fill,
        BackColor = Color.FromArgb(18, 20, 23),
        Cursor = Cursors.Cross,
        TabStop = true,
    };
    private readonly Label _status = new()
    {
        AutoSize = false,
        Dock = DockStyle.Fill,
        Padding = new Padding(10, 7, 10, 7),
        TextAlign = ContentAlignment.MiddleLeft,
    };
    private readonly Button _reset = new()
    {
        AutoSize = true,
        Text = "Reset",
    };
    private readonly Button _capture = new()
    {
        AutoSize = true,
        Text = "Capture / Validate",
    };
    private readonly Button _cancel = new()
    {
        AutoSize = true,
        DialogResult = DialogResult.Cancel,
        Text = "Cancel",
    };

    public LocalAlignmentCaptureForm(LocalAlignmentCaptureCommand command)
    {
        ArgumentNullException.ThrowIfNull(command);
        if (!string.Equals(
                command.BuildId,
                LocalAlignmentCaptureCommand.ExactBuildId,
                StringComparison.Ordinal))
        {
            throw new ArgumentException(
                "native marker capture requires exact Build 24181527",
                nameof(command));
        }

        _command = command;
        _image = LocalAlignmentCaptureMap.Load(command.MapPath);

        Text = "Pal Map Pack - Native Marker Capture";
        Width = 1_160;
        Height = 820;
        MinimumSize = new Size(900, 650);
        StartPosition = FormStartPosition.CenterScreen;
        FormBorderStyle = FormBorderStyle.Sizable;
        Controls.Add(BuildLayout());
        CancelButton = _cancel;

        _overview.Paint += PaintOverview;
        _overview.MouseClick += OverviewClicked;
        _precision.Paint += PaintPrecision;
        _precision.MouseClick += PrecisionClicked;
        _reset.Click += ResetClicked;
        _capture.Click += CaptureClicked;
        FormClosing += CaptureFormClosing;
        UpdateState();
    }

    public bool ExportSucceeded => _model.ExportSucceeded;

    public int ExitCode => _model.ExitCode;

    protected override void Dispose(bool disposing)
    {
        if (disposing)
        {
            _image.Dispose();
            _overview.Dispose();
            _precision.Dispose();
            _status.Dispose();
            _reset.Dispose();
            _capture.Dispose();
            _cancel.Dispose();
        }

        base.Dispose(disposing);
    }

    private Control BuildLayout()
    {
        var instructions = new Label
        {
            AutoSize = false,
            Dock = DockStyle.Fill,
            Padding = new Padding(12),
            Text = "Keep the character stationary. Open Palworld's native map, "
                + "then click the exact center of its character marker below. "
                + "Confirm the same point in the precision pane before capture.",
            TextAlign = ContentAlignment.MiddleLeft,
        };

        var panes = new TableLayoutPanel
        {
            ColumnCount = 2,
            Dock = DockStyle.Fill,
            Padding = new Padding(12, 0, 12, 0),
            RowCount = 2,
        };
        panes.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 68));
        panes.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 32));
        panes.RowStyles.Add(new RowStyle(SizeType.Absolute, 32));
        panes.RowStyles.Add(new RowStyle(SizeType.Percent, 100));
        panes.Controls.Add(CreatePaneTitle("1. Overview"), 0, 0);
        panes.Controls.Add(CreatePaneTitle("2. Precision confirmation"), 1, 0);
        panes.Controls.Add(_overview, 0, 1);
        panes.Controls.Add(_precision, 1, 1);

        var actions = new FlowLayoutPanel
        {
            AutoSize = true,
            Dock = DockStyle.Fill,
            FlowDirection = FlowDirection.RightToLeft,
            Padding = new Padding(8),
            WrapContents = false,
        };
        actions.Controls.Add(_capture);
        actions.Controls.Add(_reset);
        actions.Controls.Add(_cancel);

        var bottom = new TableLayoutPanel
        {
            ColumnCount = 2,
            Dock = DockStyle.Fill,
            RowCount = 1,
        };
        bottom.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100));
        bottom.ColumnStyles.Add(new ColumnStyle(SizeType.AutoSize));
        bottom.RowStyles.Add(new RowStyle(SizeType.Percent, 100));
        bottom.Controls.Add(_status, 0, 0);
        bottom.Controls.Add(actions, 1, 0);

        var root = new TableLayoutPanel
        {
            ColumnCount = 1,
            Dock = DockStyle.Fill,
            RowCount = 3,
        };
        root.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100));
        root.RowStyles.Add(new RowStyle(SizeType.Absolute, 76));
        root.RowStyles.Add(new RowStyle(SizeType.Percent, 100));
        root.RowStyles.Add(new RowStyle(SizeType.Absolute, 68));
        root.Controls.Add(instructions, 0, 0);
        root.Controls.Add(panes, 0, 1);
        root.Controls.Add(bottom, 0, 2);
        return root;
    }

    private static Label CreatePaneTitle(string text) =>
        new()
        {
            AutoSize = false,
            Dock = DockStyle.Fill,
            Padding = new Padding(4, 6, 4, 4),
            Text = text,
        };

    private void PaintOverview(object? sender, PaintEventArgs eventArgs)
    {
        eventArgs.Graphics.Clear(_overview.BackColor);
        var bounds = _model.GetOverviewImageBounds(_overview.ClientSize);
        if (bounds.IsEmpty)
        {
            return;
        }

        eventArgs.Graphics.InterpolationMode = InterpolationMode.HighQualityBicubic;
        eventArgs.Graphics.PixelOffsetMode = PixelOffsetMode.Half;
        eventArgs.Graphics.DrawImage(_image, bounds);
        if (_model.OverviewPoint is { } point
            && _model.TryProjectOverview(
                _overview.ClientSize,
                point,
                out var projected))
        {
            DrawCrosshair(eventArgs.Graphics, projected);
        }
    }

    private void PaintPrecision(object? sender, PaintEventArgs eventArgs)
    {
        eventArgs.Graphics.Clear(_precision.BackColor);
        if (_model.PrecisionViewport is not { } viewport)
        {
            TextRenderer.DrawText(
                eventArgs.Graphics,
                "Select the marker\nin the overview first.",
                Font,
                _precision.ClientRectangle,
                Color.Gainsboro,
                TextFormatFlags.HorizontalCenter
                | TextFormatFlags.VerticalCenter);
            return;
        }

        var source = viewport.PreviewSourceRectangle;
        eventArgs.Graphics.InterpolationMode = InterpolationMode.NearestNeighbor;
        eventArgs.Graphics.PixelOffsetMode = PixelOffsetMode.Half;
        eventArgs.Graphics.DrawImage(
            _image,
            _precision.ClientRectangle,
            source.X,
            source.Y,
            source.Width,
            source.Height,
            GraphicsUnit.Pixel);

        var point = _model.ConfirmedPoint ?? _model.OverviewPoint;
        if (point is not { } selected
            || !PrecisionMapCoordinateMapper.TryProject(
                _precision.ClientSize,
                viewport,
                selected,
                out var projected))
        {
            return;
        }

        DrawCrosshair(eventArgs.Graphics, projected);
    }

    private static void DrawCrosshair(Graphics graphics, PointF point)
    {
        var x = (int)Math.Round(point.X);
        var y = (int)Math.Round(point.Y);
        graphics.DrawLine(Pens.Black, x - 12, y, x + 12, y);
        graphics.DrawLine(Pens.Black, x, y - 12, x, y + 12);
        graphics.DrawLine(Pens.Lime, x - 10, y, x + 10, y);
        graphics.DrawLine(Pens.Lime, x, y - 10, x, y + 10);
    }

    private void OverviewClicked(object? sender, MouseEventArgs eventArgs)
    {
        if (!_model.TryCaptureOverview(
                _overview.ClientSize,
                eventArgs.Location))
        {
            _status.Text = _model.Phase
                    == LocalAlignmentCapturePhase.AwaitingOverview
                ? "Click inside the rendered overview map."
                : "Use Reset to select a different overview point.";
            return;
        }

        _precision.Focus();
        UpdateState();
    }

    private void PrecisionClicked(object? sender, MouseEventArgs eventArgs)
    {
        if (!_model.TryConfirmPrecision(
                _precision.ClientSize,
                eventArgs.Location))
        {
            return;
        }

        UpdateState();
    }

    private void ResetClicked(object? sender, EventArgs eventArgs)
    {
        _model.Reset();
        UpdateState();
    }

    private void CaptureClicked(object? sender, EventArgs eventArgs)
    {
        if (!_model.CanCapture)
        {
            return;
        }

        try
        {
            _model.Export(
                _command.OutputPath,
                new CryptographicLocalAlignmentNonceSource());
            if (!_model.ExportSucceeded)
            {
                throw new InvalidOperationException(
                    "owner-only capture publication did not complete");
            }

            DialogResult = DialogResult.OK;
            Close();
        }
        catch (Exception error) when (error is IOException
            or UnauthorizedAccessException
            or ArgumentException
            or InvalidOperationException
            or MapPackFailure)
        {
            MessageBox.Show(
                this,
                error.Message,
                "Capture refused",
                MessageBoxButtons.OK,
                MessageBoxIcon.Error);
            UpdateState();
        }
    }

    private void CaptureFormClosing(object? sender, FormClosingEventArgs eventArgs)
    {
        if (!_model.ExportSucceeded)
        {
            DialogResult = DialogResult.Cancel;
        }
    }

    private void UpdateState()
    {
        _capture.Enabled = _model.CanCapture;
        _reset.Enabled = !_model.ExportSucceeded
            && _model.Phase != LocalAlignmentCapturePhase.AwaitingOverview;
        _overview.Enabled =
            _model.Phase == LocalAlignmentCapturePhase.AwaitingOverview;
        _precision.Enabled = _model.Phase
            == LocalAlignmentCapturePhase.AwaitingPrecisionConfirmation;
        _status.Text = _model.Phase switch
        {
            LocalAlignmentCapturePhase.AwaitingOverview =>
                "Step 1 of 2: select the native marker in the overview.",
            LocalAlignmentCapturePhase.AwaitingPrecisionConfirmation =>
                "Step 2 of 2: confirm its exact center in the precision pane.",
            LocalAlignmentCapturePhase.ReadyToExport =>
                "Confirmed. Capture / Validate will publish one private observation.",
            LocalAlignmentCapturePhase.Exported =>
                "Private observation published.",
            _ => "Capture is unavailable.",
        };
        _overview.Invalidate();
        _precision.Invalidate();
    }

    private sealed class MapCanvas : Control
    {
        public MapCanvas()
        {
            DoubleBuffered = true;
            ResizeRedraw = true;
        }
    }
}
