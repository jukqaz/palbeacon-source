namespace PalMapPack.Calibrator;

public enum CapturePhase
{
    CapturingHoldouts = 0,
    HoldoutsReadyForExport = 1,
    AwaitingApprovedCommitment = 2,
    CapturingReferences = 3,
    Complete = 4,
}

public sealed class LandmarkCaptureSession
{
    private readonly string _gameBuildId;
    private readonly int _mapWidth;
    private readonly int _mapHeight;
    private readonly MapRegionContract _mainMap;
    private readonly string _candidateIdentitySha256;
    private readonly string _candidateMappingSha256;
    private readonly List<Landmark> _landmarks = [];

    public LandmarkCaptureSession(
        string gameBuildId,
        AssetContract candidateContract)
    {
        if (string.IsNullOrEmpty(gameBuildId)
            || gameBuildId.Any(character => !char.IsAsciiDigit(character)))
        {
            throw new ArgumentException(
                "exact numeric game Build identity is required",
                nameof(gameBuildId));
        }
        ArgumentNullException.ThrowIfNull(candidateContract);
        var mainMap = candidateContract.RequireCalibrationCandidate(gameBuildId);
        if (mainMap.MapId != "MainMap"
            || mainMap.RegionId != "FirstRegion"
            || !FiniteBounds(mainMap)
            || mainMap.MapWidthPx <= 0
            || mainMap.MapHeightPx <= 0)
        {
            throw new ArgumentException(
                "capture context requires the authoritative MainMap/FirstRegion bounds",
                nameof(mainMap));
        }
        _gameBuildId = gameBuildId;
        _mapWidth = mainMap.MapWidthPx;
        _mapHeight = mainMap.MapHeightPx;
        _mainMap = mainMap;
        _candidateIdentitySha256 = candidateContract.CandidateIdentitySha256();
        _candidateMappingSha256 = candidateContract.MappingCandidate!.Sha256;
    }

    public CapturePhase Phase { get; private set; } = CapturePhase.CapturingHoldouts;

    public IReadOnlyList<Landmark> Landmarks => _landmarks.ToArray();

    public int ReferenceCount =>
        _landmarks.Count(row => row.EvidenceSet == LandmarkEvidenceSet.Reference);

    public int SealedHoldoutCount =>
        _landmarks.Count(row => row.EvidenceSet == LandmarkEvidenceSet.SealedHoldout);

    public bool IsComplete => Phase == CapturePhase.Complete;

    public void Capture(
        string id,
        double worldX,
        double worldY,
        double mapX,
        double mapY)
    {
        var evidenceSet = Phase switch
        {
            CapturePhase.CapturingHoldouts => LandmarkEvidenceSet.SealedHoldout,
            CapturePhase.CapturingReferences => LandmarkEvidenceSet.Reference,
            _ => throw new InvalidOperationException(
                "capture is locked until the current Gate B phase is completed"),
        };
        if (string.IsNullOrWhiteSpace(id))
        {
            throw new ArgumentException("landmark identifier is required", nameof(id));
        }
        if (new[] { worldX, worldY, mapX, mapY }.Any(value => !double.IsFinite(value)))
        {
            throw new ArgumentOutOfRangeException(
                nameof(worldX),
                "landmark coordinates must be finite");
        }
        if (mapX < 0 || mapX >= _mapWidth || mapY < 0 || mapY >= _mapHeight)
        {
            throw new ArgumentOutOfRangeException(
                nameof(mapX),
                "landmark pixel coordinates must be inside the loaded map");
        }
        if (_landmarks.Any(row => row.Id == id))
        {
            throw new ArgumentException("landmark identifier must be unique", nameof(id));
        }
        if (_landmarks.Any(row =>
                row.WorldX.ToBits() == worldX.ToBits()
                && row.WorldY.ToBits() == worldY.ToBits()))
        {
            throw new ArgumentException("landmark world coordinate pair must be unique", nameof(worldX));
        }
        if (_landmarks.Any(row =>
                row.MapX.ToBits() == mapX.ToBits()
                && row.MapY.ToBits() == mapY.ToBits()))
        {
            throw new ArgumentException("landmark map coordinate pair must be unique", nameof(mapX));
        }

        var zone = CoordinateSolver.DeriveZone(
            _mainMap.WorldMinX,
            _mainMap.WorldMinY,
            _mainMap.WorldMaxX,
            _mainMap.WorldMaxY,
            worldX,
            worldY);
        var quota = evidenceSet == LandmarkEvidenceSet.Reference ? 2 : 1;
        if (Count(zone, evidenceSet) >= quota)
        {
            throw new InvalidOperationException(
                $"{zone} already has the required {evidenceSet} observations");
        }
        _landmarks.Add(new Landmark(
            id,
            worldX,
            worldY,
            mapX,
            mapY,
            zone,
            evidenceSet,
            _gameBuildId));
        AdvanceAfterCapture();
    }

    internal void MarkHoldoutsExported()
    {
        if (Phase != CapturePhase.HoldoutsReadyForExport)
        {
            throw new InvalidOperationException("all five holdouts must be captured before export");
        }
        Phase = CapturePhase.AwaitingApprovedCommitment;
    }

    public void ApproveReferenceCapture(
        SealedHoldoutCommitment commitmentArtifact,
        AssetContract reviewedContract)
    {
        if (Phase != CapturePhase.AwaitingApprovedCommitment)
        {
            throw new InvalidOperationException(
                "holdouts must be frozen and exported before approval");
        }
        ArgumentNullException.ThrowIfNull(commitmentArtifact);
        ArgumentNullException.ThrowIfNull(reviewedContract);
        var expected = CreateCommitment();
        if (!SameCommitment(commitmentArtifact, expected))
        {
            throw new InvalidOperationException(
                "commitment artifact does not match the frozen holdouts");
        }
        var approvedMainMap = reviewedContract.MapRegions?.SingleOrDefault(region =>
            region.MapId == "MainMap" && region.RegionId == "FirstRegion");
        if (!reviewedContract.Reviewed
            || string.IsNullOrWhiteSpace(reviewedContract.ReviewId)
            || reviewedContract.GameBuildId != _gameBuildId
            || reviewedContract.ApprovedSealedHoldoutSha256 != expected.SealedHoldoutSha256
            || reviewedContract.ApprovedMappingSha256 != _candidateMappingSha256
            || reviewedContract.CandidateIdentitySha256() != _candidateIdentitySha256
            || approvedMainMap is null
            || approvedMainMap != _mainMap)
        {
            throw new InvalidOperationException(
                "reviewed exact-Build contract does not approve the frozen holdout commitment");
        }
        Phase = CapturePhase.CapturingReferences;
    }

    public SealedHoldoutCommitment CreateCommitment() =>
        CoordinateSolver.CreateSealedHoldoutCommitment(
            _mainMap.WorldMinX,
            _mainMap.WorldMinY,
            _mainMap.WorldMaxX,
            _mainMap.WorldMaxY,
            _landmarks
                .Where(row => row.EvidenceSet == LandmarkEvidenceSet.SealedHoldout)
                .ToArray(),
            _gameBuildId);

    public int Count(LandmarkZone zone, LandmarkEvidenceSet evidenceSet) =>
        _landmarks.Count(row => row.Zone == zone && row.EvidenceSet == evidenceSet);

    public bool Remove(string id)
    {
        var index = _landmarks.FindIndex(row => row.Id == id);
        if (index < 0)
        {
            return false;
        }
        var row = _landmarks[index];
        if (row.EvidenceSet == LandmarkEvidenceSet.SealedHoldout
            && Phase >= CapturePhase.AwaitingApprovedCommitment)
        {
            throw new InvalidOperationException("exported holdouts are frozen");
        }
        if (row.EvidenceSet == LandmarkEvidenceSet.Reference
            && Phase is not (CapturePhase.CapturingReferences or CapturePhase.Complete))
        {
            throw new InvalidOperationException("reference removal is unavailable in this phase");
        }
        _landmarks.RemoveAt(index);
        Phase = row.EvidenceSet switch
        {
            LandmarkEvidenceSet.SealedHoldout => CapturePhase.CapturingHoldouts,
            LandmarkEvidenceSet.Reference => CapturePhase.CapturingReferences,
            _ => Phase,
        };
        return true;
    }

    private void AdvanceAfterCapture()
    {
        if (Phase == CapturePhase.CapturingHoldouts
            && Enum.GetValues<LandmarkZone>().All(zone =>
                Count(zone, LandmarkEvidenceSet.SealedHoldout) == 1))
        {
            Phase = CapturePhase.HoldoutsReadyForExport;
        }
        else if (Phase == CapturePhase.CapturingReferences
            && Enum.GetValues<LandmarkZone>().All(zone =>
                Count(zone, LandmarkEvidenceSet.Reference) == 2))
        {
            Phase = CapturePhase.Complete;
        }
    }

    private static bool FiniteBounds(MapRegionContract region) =>
        double.IsFinite(region.WorldMinX)
        && double.IsFinite(region.WorldMinY)
        && double.IsFinite(region.WorldMaxX)
        && double.IsFinite(region.WorldMaxY)
        && region.WorldMinX < region.WorldMaxX
        && region.WorldMinY < region.WorldMaxY;

    private static bool SameCommitment(
        SealedHoldoutCommitment left,
        SealedHoldoutCommitment right) =>
        left.SchemaVersion == right.SchemaVersion
        && left.GameBuildId == right.GameBuildId
        && left.SealedHoldoutCount == right.SealedHoldoutCount
        && left.SealedHoldoutSha256 == right.SealedHoldoutSha256
        && Enum.GetValues<LandmarkZone>().All(zone =>
            left.ZoneCounts.TryGetValue(zone, out var leftCount)
            && right.ZoneCounts.TryGetValue(zone, out var rightCount)
            && leftCount == rightCount);
}

internal static class DoubleBits
{
    public static long ToBits(this double value) =>
        BitConverter.DoubleToInt64Bits(value == 0 ? 0 : value);
}
