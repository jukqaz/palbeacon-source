using System.Globalization;
using System.Drawing.Drawing2D;
using System.Text.Json;

namespace PalMapPack.Calibrator;

public sealed class MainForm : Form
{
    private readonly string _gameBuildId;
    private readonly PictureBox _map = new()
    {
        Dock = DockStyle.Fill,
        SizeMode = PictureBoxSizeMode.Zoom,
        BackColor = Color.FromArgb(20, 20, 20),
        Cursor = Cursors.Cross,
    };
    private readonly SelectablePictureBox _precisionMap = new()
    {
        Width = 256,
        Height = 256,
        BackColor = Color.FromArgb(20, 20, 20),
        BorderStyle = BorderStyle.FixedSingle,
        Cursor = Cursors.Cross,
    };
    private readonly Label _precisionHelp = new()
    {
        AutoSize = false,
        Dock = DockStyle.Fill,
        Padding = new Padding(8),
        Text = "1. Click the overview map to seed a location.\n"
            + "2. Click this 4x magnifier to confirm it.\n"
            + "3. Use arrow keys for 1-pixel nudge (Shift: 0.25).\n\n"
            + "Capture stays disabled until precision confirmation.",
    };
    private readonly TextBox _id = new() { Width = 180 };
    private readonly TextBox _worldX = new() { Width = 120 };
    private readonly TextBox _worldY = new() { Width = 120 };
    private readonly Label _selectedPixel = new()
    {
        AutoSize = true,
        Text = "Map pixel: click the loaded map",
    };
    private readonly Label _progress = new()
    {
        AutoSize = true,
        Padding = new Padding(8),
    };
    private readonly DataGridView _grid = new()
    {
        Dock = DockStyle.Fill,
        AllowUserToAddRows = false,
        AllowUserToDeleteRows = false,
        AllowUserToResizeRows = false,
        AutoSizeColumnsMode = DataGridViewAutoSizeColumnsMode.Fill,
        MultiSelect = false,
        ReadOnly = true,
        RowHeadersVisible = false,
        SelectionMode = DataGridViewSelectionMode.FullRowSelect,
    };
    private readonly Button _openContract = new()
    {
        Text = "Open exact-Build candidate contract",
        AutoSize = true,
    };
    private readonly Button _openMap = new()
    {
        Text = "Open locally extracted MainMap",
        AutoSize = true,
    };
    private readonly Button _capture = new()
    {
        Text = "Capture sealed holdout",
        AutoSize = true,
    };
    private readonly Button _remove = new() { Text = "Remove selected", AutoSize = true };
    private readonly Button _exportHoldouts = new()
    {
        Text = "Freeze/export 5 holdouts",
        AutoSize = true,
    };
    private readonly Button _unlockReferences = new()
    {
        Text = "Verify commitment + reviewed contract",
        AutoSize = true,
    };
    private readonly Button _exportFinal = new()
    {
        Text = "Export final 15 observations",
        AutoSize = true,
    };

    private AssetContract? _candidateContract;
    private MapRegionContract? _mainMapContract;
    private LandmarkCaptureSession? _session;
    private PointF? _pendingMapPoint;
    private PrecisionMapViewport? _precisionViewport;
    private bool _precisionConfirmed;

    public MainForm(string gameBuildId)
    {
        _gameBuildId = gameBuildId;
        Text = $"Pal Map Pack - Local Landmark Calibrator - Build {_gameBuildId}";
        Width = 1_440;
        Height = 960;
        MinimumSize = new Size(1_000, 700);
        ConfigureGrid();
        Controls.Add(BuildLayout());

        _openContract.Click += OpenCandidateContract;
        _openMap.Click += OpenLocalMap;
        _map.MouseClick += MapClicked;
        _precisionMap.Paint += PaintPrecisionMap;
        _precisionMap.MouseClick += PrecisionMapClicked;
        _precisionMap.KeyDown += PrecisionMapKeyDown;
        _capture.Click += CaptureClicked;
        _remove.Click += RemoveClicked;
        _grid.SelectionChanged += (_, _) => UpdateState();
        _exportHoldouts.Click += ExportHoldouts;
        _unlockReferences.Click += UnlockReferences;
        _exportFinal.Click += ExportFinal;
        UpdateState();
    }

    protected override void Dispose(bool disposing)
    {
        if (disposing)
        {
            _map.Image?.Dispose();
            _map.Dispose();
            _precisionMap.Dispose();
            _grid.Dispose();
        }
        base.Dispose(disposing);
    }

    private Control BuildLayout()
    {
        var toolbar = new FlowLayoutPanel
        {
            AutoSize = true,
            Dock = DockStyle.Top,
            Padding = new Padding(8),
            WrapContents = true,
        };
        toolbar.Controls.Add(_openContract);
        toolbar.Controls.Add(_openMap);
        toolbar.Controls.Add(new Label
        {
            AutoSize = true,
            Padding = new Padding(8, 7, 0, 0),
            Text = $"Detected exact Build: {_gameBuildId}",
        });
        AddLabeled(toolbar, "ID", _id);
        AddLabeled(toolbar, "World X", _worldX);
        AddLabeled(toolbar, "World Y", _worldY);
        toolbar.Controls.Add(_capture);
        toolbar.Controls.Add(_selectedPixel);

        var actions = new FlowLayoutPanel
        {
            AutoSize = true,
            Dock = DockStyle.Bottom,
            Padding = new Padding(8),
            WrapContents = true,
        };
        actions.Controls.Add(_remove);
        actions.Controls.Add(_exportHoldouts);
        actions.Controls.Add(_unlockReferences);
        actions.Controls.Add(_exportFinal);
        actions.Controls.Add(new Label
        {
            AutoSize = true,
            ForeColor = Color.DarkRed,
            Padding = new Padding(12, 6, 0, 0),
            Text = "Reference capture stays locked until the reviewed contract precommits the frozen holdout hash.",
        });

        var right = new Panel { Dock = DockStyle.Fill };
        right.Controls.Add(_grid);
        right.Controls.Add(_progress);
        _progress.Dock = DockStyle.Top;

        var split = new SplitContainer
        {
            Dock = DockStyle.Fill,
            Orientation = Orientation.Vertical,
            SplitterDistance = 720,
            Panel1MinSize = 500,
            Panel2MinSize = 420,
        };
        var precisionArea = new TableLayoutPanel
        {
            ColumnCount = 2,
            Dock = DockStyle.Fill,
            Padding = new Padding(8),
            RowCount = 1,
        };
        precisionArea.ColumnStyles.Add(new ColumnStyle(SizeType.Absolute, 272));
        precisionArea.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100));
        precisionArea.RowStyles.Add(new RowStyle(SizeType.Percent, 100));
        precisionArea.Controls.Add(_precisionMap, 0, 0);
        precisionArea.Controls.Add(_precisionHelp, 1, 0);

        var mapLayout = new TableLayoutPanel
        {
            ColumnCount = 1,
            Dock = DockStyle.Fill,
            RowCount = 2,
        };
        mapLayout.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100));
        mapLayout.RowStyles.Add(new RowStyle(SizeType.Percent, 100));
        mapLayout.RowStyles.Add(new RowStyle(SizeType.Absolute, 280));
        mapLayout.Controls.Add(_map, 0, 0);
        mapLayout.Controls.Add(precisionArea, 0, 1);
        split.Panel1.Controls.Add(mapLayout);
        split.Panel2.Controls.Add(right);

        var root = new Panel { Dock = DockStyle.Fill };
        root.Controls.Add(split);
        root.Controls.Add(actions);
        root.Controls.Add(toolbar);
        return root;
    }

    private static void AddLabeled(FlowLayoutPanel panel, string label, Control control)
    {
        panel.Controls.Add(new Label
        {
            AutoSize = true,
            Padding = new Padding(8, 7, 0, 0),
            Text = label,
        });
        panel.Controls.Add(control);
    }

    private void ConfigureGrid()
    {
        _grid.Columns.Add("id", "ID");
        _grid.Columns.Add("zone", "Zone");
        _grid.Columns.Add("set", "Evidence");
        _grid.Columns.Add("world", "World X / Y");
        _grid.Columns.Add("pixel", "Map X / Y");
    }

    private void OpenCandidateContract(object? sender, EventArgs eventArgs)
    {
        var path = SelectInputFile("Exact-Build asset contract|*.json",
            "Open candidate exact-Build asset contract");
        if (path is null)
        {
            return;
        }
        try
        {
            var contract = AssetContract.Load(path);
            if (contract.GameBuildId != _gameBuildId)
            {
                throw new InvalidOperationException("contract Build does not match the detected game Build");
            }
            var mainMap = contract.RequireCalibrationCandidate(_gameBuildId);
            if (_map.Image is not null)
            {
                throw new InvalidOperationException(
                    "restart the calibrator before replacing a contract-bound map context");
            }
            _candidateContract = contract;
            _mainMapContract = mainMap;
            TryStartSession();
            UpdateState();
        }
        catch (Exception error) when (error is MapPackFailure
            or InvalidOperationException
            or ArgumentException
            or UnauthorizedAccessException)
        {
            ShowError(error.Message);
        }
    }

    private void OpenLocalMap(object? sender, EventArgs eventArgs)
    {
        if (_candidateContract is null || _mainMapContract is null)
        {
            ShowError("Open the content-bound exact-Build candidate contract first.");
            return;
        }
        var path = SelectInputFile(
            "Deterministic calibration preview|*.bmp",
            "Open probe-generated exact-Build MainMap preview (never uploaded)");
        if (path is null)
        {
            return;
        }
        try
        {
            var preview = _mainMapContract.CalibrationPreview!;
            var verifiedBytes = CalibrationPreviewBuilder.VerifyExactFile(
                path,
                preview,
                _mainMapContract.MapAssetSha256!);
            using var stream = new MemoryStream(verifiedBytes, writable: false);
            using var loaded = Image.FromStream(stream);
            if (loaded.Width != preview.WidthPx || loaded.Height != preview.HeightPx)
            {
                throw new InvalidOperationException(
                    "calibration preview dimensions do not match the exact-Build contract");
            }
            var clone = new Bitmap(loaded);
            var previous = _map.Image;
            _map.Image = clone;
            previous?.Dispose();
            TryStartSession();
            UpdateState();
        }
        catch (Exception error) when (error is IOException
            or ArgumentException
            or InvalidOperationException
            or MapPackFailure
            or UnauthorizedAccessException
            or OutOfMemoryException)
        {
            ShowError(error.Message);
        }
    }

    private void TryStartSession()
    {
        if (_mainMapContract is null || _map.Image is null)
        {
            return;
        }
        var preview = _mainMapContract.CalibrationPreview
            ?? throw new InvalidOperationException(
                "candidate contract has no deterministic calibration preview");
        if (_map.Image.Width != preview.WidthPx || _map.Image.Height != preview.HeightPx)
        {
            throw new InvalidOperationException(
                "calibration preview dimensions do not match the exact-Build contract");
        }
        if (_session is not null && _session.Landmarks.Count != 0)
        {
            throw new InvalidOperationException(
                "a capture is already in progress; restart the calibrator to replace its context");
        }
        _session = new LandmarkCaptureSession(
            _gameBuildId,
            _candidateContract!);
        ClearPendingMapPoint("Map pixel: click the loaded map");
        _grid.Rows.Clear();
    }

    private void MapClicked(object? sender, MouseEventArgs eventArgs)
    {
        if (_map.Image is null
            || _mainMapContract is null
            || _session is null
            || !ZoomedImageCoordinateMapper.TryMap(
                _map.ClientSize,
                _map.Image.Size,
                new Size(_mainMapContract.MapWidthPx, _mainMapContract.MapHeightPx),
                eventArgs.Location,
                out var point))
        {
            ClearPendingMapPoint("Map pixel: click inside the loaded image");
            UpdateState();
            return;
        }
        _pendingMapPoint = point;
        _precisionConfirmed = false;
        _precisionViewport = PrecisionMapCoordinateMapper.TryCreateViewport(
            _map.Image.Size,
            new Size(_mainMapContract.MapWidthPx, _mainMapContract.MapHeightPx),
            point,
            out var viewport)
            ? viewport
            : null;
        _selectedPixel.Text = FormattableString.Invariant(
            $"Map pixel: {point.X:F2}, {point.Y:F2} (refine in magnifier)");
        _precisionMap.Invalidate();
        UpdateState();
    }

    private void PrecisionMapClicked(object? sender, MouseEventArgs eventArgs)
    {
        if (_precisionViewport is not { } viewport
            || !PrecisionMapCoordinateMapper.TryMap(
                _precisionMap.ClientSize,
                viewport,
                eventArgs.Location,
                out var point))
        {
            return;
        }

        _pendingMapPoint = point;
        _precisionConfirmed = true;
        _precisionMap.Focus();
        UpdateSelectedPixelText(point);
        _precisionMap.Invalidate();
        UpdateState();
    }

    private void PrecisionMapKeyDown(object? sender, KeyEventArgs eventArgs)
    {
        if (_pendingMapPoint is not { } point || _mainMapContract is null)
        {
            return;
        }

        var step = eventArgs.Shift ? 0.25f : 1f;
        var delta = eventArgs.KeyCode switch
        {
            Keys.Left => new PointF(-step, 0),
            Keys.Right => new PointF(step, 0),
            Keys.Up => new PointF(0, -step),
            Keys.Down => new PointF(0, step),
            _ => PointF.Empty,
        };
        if (delta.IsEmpty)
        {
            return;
        }

        var next = new PointF(
            Math.Clamp(
                point.X + delta.X,
                0,
                _mainMapContract.MapWidthPx - 0.01f),
            Math.Clamp(
                point.Y + delta.Y,
                0,
                _mainMapContract.MapHeightPx - 0.01f));
        if (_map.Image is null
            || !PrecisionMapCoordinateMapper.TryCreateViewport(
                _map.Image.Size,
                new Size(_mainMapContract.MapWidthPx, _mainMapContract.MapHeightPx),
                next,
                out var viewport))
        {
            ClearPendingMapPoint("Map pixel: precision point became invalid; select it again");
            UpdateState();
            return;
        }
        _pendingMapPoint = next;
        _precisionViewport = viewport;
        _precisionConfirmed = true;
        UpdateSelectedPixelText(_pendingMapPoint.Value);
        _precisionMap.Invalidate();
        eventArgs.Handled = true;
        eventArgs.SuppressKeyPress = true;
        UpdateState();
    }

    private void PaintPrecisionMap(object? sender, PaintEventArgs eventArgs)
    {
        eventArgs.Graphics.Clear(_precisionMap.BackColor);
        if (_map.Image is null || _precisionViewport is not { } viewport)
        {
            TextRenderer.DrawText(
                eventArgs.Graphics,
                "Select a point\non the overview map",
                Font,
                _precisionMap.ClientRectangle,
                Color.LightGray,
                TextFormatFlags.HorizontalCenter | TextFormatFlags.VerticalCenter);
            return;
        }

        eventArgs.Graphics.InterpolationMode = InterpolationMode.NearestNeighbor;
        eventArgs.Graphics.PixelOffsetMode = PixelOffsetMode.Half;
        var source = viewport.PreviewSourceRectangle;
        eventArgs.Graphics.DrawImage(
            _map.Image,
            _precisionMap.ClientRectangle,
            source.X,
            source.Y,
            source.Width,
            source.Height,
            GraphicsUnit.Pixel);

        if (_pendingMapPoint is not { } point
            || !PrecisionMapCoordinateMapper.TryProject(
                _precisionMap.ClientSize,
                viewport,
                point,
                out var projected))
        {
            return;
        }

        var x = (int)Math.Round(projected.X);
        var y = (int)Math.Round(projected.Y);
        eventArgs.Graphics.DrawLine(Pens.Black, x - 12, y, x + 12, y);
        eventArgs.Graphics.DrawLine(Pens.Black, x, y - 12, x, y + 12);
        eventArgs.Graphics.DrawLine(Pens.Lime, x - 10, y, x + 10, y);
        eventArgs.Graphics.DrawLine(Pens.Lime, x, y - 10, x, y + 10);
    }

    private void UpdateSelectedPixelText(PointF point)
    {
        var resolution = _precisionViewport is { } viewport
            ? PrecisionMapCoordinateMapper.MaxCoordinateUnitsPerControlPixel(
                _precisionMap.ClientSize,
                viewport)
            : double.PositiveInfinity;
        _selectedPixel.Text = FormattableString.Invariant(
            $"Map pixel: {point.X:F2}, {point.Y:F2} (precision confirmed, {resolution:F2} map px/screen px)");
    }

    private void ClearPendingMapPoint(string status)
    {
        _pendingMapPoint = null;
        _precisionViewport = null;
        _precisionConfirmed = false;
        _selectedPixel.Text = status;
        _precisionMap.Invalidate();
    }

    private void CaptureClicked(object? sender, EventArgs eventArgs)
    {
        if (_session is null || _pendingMapPoint is not { } point)
        {
            return;
        }
        if (!TryCoordinate(_worldX.Text, out var worldX)
            || !TryCoordinate(_worldY.Text, out var worldY))
        {
            ShowError("World X and Y must be finite numeric coordinates.");
            return;
        }
        try
        {
            _session.Capture(
                _id.Text.Trim(),
                worldX,
                worldY,
                point.X,
                point.Y);
            RefreshRows();
            _id.Clear();
            ClearPendingMapPoint("Map pixel: click the next landmark");
            UpdateState();
        }
        catch (Exception error) when (error is ArgumentException
            or InvalidOperationException
            or MapPackFailure)
        {
            ShowError(error.Message);
        }
    }

    private void RemoveClicked(object? sender, EventArgs eventArgs)
    {
        if (_session is null || _grid.SelectedRows.Count != 1)
        {
            return;
        }
        var id = _grid.SelectedRows[0].Cells[0].Value?.ToString();
        try
        {
            if (id is not null && _session.Remove(id))
            {
                RefreshRows();
                UpdateState();
            }
        }
        catch (InvalidOperationException error)
        {
            ShowError(error.Message);
        }
    }

    private void ExportHoldouts(object? sender, EventArgs eventArgs)
    {
        if (_session is null)
        {
            return;
        }
        var path = SelectOutputFile($"palworld-{_gameBuildId}-sealed-holdouts.json",
            "Freeze and export five sealed holdouts to a new private file");
        if (path is null)
        {
            return;
        }
        try
        {
            EnsureInstalledBuildStillMatches();
            LandmarkCaptureExporter.WriteHoldoutsNew(path, _session);
            ShowSaved();
            UpdateState();
        }
        catch (Exception error) when (error is IOException
            or UnauthorizedAccessException
            or InvalidOperationException
            or MapPackFailure)
        {
            ShowError(error.Message);
        }
    }

    private void UnlockReferences(object? sender, EventArgs eventArgs)
    {
        if (_session is null)
        {
            return;
        }
        var commitmentPath = SelectInputFile(
            "Holdout commitment|*.json",
            "Open the CLI-generated holdout commitment");
        if (commitmentPath is null)
        {
            return;
        }
        var contractPath = SelectInputFile(
            "Reviewed asset contract|*.json",
            "Open the independently reviewed exact-Build contract");
        if (contractPath is null)
        {
            return;
        }
        try
        {
            EnsureInstalledBuildStillMatches();
            var commitment = LandmarkCaptureExporter.ReadCommitment(commitmentPath);
            var reviewedContract = AssetContract.Load(contractPath);
            _session.ApproveReferenceCapture(commitment, reviewedContract);
            UpdateState();
        }
        catch (Exception error) when (error is IOException
            or UnauthorizedAccessException
            or ArgumentException
            or JsonException
            or MapPackFailure
            or InvalidOperationException)
        {
            ShowError(error.Message);
        }
    }

    private void ExportFinal(object? sender, EventArgs eventArgs)
    {
        if (_session is null)
        {
            return;
        }
        var path = SelectOutputFile($"palworld-{_gameBuildId}-landmarks.json",
            "Export the approved final 15 observations to a new private file");
        if (path is null)
        {
            return;
        }
        try
        {
            EnsureInstalledBuildStillMatches();
            LandmarkCaptureExporter.WriteFinalNew(path, _session);
            ShowSaved();
        }
        catch (Exception error) when (error is IOException
            or UnauthorizedAccessException
            or InvalidOperationException
            or MapPackFailure)
        {
            ShowError(error.Message);
        }
    }

    private void RefreshRows()
    {
        _grid.Rows.Clear();
        if (_session is null)
        {
            return;
        }
        foreach (var row in _session.Landmarks)
        {
            _grid.Rows.Add(
                row.Id,
                row.Zone,
                row.EvidenceSet,
                FormattableString.Invariant($"{row.WorldX:F2}, {row.WorldY:F2}"),
                FormattableString.Invariant($"{row.MapX:F2}, {row.MapY:F2}"));
        }
    }

    private void UpdateState()
    {
        var hasRows = _session is not null && _session.Landmarks.Count != 0;
        _openContract.Enabled = !hasRows && _map.Image is null;
        _openMap.Enabled = !hasRows && _mainMapContract is not null && _map.Image is null;
        _map.Enabled = _session is not null;
        _capture.Enabled = _session?.Phase is CapturePhase.CapturingHoldouts
                or CapturePhase.CapturingReferences
            && _pendingMapPoint is not null
            && _precisionConfirmed;
        _capture.Text = _session?.Phase == CapturePhase.CapturingReferences
            ? "Capture reference"
            : "Capture sealed holdout";
        _remove.Enabled = _session is not null && _grid.SelectedRows.Count == 1;
        _exportHoldouts.Enabled = _session?.Phase == CapturePhase.HoldoutsReadyForExport;
        _unlockReferences.Enabled =
            _session?.Phase == CapturePhase.AwaitingApprovedCommitment;
        _exportFinal.Enabled = _session?.Phase == CapturePhase.Complete;
        if (_session is null)
        {
            _progress.Text = _candidateContract is null
                ? "Open the exact-Build candidate contract and MainMap image."
                : "Contract bound. Open the exact-Build MainMap image.";
            return;
        }
        var lines = Enum.GetValues<LandmarkZone>().Select(zone =>
            $"{zone}: sealed {_session.Count(zone, LandmarkEvidenceSet.SealedHoldout)}/1, "
            + $"ref {_session.Count(zone, LandmarkEvidenceSet.Reference)}/2");
        _progress.Text = $"Phase: {_session.Phase}\nBuild {_gameBuildId}\n"
            + string.Join(Environment.NewLine, lines);
    }

    private static string? SelectInputFile(string filter, string title)
    {
        using var dialog = new OpenFileDialog
        {
            CheckFileExists = true,
            Filter = filter,
            Title = title,
        };
        return dialog.ShowDialog() == DialogResult.OK ? dialog.FileName : null;
    }

    private static string? SelectOutputFile(string fileName, string title)
    {
        using var dialog = new SaveFileDialog
        {
            AddExtension = true,
            DefaultExt = "json",
            Filter = "JSON|*.json",
            FileName = fileName,
            OverwritePrompt = true,
            Title = title,
        };
        return dialog.ShowDialog() == DialogResult.OK ? dialog.FileName : null;
    }

    private static bool TryCoordinate(string text, out double value) =>
        (double.TryParse(
            text,
            NumberStyles.Float,
            CultureInfo.InvariantCulture,
            out value)
        || double.TryParse(
            text,
            NumberStyles.Float,
            CultureInfo.CurrentCulture,
            out value))
        && double.IsFinite(value);

    private void EnsureInstalledBuildStillMatches()
    {
        var current = SteamInstallLocator.LocateDefault("1623730").BuildId;
        if (current != _gameBuildId)
        {
            throw new InvalidOperationException(
                "the installed Steam Build changed during capture; no output was written");
        }
    }

    private void ShowSaved() =>
        MessageBox.Show(
            this,
            "Saved locally. This tool never uploads observations.",
            "Capture exported",
            MessageBoxButtons.OK,
            MessageBoxIcon.Information);

    private void ShowError(string message) =>
        MessageBox.Show(
            this,
            message,
            "Capture refused",
            MessageBoxButtons.OK,
            MessageBoxIcon.Error);

    private sealed class SelectablePictureBox : PictureBox
    {
        public SelectablePictureBox()
        {
            SetStyle(ControlStyles.Selectable, true);
            TabStop = true;
        }

        protected override bool IsInputKey(Keys keyData) =>
            (keyData & Keys.KeyCode) is Keys.Left or Keys.Right or Keys.Up or Keys.Down
            || base.IsInputKey(keyData);
    }
}
