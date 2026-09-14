using PalMapPack;
using System.Globalization;
using System.Text;

namespace PalMapPack.Tests;

public sealed class CoordinateSolverTests
{
    [Fact]
    public void BoundsTransformIsDeterministicAndLandmarksOnlyValidateIt()
    {
        var region = Fixture.Assets().MainMap;
        var expected = CoordinateSolver.DeriveAuthoritativeBoundsTransform(region);

        var result = CoordinateSolver.ValidateAuthoritativeBounds(
            region,
            Fixture.ExactLandmarks(),
            Fixture.Build);

        Assert.True(result.Accepted, result.RejectionReason);
        Assert.Equal(expected, result.Matrix);
        Assert.Equal(10u, result.ReferenceCount);
        Assert.Equal(5u, result.SealedHoldoutCount);
        Assert.All(result.Residuals, residual => Assert.True(residual.ErrorPixels < 0.000001));
        Assert.Matches("^[0-9a-f]{64}$", result.SealedHoldoutSha256);
    }

    [Fact]
    public void MovingALandmarkCannotChangeTheAuthoritativeMatrix()
    {
        var region = Fixture.Assets().MainMap;
        var landmarks = Fixture.ExactLandmarks().ToArray();
        landmarks[0] = landmarks[0] with { MapX = landmarks[0].MapX + 50.0 };

        var result = CoordinateSolver.ValidateAuthoritativeBounds(region, landmarks, Fixture.Build);

        Assert.False(result.Accepted);
        Assert.Equal(CoordinateSolver.DeriveAuthoritativeBoundsTransform(region), result.Matrix);
    }

    [Fact]
    public void ReferenceAffineFitRecoversARealTransformAndValidatesIndependentHoldouts()
    {
        double[][] expected =
        [
            [0.000_073, 0.000_409, 318.25],
            [-0.000_351, 0.000_061, 147.75],
        ];
        var observations = ProjectWith(expected, Fixture.ExactLandmarks());

        var result = CoordinateSolver.FitAndValidateReferenceAffine(
            Fixture.Assets().MainMap,
            observations,
            Fixture.Build);

        Assert.True(result.Accepted, result.RejectionReason);
        AssertMatrixClose(expected, result.Matrix, 0.000_000_001);
        Assert.InRange(result.ReferenceMaxErrorPixels, 0, 0.000_001);
        Assert.InRange(result.SealedHoldoutMaxErrorPixels, 0, 0.000_001);
    }

    [Fact]
    public void SealedHoldoutCannotInfluenceReferenceAffineFit()
    {
        double[][] expected =
        [
            [0.000_073, 0.000_409, 318.25],
            [-0.000_351, 0.000_061, 147.75],
        ];
        var observations = ProjectWith(expected, Fixture.ExactLandmarks());
        var baseline = CoordinateSolver.FitAndValidateReferenceAffine(
            Fixture.Assets().MainMap,
            observations,
            Fixture.Build);
        var changed = observations.ToArray();
        var holdoutIndex = Array.FindIndex(
            changed,
            row => row.EvidenceSet == LandmarkEvidenceSet.SealedHoldout);
        changed[holdoutIndex] = changed[holdoutIndex] with
        {
            MapX = changed[holdoutIndex].MapX + 30.0,
        };

        var result = CoordinateSolver.FitAndValidateReferenceAffine(
            Fixture.Assets().MainMap,
            changed,
            Fixture.Build);

        Assert.False(result.Accepted);
        Assert.Equal(baseline.Matrix, result.Matrix);
        Assert.InRange(result.ReferenceMaxErrorPixels, 0, 0.000_001);
        Assert.InRange(result.SealedHoldoutMaxErrorPixels, 29.999_999, 30.000_001);
    }

    [Fact]
    public void CorruptReferenceFitIsRejectedBySealedHoldouts()
    {
        double[][] expected =
        [
            [0.000_073, 0.000_409, 318.25],
            [-0.000_351, 0.000_061, 147.75],
        ];
        var observations = ProjectWith(expected, Fixture.ExactLandmarks());
        var baseline = CoordinateSolver.FitAndValidateReferenceAffine(
            Fixture.Assets().MainMap,
            observations,
            Fixture.Build);
        var changed = observations.ToArray();
        var referenceIndex = Array.FindIndex(
            changed,
            row => row.EvidenceSet == LandmarkEvidenceSet.Reference);
        changed[referenceIndex] = changed[referenceIndex] with
        {
            MapX = changed[referenceIndex].MapX + 300.0,
        };

        var result = CoordinateSolver.FitAndValidateReferenceAffine(
            Fixture.Assets().MainMap,
            changed,
            Fixture.Build);

        Assert.False(result.Accepted);
        Assert.NotEqual(baseline.Matrix, result.Matrix);
        Assert.True(
            result.SealedHoldoutMaxErrorPixels > 25.0,
            $"expected a sealed holdout failure, got {result.SealedHoldoutMaxErrorPixels:R}px");
    }

    [Fact]
    public void ValidationRequiresExactlyFifteenSamplesAndTheExactPerZoneSplit()
    {
        var region = Fixture.Assets().MainMap;
        var fourteen = Fixture.ExactLandmarks().Take(14).ToArray();
        Assert.Contains(
            "15",
            CoordinateSolver.ValidateAuthoritativeBounds(region, fourteen, Fixture.Build).RejectionReason,
            StringComparison.Ordinal);

        var extraWorldX = -370_000.0;
        var extraWorldY = 5_000.0;
        var extraMap = CoordinateSolver.Project(
            CoordinateSolver.DeriveAuthoritativeBoundsTransform(region),
            extraWorldX,
            extraWorldY);
        var sixteen = Fixture.ExactLandmarks()
            .Append(new Landmark(
                "extra-reference",
                extraWorldX,
                extraWorldY,
                extraMap.X,
                extraMap.Y,
                CoordinateSolver.DeriveZone(extraWorldX, extraWorldY),
                LandmarkEvidenceSet.Reference,
                Fixture.Build))
            .ToArray();
        Assert.Contains(
            "exactly 15",
            CoordinateSolver.ValidateAuthoritativeBounds(region, sixteen, Fixture.Build).RejectionReason,
            StringComparison.Ordinal);

        var unsealedNorthWest = Fixture.ExactLandmarks()
            .Select(row => row.Zone == LandmarkZone.NorthWest
                ? row with { EvidenceSet = LandmarkEvidenceSet.Reference }
                : row)
            .ToArray();
        var result = CoordinateSolver.ValidateAuthoritativeBounds(
            region,
            unsealedNorthWest,
            Fixture.Build);
        Assert.False(result.Accepted);
        Assert.Contains("exactly", result.RejectionReason, StringComparison.OrdinalIgnoreCase);

        var missingNorthWestReference = Fixture.ExactLandmarks().ToArray();
        var northWestReferenceIndex = Array.FindIndex(
            missingNorthWestReference,
            row => row.Zone == LandmarkZone.NorthWest
                && row.EvidenceSet == LandmarkEvidenceSet.Reference);
        missingNorthWestReference[northWestReferenceIndex] =
            missingNorthWestReference[northWestReferenceIndex] with
            {
                EvidenceSet = LandmarkEvidenceSet.SealedHoldout,
            };
        result = CoordinateSolver.ValidateAuthoritativeBounds(
            region,
            missingNorthWestReference,
            Fixture.Build);
        Assert.False(result.Accepted);
        Assert.Contains("two reference", result.RejectionReason, StringComparison.OrdinalIgnoreCase);
    }

    [Fact]
    public void ValidationRejectsSamplesFromAnotherBuild()
    {
        var result = CoordinateSolver.ValidateAuthoritativeBounds(
            Fixture.Assets().MainMap,
            Fixture.ExactLandmarks().Select(row => row with { GameBuildId = "old-build" }).ToArray(),
            Fixture.Build);

        Assert.False(result.Accepted);
        Assert.Contains("Build", result.RejectionReason, StringComparison.Ordinal);
    }

    [Fact]
    public void ZoneLabelsAreDerivedAndCannotOverrideSpatialCoverage()
    {
        var misleadingLabels = Fixture.ExactLandmarks()
            .Select(row => row with { Zone = LandmarkZone.Center })
            .ToArray();

        var result = CoordinateSolver.ValidateAuthoritativeBounds(
            Fixture.Assets().MainMap,
            misleadingLabels,
            Fixture.Build);

        Assert.True(result.Accepted, result.RejectionReason);
        Assert.Equal(3, result.ZoneCounts[LandmarkZone.NorthWest]);
    }

    [Fact]
    public void ZoneNamesFollowRenderedMapAxes()
    {
        Assert.Equal(LandmarkZone.Center, CoordinateSolver.DeriveZone(-375_000, 0));
        Assert.Equal(LandmarkZone.NorthWest, CoordinateSolver.DeriveZone(100_000, -400_000));
        Assert.Equal(LandmarkZone.NorthEast, CoordinateSolver.DeriveZone(100_000, 400_000));
        Assert.Equal(LandmarkZone.SouthWest, CoordinateSolver.DeriveZone(-800_000, -400_000));
        Assert.Equal(LandmarkZone.SouthEast, CoordinateSolver.DeriveZone(-800_000, 400_000));
    }

    [Theory]
    [InlineData(10.00, 25.00, true)]
    [InlineData(10.01, 25.00, false)]
    [InlineData(10.00, 25.01, false)]
    public void ValidationThresholdBoundariesAreExact(double median, double maximum, bool accepted)
    {
        Assert.Equal(accepted, CoordinateSolver.WithinThresholds(median, maximum));
    }

    [Fact]
    public void ParityFixtureCoversTwentyFiveGridPoints()
    {
        var matrix = CoordinateSolver.DeriveAuthoritativeBoundsTransform(Fixture.Assets().MainMap);
        var parity = CoordinateSolver.CreateParityPoints(matrix, 640, 520);
        Assert.Equal(25, parity.Count);
        Assert.Equal(25, parity.Select(point => (point.ExpectedMapX, point.ExpectedMapY)).Distinct().Count());
    }

    [Fact]
    public void HoldoutHashChangesWhenSealedEvidenceChangesButNotWhenReferenceEvidenceChanges()
    {
        var region = Fixture.Assets().MainMap;
        var baseline = CoordinateSolver.ValidateAuthoritativeBounds(region, Fixture.ExactLandmarks(), Fixture.Build);

        var referencesChanged = Fixture.ExactLandmarks().ToArray();
        referencesChanged[0] = referencesChanged[0] with { Id = "reference-renamed" };
        var referenceResult = CoordinateSolver.ValidateAuthoritativeBounds(region, referencesChanged, Fixture.Build);

        var holdoutChanged = Fixture.ExactLandmarks().ToArray();
        holdoutChanged[2] = holdoutChanged[2] with { Id = "holdout-renamed" };
        var holdoutResult = CoordinateSolver.ValidateAuthoritativeBounds(region, holdoutChanged, Fixture.Build);

        Assert.Equal(baseline.SealedHoldoutSha256, referenceResult.SealedHoldoutSha256);
        Assert.NotEqual(baseline.SealedHoldoutSha256, holdoutResult.SealedHoldoutSha256);
    }

    [Fact]
    public void HoldoutHashFramingPreventsEmbeddedDelimiterSetSubstitution()
    {
        var region = Fixture.Assets().MainMap;
        var original = Fixture.ExactLandmarks().ToArray();
        var first = original.Single(row => row.Id == "landmark-2");
        var second = original.Single(row => row.Id == "landmark-5");
        var replacementWorldX = -360_000.0;
        var replacementWorldY = 10_000.0;
        var matrix = CoordinateSolver.DeriveAuthoritativeBoundsTransform(region);
        var replacementMap = CoordinateSolver.Project(
            matrix,
            replacementWorldX,
            replacementWorldY);
        var replacement = new Landmark(
            string.Empty,
            replacementWorldX,
            replacementWorldY,
            replacementMap.X,
            replacementMap.Y,
            CoordinateSolver.DeriveZone(replacementWorldX, replacementWorldY),
            LandmarkEvidenceSet.SealedHoldout,
            Fixture.Build);
        var firstEncoding = OldDelimitedSuffix(first);
        var replacementEncoding = OldDelimitedSuffix(replacement);

        var delimiterInjected = original.Select(row => row.Id switch
            {
                "landmark-2" => row with { Id = "a" },
                "landmark-5" => row with
                {
                    Id = $"p\0{replacementEncoding}\n{Fixture.Build}\0r",
                },
                "landmark-8" or "landmark-11" or "landmark-14" => row with
                {
                    Id = $"z-{row.Id}",
                },
                _ => row,
            })
            .ToArray();
        var structurallyDifferent = original.Select(row => row.Id switch
            {
                "landmark-2" => replacement with
                {
                    Id = $"a\0{firstEncoding}\n{Fixture.Build}\0p",
                },
                "landmark-5" => row with { Id = "r" },
                "landmark-8" or "landmark-11" or "landmark-14" => row with
                {
                    Id = $"z-{row.Id}",
                },
                _ => row,
            })
            .ToArray();

        Assert.Equal(
            OldDelimitedHoldoutEncoding(delimiterInjected),
            OldDelimitedHoldoutEncoding(structurallyDifferent));

        var injectedResult = CoordinateSolver.ValidateAuthoritativeBounds(
            region,
            delimiterInjected,
            Fixture.Build);
        var differentResult = CoordinateSolver.ValidateAuthoritativeBounds(
            region,
            structurallyDifferent,
            Fixture.Build);
        Assert.True(injectedResult.Accepted, injectedResult.RejectionReason);
        Assert.True(differentResult.Accepted, differentResult.RejectionReason);
        Assert.NotEqual(injectedResult.SealedHoldoutSha256, differentResult.SealedHoldoutSha256);
    }

    [Fact]
    public void HoldoutHashRejectsInvalidUnicodeInsteadOfUsingReplacementBytes()
    {
        var landmarks = Fixture.ExactLandmarks().ToArray();
        landmarks[2] = landmarks[2] with { Id = "invalid-\ud800" };

        var result = CoordinateSolver.ValidateAuthoritativeBounds(
            Fixture.Assets().MainMap,
            landmarks,
            Fixture.Build);

        Assert.False(result.Accepted);
        Assert.Contains("identifiers", result.RejectionReason, StringComparison.Ordinal);
    }

    private static string OldDelimitedHoldoutEncoding(IReadOnlyList<Landmark> landmarks)
    {
        var canonical = new StringBuilder();
        foreach (var row in landmarks
            .Where(row => row.EvidenceSet == LandmarkEvidenceSet.SealedHoldout)
            .OrderBy(row => row.Id, StringComparer.Ordinal))
        {
            canonical.Append(row.GameBuildId).Append('\0')
                .Append(row.Id).Append('\0')
                .Append(CoordinateSolver.DeriveZone(row.WorldX, row.WorldY)).Append('\0')
                .Append(row.WorldX.ToString("R", CultureInfo.InvariantCulture)).Append('\0')
                .Append(row.WorldY.ToString("R", CultureInfo.InvariantCulture)).Append('\0')
                .Append(row.MapX.ToString("R", CultureInfo.InvariantCulture)).Append('\0')
                .Append(row.MapY.ToString("R", CultureInfo.InvariantCulture)).Append('\n');
        }
        return canonical.ToString();
    }

    private static string OldDelimitedSuffix(Landmark row) => string.Join(
        '\0',
        CoordinateSolver.DeriveZone(row.WorldX, row.WorldY).ToString(),
        row.WorldX.ToString("R", CultureInfo.InvariantCulture),
        row.WorldY.ToString("R", CultureInfo.InvariantCulture),
        row.MapX.ToString("R", CultureInfo.InvariantCulture),
        row.MapY.ToString("R", CultureInfo.InvariantCulture));

    private static IReadOnlyList<Landmark> ProjectWith(
        double[][] matrix,
        IReadOnlyList<Landmark> landmarks) =>
        landmarks.Select(row =>
        {
            var projected = CoordinateSolver.Project(matrix, row.WorldX, row.WorldY);
            return row with { MapX = projected.X, MapY = projected.Y };
        }).ToArray();

    private static void AssertMatrixClose(
        double[][] expected,
        double[][] actual,
        double tolerance)
    {
        Assert.Equal(2, actual.Length);
        for (var row = 0; row < 2; row++)
        {
            Assert.Equal(3, actual[row].Length);
            for (var column = 0; column < 3; column++)
            {
                Assert.InRange(
                    Math.Abs(actual[row][column] - expected[row][column]),
                    0,
                    tolerance);
            }
        }
    }
}
