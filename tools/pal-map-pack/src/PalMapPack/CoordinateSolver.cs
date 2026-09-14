using System.Buffers.Binary;
using System.Security.Cryptography;
using System.Text;

namespace PalMapPack;

public enum LandmarkZone
{
    Center = 0,
    NorthWest = 1,
    NorthEast = 2,
    SouthWest = 3,
    SouthEast = 4,
}

public enum LandmarkEvidenceSet
{
    Reference = 0,
    SealedHoldout = 1,
}

public sealed record Landmark(
    string Id,
    double WorldX,
    double WorldY,
    double MapX,
    double MapY,
    LandmarkZone Zone,
    LandmarkEvidenceSet EvidenceSet,
    string GameBuildId);

public sealed record CoordinateValidationResidual(
    string LandmarkId,
    LandmarkEvidenceSet EvidenceSet,
    LandmarkZone Zone,
    double ErrorPixels);

public sealed record ParityPoint(
    double WorldX,
    double WorldY,
    double ExpectedMapX,
    double ExpectedMapY);

public sealed record SealedHoldoutCommitment(
    uint SchemaVersion,
    string GameBuildId,
    uint SealedHoldoutCount,
    string SealedHoldoutSha256,
    IReadOnlyDictionary<LandmarkZone, uint> ZoneCounts);

public sealed record CoordinateValidationResult(
    bool Accepted,
    string RejectionReason,
    double[][] Matrix,
    uint ReferenceCount,
    uint SealedHoldoutCount,
    double ReferenceMedianErrorPixels,
    double ReferenceMaxErrorPixels,
    double SealedHoldoutMedianErrorPixels,
    double SealedHoldoutMaxErrorPixels,
    string SealedHoldoutSha256,
    IReadOnlyList<CoordinateValidationResidual> Residuals,
    IReadOnlyDictionary<LandmarkZone, int> ZoneCounts,
    IReadOnlyDictionary<LandmarkZone, int> ReferenceZoneCounts,
    IReadOnlyDictionary<LandmarkZone, int> SealedHoldoutZoneCounts);

/// <summary>
/// Defines deterministic world-to-map transforms and validates independent observations.
///
/// The legacy bounds transform remains available for compatibility. Production Gate B uses only
/// the ten reference observations to fit an affine transform, then evaluates the five
/// precommitted sealed holdouts without allowing them to influence the fit.
/// </summary>
public static class CoordinateSolver
{
    private const double RelativeRankTolerance = 1e-12;
    private const double MainMapMinX = -1_099_400.0;
    private const double MainMapMinY = -724_400.0;
    private const double MainMapMaxX = 349_400.0;
    private const double MainMapMaxY = 724_400.0;
    private static readonly UTF8Encoding StrictUtf8 = new(false, true);

    public static CoordinateValidationResult ValidateAuthoritativeBounds(
        MapRegionAsset region,
        IReadOnlyList<Landmark> landmarks,
        string gameBuildId)
        => Validate(region, landmarks, gameBuildId, fitReferences: false);

    public static CoordinateValidationResult FitAndValidateReferenceAffine(
        MapRegionAsset region,
        IReadOnlyList<Landmark> landmarks,
        string gameBuildId)
        => Validate(region, landmarks, gameBuildId, fitReferences: true);

    private static CoordinateValidationResult Validate(
        MapRegionAsset region,
        IReadOnlyList<Landmark> landmarks,
        string gameBuildId,
        bool fitReferences)
    {
        ArgumentNullException.ThrowIfNull(region);
        ArgumentNullException.ThrowIfNull(landmarks);
        var matrix = DeriveAuthoritativeBoundsTransform(region);
        var emptyCounts = EmptyZoneCounts();
        if (string.IsNullOrEmpty(gameBuildId)
            || !StrictUtf8Encodable(gameBuildId)
            || landmarks.Any(row => string.IsNullOrWhiteSpace(row.Id)
                || !StrictUtf8Encodable(row.Id)
                || !StrictUtf8Encodable(row.GameBuildId)
                || !string.Equals(row.GameBuildId, gameBuildId, StringComparison.Ordinal)
                || !Finite(row.WorldX, row.WorldY, row.MapX, row.MapY)))
        {
            return Rejected(matrix, emptyCounts, emptyCounts, emptyCounts,
                "landmark identifiers, coordinates, and exact Build identity must be valid");
        }

        LandmarkZone Zone(Landmark row) => DeriveZone(region, row.WorldX, row.WorldY);
        IReadOnlyDictionary<LandmarkZone, int> Counts(LandmarkEvidenceSet? set) =>
            Enum.GetValues<LandmarkZone>().ToDictionary(
                zone => zone,
                zone => landmarks.Count(row => Zone(row) == zone
                    && (set is null || row.EvidenceSet == set.Value)));

        IReadOnlyDictionary<LandmarkZone, int> zones;
        IReadOnlyDictionary<LandmarkZone, int> references;
        IReadOnlyDictionary<LandmarkZone, int> holdouts;
        try
        {
            zones = Counts(null);
            references = Counts(LandmarkEvidenceSet.Reference);
            holdouts = Counts(LandmarkEvidenceSet.SealedHoldout);
        }
        catch (MapPackFailure)
        {
            return Rejected(matrix, emptyCounts, emptyCounts, emptyCounts,
                "landmarks must be inside the authoritative region bounds");
        }

        if (landmarks.Count != 15)
        {
            return Rejected(matrix, zones, references, holdouts,
                "exactly 15 current-Build landmarks are required");
        }
        if (references.Any(pair => pair.Value != 2))
        {
            return Rejected(matrix, zones, references, holdouts,
                "exactly two reference landmarks are required in every authoritative spatial zone");
        }
        if (holdouts.Any(pair => pair.Value != 1))
        {
            return Rejected(matrix, zones, references, holdouts,
                "exactly one sealed holdout landmark is required in every authoritative spatial zone");
        }
        if (landmarks.Select(row => row.Id).Distinct(StringComparer.Ordinal).Count() != landmarks.Count
            || landmarks.Select(row => (Bits(row.WorldX), Bits(row.WorldY))).Distinct().Count() != landmarks.Count
            || landmarks.Select(row => (Bits(row.MapX), Bits(row.MapY))).Distinct().Count() != landmarks.Count)
        {
            return Rejected(matrix, zones, references, holdouts,
                "landmark identifiers and observed coordinate pairs must be unique");
        }

        if (fitReferences)
        {
            try
            {
                matrix = FitReferenceAffine(landmarks);
            }
            catch (MapPackFailure)
            {
                return Rejected(matrix, zones, references, holdouts,
                    "reference landmarks do not define a stable affine transform");
            }
        }

        var residuals = landmarks.Select(row =>
        {
            var derivedZone = Zone(row);
            var projected = Project(matrix, row.WorldX, row.WorldY);
            return new CoordinateValidationResidual(
                row.Id,
                row.EvidenceSet,
                derivedZone,
                Hypot(projected.X - row.MapX, projected.Y - row.MapY));
        }).ToArray();
        var referenceErrors = residuals
            .Where(row => row.EvidenceSet == LandmarkEvidenceSet.Reference)
            .Select(row => row.ErrorPixels)
            .ToArray();
        var holdoutErrors = residuals
            .Where(row => row.EvidenceSet == LandmarkEvidenceSet.SealedHoldout)
            .Select(row => row.ErrorPixels)
            .ToArray();
        var referenceMedian = Median(referenceErrors);
        var referenceMaximum = referenceErrors.Max();
        var holdoutMedian = Median(holdoutErrors);
        var holdoutMaximum = holdoutErrors.Max();
        var accepted = WithinThresholds(referenceMedian, referenceMaximum)
            && WithinThresholds(holdoutMedian, holdoutMaximum);
        return new CoordinateValidationResult(
            accepted,
            accepted
                ? string.Empty
                : "reference or sealed holdout residual exceeds Gate B tolerance",
            matrix,
            checked((uint)referenceErrors.Length),
            checked((uint)holdoutErrors.Length),
            referenceMedian,
            referenceMaximum,
            holdoutMedian,
            holdoutMaximum,
            ComputeSealedHoldoutSha256(
                region.WorldMinX,
                region.WorldMinY,
                region.WorldMaxX,
                region.WorldMaxY,
                landmarks),
            residuals,
            zones,
            references,
            holdouts);
    }

    public static SealedHoldoutCommitment CreateSealedHoldoutCommitment(
        double worldMinX,
        double worldMinY,
        double worldMaxX,
        double worldMaxY,
        IReadOnlyList<Landmark> sealedHoldouts,
        string gameBuildId)
    {
        ArgumentNullException.ThrowIfNull(sealedHoldouts);
        if (!Finite(worldMinX, worldMinY, worldMaxX, worldMaxY)
            || worldMinX >= worldMaxX
            || worldMinY >= worldMaxY
            || string.IsNullOrEmpty(gameBuildId)
            || !StrictUtf8Encodable(gameBuildId)
            || sealedHoldouts.Count != 5
            || sealedHoldouts.Any(row =>
                row.EvidenceSet != LandmarkEvidenceSet.SealedHoldout
                || string.IsNullOrWhiteSpace(row.Id)
                || !StrictUtf8Encodable(row.Id)
                || !StrictUtf8Encodable(row.GameBuildId)
                || !string.Equals(row.GameBuildId, gameBuildId, StringComparison.Ordinal)
                || !Finite(row.WorldX, row.WorldY, row.MapX, row.MapY)))
        {
            throw new MapPackFailure(
                ExitCodes.CalibrationRejected,
                "holdout commitment requires exactly five valid sealed exact-Build observations");
        }

        LandmarkZone Zone(Landmark row) => DeriveZone(
            worldMinX,
            worldMinY,
            worldMaxX,
            worldMaxY,
            row.WorldX,
            row.WorldY);
        LandmarkZone[] zones;
        try
        {
            zones = sealedHoldouts.Select(Zone).ToArray();
        }
        catch (MapPackFailure)
        {
            throw new MapPackFailure(
                ExitCodes.CalibrationRejected,
                "sealed holdout observations must be inside the authoritative MainMap bounds");
        }
        var counts = Enum.GetValues<LandmarkZone>()
            .ToDictionary(
                zone => zone,
                zone => checked((uint)zones.Count(candidate => candidate == zone)));
        if (counts.Any(pair => pair.Value != 1)
            || sealedHoldouts.Select(row => row.Id).Distinct(StringComparer.Ordinal).Count()
                != sealedHoldouts.Count
            || sealedHoldouts.Select(row => (Bits(row.WorldX), Bits(row.WorldY))).Distinct().Count()
                != sealedHoldouts.Count
            || sealedHoldouts.Select(row => (Bits(row.MapX), Bits(row.MapY))).Distinct().Count()
                != sealedHoldouts.Count)
        {
            throw new MapPackFailure(
                ExitCodes.CalibrationRejected,
                "sealed holdout commitment requires one unique observation in every authoritative spatial zone");
        }
        return new SealedHoldoutCommitment(
            1,
            gameBuildId,
            5,
            ComputeSealedHoldoutSha256(
                worldMinX,
                worldMinY,
                worldMaxX,
                worldMaxY,
                sealedHoldouts),
            counts);
    }

    public static double[][] DeriveAuthoritativeBoundsTransform(MapRegionAsset region)
    {
        ArgumentNullException.ThrowIfNull(region);
        var spanX = region.WorldMaxX - region.WorldMinX;
        var spanY = region.WorldMaxY - region.WorldMinY;
        if (!double.IsFinite(spanX)
            || !double.IsFinite(spanY)
            || spanX <= 0
            || spanY <= 0
            || region.Map.Width <= 0
            || region.Map.Height <= 0)
        {
            throw new MapPackFailure(
                ExitCodes.PackIntegrity,
                "authoritative world bounds cannot define a coordinate transform");
        }
        var xScale = region.Map.Width / spanY;
        var yScale = region.Map.Height / spanX;
        return
        [
            [0.0, xScale, -region.WorldMinY * xScale],
            [-yScale, 0.0, region.WorldMaxX * yScale],
        ];
    }

    public static double[][] FitReferenceAffine(IReadOnlyList<Landmark> landmarks)
    {
        ArgumentNullException.ThrowIfNull(landmarks);
        var references = landmarks
            .Where(row => row.EvidenceSet == LandmarkEvidenceSet.Reference)
            .ToArray();
        if (references.Length != 10
            || references.Any(row => !Finite(row.WorldX, row.WorldY, row.MapX, row.MapY)))
        {
            throw new MapPackFailure(
                ExitCodes.CalibrationRejected,
                "affine fitting requires exactly ten finite reference observations");
        }

        var meanX = references.Average(row => row.WorldX);
        var meanY = references.Average(row => row.WorldY);
        var scaleX = references.Max(row => Math.Abs(row.WorldX - meanX));
        var scaleY = references.Max(row => Math.Abs(row.WorldY - meanY));
        if (!Finite(meanX, meanY, scaleX, scaleY) || scaleX <= 0 || scaleY <= 0)
        {
            throw new MapPackFailure(
                ExitCodes.CalibrationRejected,
                "reference observations do not span both world axes");
        }

        var normalized = references
            .Select(row => (
                U: (row.WorldX - meanX) / scaleX,
                V: (row.WorldY - meanY) / scaleY,
                row.MapX,
                row.MapY))
            .ToArray();
        var mapX = SolveLeastSquares3(
            normalized.Select(row => (row.U, row.V, row.MapX)).ToArray());
        var mapY = SolveLeastSquares3(
            normalized.Select(row => (row.U, row.V, row.MapY)).ToArray());

        double[] Denormalize(double[] fitted)
        {
            var fromX = fitted[0] / scaleX;
            var fromY = fitted[1] / scaleY;
            var offset = fitted[2] - fromX * meanX - fromY * meanY;
            if (!Finite(fromX, fromY, offset))
            {
                throw new MapPackFailure(
                    ExitCodes.CalibrationRejected,
                    "affine fit produced a non-finite coefficient");
            }
            return [fromX, fromY, offset];
        }

        var matrix = new[] { Denormalize(mapX), Denormalize(mapY) };
        ValidateMatrix(matrix);
        var linearScale = matrix
            .SelectMany(row => row.Take(2))
            .Max(value => Math.Abs(value));
        var normalizedDeterminant =
            matrix[0][0] / linearScale * (matrix[1][1] / linearScale)
            - matrix[0][1] / linearScale * (matrix[1][0] / linearScale);
        if (linearScale == 0
            || !double.IsFinite(normalizedDeterminant)
            || Math.Abs(normalizedDeterminant) <= RelativeRankTolerance)
        {
            throw new MapPackFailure(
                ExitCodes.CalibrationRejected,
                "reference observations define a singular affine transform");
        }
        return matrix;
    }

    public static LandmarkZone DeriveZone(double worldX, double worldY) =>
        DeriveZone(
            MainMapMinX,
            MainMapMinY,
            MainMapMaxX,
            MainMapMaxY,
            worldX,
            worldY);

    public static LandmarkZone DeriveZone(MapRegionAsset region, double worldX, double worldY)
    {
        ArgumentNullException.ThrowIfNull(region);
        return DeriveZone(
            region.WorldMinX,
            region.WorldMinY,
            region.WorldMaxX,
            region.WorldMaxY,
            worldX,
            worldY);
    }

    public static bool WithinThresholds(double medianErrorPixels, double maxErrorPixels) =>
        double.IsFinite(medianErrorPixels)
        && double.IsFinite(maxErrorPixels)
        && medianErrorPixels >= 0
        && maxErrorPixels >= medianErrorPixels
        && medianErrorPixels <= 10.0
        && maxErrorPixels <= 25.0;

    public static IReadOnlyList<ParityPoint> CreateParityPoints(
        double[][] matrix,
        int mapWidth,
        int mapHeight)
    {
        ValidateMatrix(matrix);
        if (mapWidth <= 0 || mapHeight <= 0)
        {
            throw new ArgumentOutOfRangeException(nameof(mapWidth));
        }

        var a = matrix[0][0];
        var b = matrix[0][1];
        var c = matrix[0][2];
        var d = matrix[1][0];
        var e = matrix[1][1];
        var f = matrix[1][2];
        var determinant = a * e - b * d;
        if (!double.IsFinite(determinant) || Math.Abs(determinant) <= RelativeRankTolerance)
        {
            throw new MapPackFailure(ExitCodes.CalibrationRejected, "transform cannot be inverted for parity fixture");
        }

        var result = new List<ParityPoint>(25);
        for (var gridY = 0; gridY < 5; gridY++)
        {
            for (var gridX = 0; gridX < 5; gridX++)
            {
                var mapX = mapWidth * gridX / 4.0;
                var mapY = mapHeight * gridY / 4.0;
                var translatedX = mapX - c;
                var translatedY = mapY - f;
                var worldX = (e * translatedX - b * translatedY) / determinant;
                var worldY = (-d * translatedX + a * translatedY) / determinant;
                result.Add(new ParityPoint(worldX, worldY, mapX, mapY));
            }
        }
        return result;
    }

    public static (double X, double Y) Project(double[][] matrix, double worldX, double worldY)
    {
        ValidateMatrix(matrix);
        var x = matrix[0][0] * worldX + matrix[0][1] * worldY + matrix[0][2];
        var y = matrix[1][0] * worldX + matrix[1][1] * worldY + matrix[1][2];
        if (!Finite(x, y))
        {
            throw new MapPackFailure(ExitCodes.CalibrationRejected, "coordinate projection is non-finite");
        }
        return (x, y);
    }

    public static LandmarkZone DeriveZone(
        double minX,
        double minY,
        double maxX,
        double maxY,
        double worldX,
        double worldY)
    {
        if (!Finite(worldX, worldY)
            || worldX < minX
            || worldX > maxX
            || worldY < minY
            || worldY > maxY)
        {
            throw new MapPackFailure(
                ExitCodes.CalibrationRejected,
                "landmark is outside the authoritative region bounds");
        }
        var midpointX = (minX + maxX) / 2.0;
        var midpointY = (minY + maxY) / 2.0;
        var centerHalfWidth = (maxX - minX) / 6.0;
        var centerHalfHeight = (maxY - minY) / 6.0;
        if (Math.Abs(worldX - midpointX) <= centerHalfWidth
            && Math.Abs(worldY - midpointY) <= centerHalfHeight)
        {
            return LandmarkZone.Center;
        }
        return (worldX < midpointX, worldY >= midpointY) switch
        {
            (true, true) => LandmarkZone.SouthEast,
            (false, true) => LandmarkZone.NorthEast,
            (true, false) => LandmarkZone.SouthWest,
            (false, false) => LandmarkZone.NorthWest,
        };
    }

    private static string ComputeSealedHoldoutSha256(
        double worldMinX,
        double worldMinY,
        double worldMaxX,
        double worldMaxY,
        IReadOnlyList<Landmark> landmarks)
    {
        var holdouts = landmarks
            .Where(row => row.EvidenceSet == LandmarkEvidenceSet.SealedHoldout)
            .OrderBy(row => row.Id, StringComparer.Ordinal)
            .ToArray();
        using var hash = IncrementalHash.CreateHash(HashAlgorithmName.SHA256);
        AppendLengthPrefixedUtf8(hash, "PalMapPack.SealedHoldout");
        AppendUInt32(hash, 1);
        AppendUInt32(hash, checked((uint)holdouts.Length));
        foreach (var row in holdouts)
        {
            AppendLengthPrefixedUtf8(hash, row.GameBuildId);
            AppendLengthPrefixedUtf8(hash, row.Id);
            AppendUInt32(hash, checked((uint)row.EvidenceSet));
            AppendUInt32(hash, checked((uint)DeriveZone(
                worldMinX,
                worldMinY,
                worldMaxX,
                worldMaxY,
                row.WorldX,
                row.WorldY)));
            AppendUInt64(hash, Bits(row.WorldX));
            AppendUInt64(hash, Bits(row.WorldY));
            AppendUInt64(hash, Bits(row.MapX));
            AppendUInt64(hash, Bits(row.MapY));
        }
        return Convert.ToHexString(hash.GetHashAndReset()).ToLowerInvariant();
    }

    private static void AppendLengthPrefixedUtf8(IncrementalHash hash, string value)
    {
        var bytes = StrictUtf8.GetBytes(value);
        AppendUInt32(hash, checked((uint)bytes.Length));
        hash.AppendData(bytes);
    }

    private static void AppendUInt32(IncrementalHash hash, uint value)
    {
        Span<byte> bytes = stackalloc byte[sizeof(uint)];
        BinaryPrimitives.WriteUInt32LittleEndian(bytes, value);
        hash.AppendData(bytes);
    }

    private static void AppendUInt64(IncrementalHash hash, ulong value)
    {
        Span<byte> bytes = stackalloc byte[sizeof(ulong)];
        BinaryPrimitives.WriteUInt64LittleEndian(bytes, value);
        hash.AppendData(bytes);
    }

    private static double Median(IReadOnlyList<double> values)
    {
        var ordered = values.Order().ToArray();
        var middle = ordered.Length / 2;
        return ordered.Length % 2 == 0
            ? (ordered[middle - 1] + ordered[middle]) / 2.0
            : ordered[middle];
    }

    private static double[] SolveLeastSquares3(
        IReadOnlyList<(double U, double V, double Target)> rows)
    {
        var system = new double[3, 4];
        foreach (var row in rows)
        {
            var basis = new[] { row.U, row.V, 1.0 };
            for (var r = 0; r < 3; r++)
            {
                for (var c = 0; c < 3; c++)
                {
                    system[r, c] += basis[r] * basis[c];
                }
                system[r, 3] += basis[r] * row.Target;
            }
        }

        for (var pivot = 0; pivot < 3; pivot++)
        {
            var best = pivot;
            for (var candidate = pivot + 1; candidate < 3; candidate++)
            {
                if (Math.Abs(system[candidate, pivot]) > Math.Abs(system[best, pivot]))
                {
                    best = candidate;
                }
            }
            var pivotScale = Enumerable
                .Range(pivot, 3 - pivot)
                .Select(column => Math.Abs(system[best, column]))
                .DefaultIfEmpty(0)
                .Max();
            if (!double.IsFinite(pivotScale)
                || pivotScale == 0
                || Math.Abs(system[best, pivot]) <= RelativeRankTolerance * pivotScale)
            {
                throw new MapPackFailure(
                    ExitCodes.CalibrationRejected,
                    "reference least-squares system is rank deficient");
            }
            if (best != pivot)
            {
                for (var column = pivot; column < 4; column++)
                {
                    (system[pivot, column], system[best, column]) =
                        (system[best, column], system[pivot, column]);
                }
            }

            var divisor = system[pivot, pivot];
            for (var column = pivot; column < 4; column++)
            {
                system[pivot, column] /= divisor;
            }
            for (var row = 0; row < 3; row++)
            {
                if (row == pivot)
                {
                    continue;
                }
                var factor = system[row, pivot];
                for (var column = pivot; column < 4; column++)
                {
                    system[row, column] -= factor * system[pivot, column];
                }
            }
        }

        var result = new[] { system[0, 3], system[1, 3], system[2, 3] };
        if (result.Any(value => !double.IsFinite(value)))
        {
            throw new MapPackFailure(
                ExitCodes.CalibrationRejected,
                "reference least-squares solution is non-finite");
        }
        return result;
    }

    private static double Hypot(double x, double y) => Math.Sqrt(x * x + y * y);

    private static void ValidateMatrix(double[][] matrix)
    {
        if (matrix.Length != 2
            || matrix.Any(row => row.Length != 3)
            || matrix.SelectMany(row => row).Any(value => !double.IsFinite(value)))
        {
            throw new MapPackFailure(ExitCodes.CalibrationRejected, "matrix must contain six finite coefficients");
        }
    }

    private static ulong Bits(double value) => value == 0.0 ? 0 : BitConverter.DoubleToUInt64Bits(value);

    private static bool Finite(params double[] values) => values.All(double.IsFinite);

    private static bool StrictUtf8Encodable(string value)
    {
        try
        {
            _ = StrictUtf8.GetByteCount(value);
            return true;
        }
        catch (EncoderFallbackException)
        {
            return false;
        }
    }

    private static IReadOnlyDictionary<LandmarkZone, int> EmptyZoneCounts() =>
        Enum.GetValues<LandmarkZone>().ToDictionary(zone => zone, _ => 0);

    private static CoordinateValidationResult Rejected(
        double[][] matrix,
        IReadOnlyDictionary<LandmarkZone, int> zones,
        IReadOnlyDictionary<LandmarkZone, int> references,
        IReadOnlyDictionary<LandmarkZone, int> holdouts,
        string reason) =>
        new(
            false,
            reason,
            matrix,
            checked((uint)references.Values.Sum()),
            checked((uint)holdouts.Values.Sum()),
            double.PositiveInfinity,
            double.PositiveInfinity,
            double.PositiveInfinity,
            double.PositiveInfinity,
            new string('0', 64),
            Array.Empty<CoordinateValidationResidual>(),
            zones,
            references,
            holdouts);
}
