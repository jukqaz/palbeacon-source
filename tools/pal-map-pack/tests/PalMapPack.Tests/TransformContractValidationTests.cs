using System.Text.Json;
using System.Text.Json.Serialization;
using PalMapPack;

namespace PalMapPack.Tests;

public sealed class TransformContractValidationTests
{
    private static readonly JsonSerializerOptions JsonOptions = new()
    {
        PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower,
        UnmappedMemberHandling = JsonUnmappedMemberHandling.Disallow,
    };

    [Fact]
    public void BuilderRequiresTheReviewedContractToPrecommitTheSealedHoldout()
    {
        var request = BuildRequest() with
        {
            Contract = Fixture.ApprovedContract() with
            {
                ApprovedSealedHoldoutSha256 = new string('9', 64),
            },
        };

        var failure = Assert.Throws<MapPackFailure>(() =>
            MapPackBuilder.BuildStaging(request));

        Assert.Equal(ExitCodes.CalibrationRejected, failure.ExitCode);
        Assert.Contains("commitment", failure.Message, StringComparison.OrdinalIgnoreCase);
    }

    [Fact]
    public void ValidatorRejectsCalibrationZoneCountsThatDoNotSumToLandmarkCount()
    {
        var pack = BuildPack();
        var transform = ReadMainTransform(pack);

        ReplaceMainTransform(pack, transform with
        {
            Calibration = transform.Calibration! with
            {
                LandmarkCount = transform.Calibration.LandmarkCount + 1,
            },
        });

        AssertInvalid(pack);
    }

    [Fact]
    public void ValidatorRequiresIndependentlyAccountedSealedHoldoutsInAllFiveZones()
    {
        var pack = BuildPack();
        var transform = ReadMainTransform(pack);

        ReplaceMainTransform(pack, transform with
        {
            Calibration = transform.Calibration! with
            {
                SealedHoldoutNorthWestCount = 0,
            },
        });

        AssertInvalid(pack);
    }

    [Fact]
    public void ValidatorRejectsMissingHoldoutAttestationOrHoldoutResidualFailure()
    {
        var pack = BuildPack();
        var transform = ReadMainTransform(pack);
        ReplaceMainTransform(pack, transform with
        {
            Calibration = transform.Calibration! with
            {
                SealedHoldoutSha256 = new string('0', 64),
            },
        });
        AssertInvalid(pack);

        pack = BuildPack();
        transform = ReadMainTransform(pack);
        ReplaceMainTransform(pack, transform with
        {
            Calibration = transform.Calibration! with
            {
                SealedHoldoutMaxErrorPx = 25.01,
            },
        });
        AssertInvalid(pack);
    }

    [Fact]
    public void ValidatorRejectsParityExpectedCoordinatesOutsideMapBounds()
    {
        var pack = BuildPack();
        var transform = ReadMainTransform(pack);
        var parity = transform.Calibration!.ParityPoints.ToArray();
        parity[0] = PointForExpectedMap(transform.Matrix, -0.005, parity[0].ExpectedMapY);

        ReplaceMainTransform(pack, transform with
        {
            Calibration = transform.Calibration with { ParityPoints = parity },
        });

        AssertInvalid(pack);
    }

    [Fact]
    public void ValidatorRejectsDuplicateParityWorldCoordinates()
    {
        var pack = BuildPack();
        var transform = ReadMainTransform(pack);
        var parity = transform.Calibration!.ParityPoints.ToArray();
        parity[1] = new ParityPoint(
            parity[0].WorldX,
            parity[0].WorldY,
            parity[0].ExpectedMapX + 0.0000000005,
            parity[0].ExpectedMapY);

        ReplaceMainTransform(pack, transform with
        {
            Calibration = transform.Calibration with { ParityPoints = parity },
        });

        AssertInvalid(pack);
    }

    [Fact]
    public void ValidatorRejectsDuplicateParityExpectedCoordinates()
    {
        var pack = BuildPack();
        var transform = ReadMainTransform(pack);
        var parity = transform.Calibration!.ParityPoints.ToArray();
        var nearbyWorld = PointForExpectedMap(
            transform.Matrix,
            parity[0].ExpectedMapX + 0.0000000005,
            parity[0].ExpectedMapY);
        parity[1] = nearbyWorld with
        {
            ExpectedMapX = parity[0].ExpectedMapX,
            ExpectedMapY = parity[0].ExpectedMapY,
        };

        ReplaceMainTransform(pack, transform with
        {
            Calibration = transform.Calibration with { ParityPoints = parity },
        });

        AssertInvalid(pack);
    }

    [Fact]
    public void ValidatorRequiresEveryExactFiveByFiveParityGridSlot()
    {
        var pack = BuildPack();
        var transform = ReadMainTransform(pack);
        var parity = transform.Calibration!.ParityPoints.ToArray();
        parity[6] = PointForExpectedMap(
            transform.Matrix,
            parity[6].ExpectedMapX + 1.0,
            parity[6].ExpectedMapY);

        ReplaceMainTransform(pack, transform with
        {
            Calibration = transform.Calibration with { ParityPoints = parity },
        });

        AssertInvalid(pack);
    }

    private static MapPackBuildResult BuildPack() =>
        MapPackBuilder.BuildStaging(BuildRequest());

    private static MapPackBuildRequest BuildRequest() =>
        new(
            Fixture.TempDirectory(),
            Fixture.Build,
            "fixture.usmap",
            "mapping"u8.ToArray(),
            "contract"u8.ToArray(),
            Fixture.ApprovedContract(),
            new[] { Fixture.Container() },
            Fixture.ContainerPaths(),
            Fixture.ExactLandmarks(),
            new FixtureAssetReader(Fixture.Assets()),
            DateTimeOffset.UnixEpoch,
            "fixture",
            "abc",
            Fixture.Inventory());

    private static TransformDocument ReadMainTransform(MapPackBuildResult pack)
    {
        var main = Assert.Single(
            pack.Manifest.MapRegions,
            region => region.MapId == "MainMap");
        return JsonSerializer.Deserialize<TransformDocument>(
            File.ReadAllBytes(ComponentPath(pack, main.TransformRelativePath)),
            JsonOptions)!;
    }

    private static void ReplaceMainTransform(
        MapPackBuildResult pack,
        TransformDocument transform)
    {
        var bytes = ManifestWriter.SerializeBytes(transform);
        var main = Assert.Single(
            pack.Manifest.MapRegions,
            region => region.MapId == "MainMap");
        File.WriteAllBytes(ComponentPath(pack, main.TransformRelativePath), bytes);
        var updatedManifest = pack.Manifest with
        {
            MapRegions = pack.Manifest.MapRegions.Select(region =>
                region.MapId == "MainMap"
                    ? region with { TransformSha256 = Hashing.Sha256Hex(bytes) }
                    : region).ToArray(),
        };
        File.WriteAllBytes(
            Path.Combine(pack.StagingPath, "manifest.json"),
            ManifestWriter.SerializeBytes(updatedManifest));
    }

    private static ParityPoint PointForExpectedMap(
        double[][] matrix,
        double expectedMapX,
        double expectedMapY)
    {
        var a = matrix[0][0];
        var b = matrix[0][1];
        var c = matrix[0][2];
        var d = matrix[1][0];
        var e = matrix[1][1];
        var f = matrix[1][2];
        var determinant = a * e - b * d;
        var translatedX = expectedMapX - c;
        var translatedY = expectedMapY - f;
        return new ParityPoint(
            (e * translatedX - b * translatedY) / determinant,
            (-d * translatedX + a * translatedY) / determinant,
            expectedMapX,
            expectedMapY);
    }

    private static string ComponentPath(MapPackBuildResult pack, string relativePath) =>
        Path.Combine(
            pack.StagingPath,
            relativePath.Replace('/', Path.DirectorySeparatorChar));

    private static void AssertInvalid(MapPackBuildResult pack) =>
        Assert.False(MapPackValidator.IsValid(
            pack.StagingPath,
            Fixture.Build,
            Hashing.Sha256Hex("contract"u8),
            Hashing.Sha256Hex("mapping"u8)));
}
