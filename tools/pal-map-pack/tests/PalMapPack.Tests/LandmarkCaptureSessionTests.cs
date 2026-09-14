using System.Drawing;
using PalMapPack.Calibrator;

namespace PalMapPack.Tests;

public sealed class LandmarkCaptureSessionTests
{
    [Fact]
    public void CaptureStartsWithSealedHoldoutsAndRejectsInvalidCoordinates()
    {
        var session = NewSession();
        session.Capture("one", -1_000_000, 600_000, 3, 4);
        var landmark = Assert.Single(session.Landmarks);
        Assert.Equal(LandmarkEvidenceSet.SealedHoldout, landmark.EvidenceSet);
        Assert.Equal(LandmarkZone.SouthEast, landmark.Zone);
        Assert.Throws<ArgumentOutOfRangeException>(() =>
            session.Capture("bad", double.NaN, 2, 3, 4));
        Assert.Throws<MapPackFailure>(() =>
            session.Capture("outside", 400_000, 0, 5, 6));
        Assert.Throws<ArgumentOutOfRangeException>(() =>
            session.Capture("pixel-outside", -900_000, 500_000, 8_192, 4));
    }

    [Theory]
    [InlineData("")]
    [InlineData(" ")]
    [InlineData("not-a-build")]
    public void ConstructorRequiresExactNumericBuildIdentity(string buildId)
    {
        Assert.Throws<ArgumentException>(() =>
            new LandmarkCaptureSession(buildId, CandidateContract()));
    }

    [Fact]
    public void HoldoutPhaseEnforcesOneUniqueObservationPerZone()
    {
        var session = NewSession();
        session.Capture("center-holdout", -375_000, 0, 1, 1);
        Assert.Throws<InvalidOperationException>(() =>
            session.Capture("center-holdout-extra", -374_000, 1_000, 2, 2));
        Assert.Throws<ArgumentException>(() =>
            session.Capture("center-holdout", 100_000, -500_000, 3, 3));
        Assert.Throws<ArgumentException>(() =>
            session.Capture("duplicate-world", -375_000, 0, 4, 4));
        Assert.Throws<ArgumentException>(() =>
            session.Capture("duplicate-map", 100_000, -500_000, 1, 1));
    }

    [Fact]
    public void ReferenceCaptureRemainsLockedUntilHoldoutsAreFrozenAndApproved()
    {
        var session = CompleteHoldouts();
        Assert.Equal(CapturePhase.HoldoutsReadyForExport, session.Phase);
        Assert.Throws<InvalidOperationException>(() =>
            session.Capture("early-reference", -374_000, 1_000, 10, 10));

        ExportHoldouts(session);
        Assert.Equal(CapturePhase.AwaitingApprovedCommitment, session.Phase);
        Assert.Throws<InvalidOperationException>(() =>
            session.Remove("center-holdout"));

        var commitment = session.CreateCommitment();
        var candidate = ApprovedContract(session, reviewed: false);
        Assert.Throws<InvalidOperationException>(() =>
            session.ApproveReferenceCapture(commitment, candidate));
        Assert.Equal(CapturePhase.AwaitingApprovedCommitment, session.Phase);

        session.ApproveReferenceCapture(
            commitment,
            ApprovedContract(session, reviewed: true));
        Assert.Equal(CapturePhase.CapturingReferences, session.Phase);
    }

    [Fact]
    public void ApprovalRejectsWrongCommitmentBuildHashAndBounds()
    {
        var session = CompleteHoldouts();
        ExportHoldouts(session);
        var commitment = session.CreateCommitment();
        var approved = ApprovedContract(session, reviewed: true);

        Assert.Throws<InvalidOperationException>(() => session.ApproveReferenceCapture(
            commitment with { GameBuildId = "1" },
            approved));
        Assert.Throws<InvalidOperationException>(() => session.ApproveReferenceCapture(
            commitment,
            approved with { ApprovedSealedHoldoutSha256 = new string('a', 64) }));
        Assert.Throws<InvalidOperationException>(() => session.ApproveReferenceCapture(
            commitment,
            approved with { ApprovedMappingSha256 = new string('b', 64) }));
        Assert.Throws<InvalidOperationException>(() => session.ApproveReferenceCapture(
            commitment,
            approved with
            {
                MapRegions = approved.MapRegions.Select(region =>
                    region.MapId == "MainMap"
                        ? region with { WorldMaxX = region.WorldMaxX + 1 }
                        : region).ToArray(),
            }));
        Assert.Throws<InvalidOperationException>(() => session.ApproveReferenceCapture(
            commitment,
            approved with
            {
                PoiSources = approved.PoiSources with
                {
                    ExpectedWorldExportCount = approved.PoiSources.ExpectedWorldExportCount + 1,
                },
            }));
    }

    [Fact]
    public void TwoReferencesPerZoneCompletesTheSession()
    {
        var session = ApprovedSession();
        CaptureReferences(session);

        Assert.True(session.IsComplete);
        Assert.Equal(10, session.ReferenceCount);
        Assert.Equal(5, session.SealedHoldoutCount);
        foreach (var zone in Enum.GetValues<LandmarkZone>())
        {
            Assert.Equal(2, session.Count(zone, LandmarkEvidenceSet.Reference));
            Assert.Equal(1, session.Count(zone, LandmarkEvidenceSet.SealedHoldout));
        }

        Assert.True(session.Remove("center-reference-1"));
        Assert.Equal(CapturePhase.CapturingReferences, session.Phase);
        Assert.False(session.IsComplete);
        Assert.False(session.Remove("missing"));
    }

    [Fact]
    public void ExporterWritesHoldoutThenFinalPipelineDocumentsWithoutOverwriting()
    {
        var session = CompleteHoldouts();
        var root = Path.Combine(Path.GetTempPath(), $"pal-calibrator-{Guid.NewGuid():N}");
        Directory.CreateDirectory(root);
        try
        {
            var holdouts = Path.Combine(root, "sealed-holdouts.json");
            LandmarkCaptureExporter.WriteHoldoutsNew(holdouts, session);
            Assert.Equal(CapturePhase.AwaitingApprovedCommitment, session.Phase);
            using (var holdoutJson = System.Text.Json.JsonDocument.Parse(
                File.ReadAllBytes(holdouts)))
            {
                Assert.Equal(5, holdoutJson.RootElement.GetArrayLength());
                Assert.All(holdoutJson.RootElement.EnumerateArray(), row =>
                {
                    Assert.Equal("sealed_holdout", row.GetProperty("evidence_set").GetString());
                    Assert.Equal(Fixture.Build, row.GetProperty("game_build_id").GetString());
                    Assert.False(row.TryGetProperty("zone", out _));
                    Assert.Equal(7, row.EnumerateObject().Count());
                });
            }
            Assert.Throws<InvalidOperationException>(() =>
                LandmarkCaptureExporter.WriteHoldoutsNew(holdouts, session));

            session.ApproveReferenceCapture(
                session.CreateCommitment(),
                ApprovedContract(session, reviewed: true));
            CaptureReferences(session);
            var final = Path.Combine(root, "landmarks.json");
            LandmarkCaptureExporter.WriteFinalNew(final, session);
            using var finalJson = System.Text.Json.JsonDocument.Parse(File.ReadAllBytes(final));
            Assert.Equal(15, finalJson.RootElement.GetArrayLength());
            Assert.Equal(10, finalJson.RootElement.EnumerateArray().Count(row =>
                row.GetProperty("evidence_set").GetString() == "reference"));
            Assert.Throws<IOException>(() =>
                LandmarkCaptureExporter.WriteFinalNew(final, session));
        }
        finally
        {
            Directory.Delete(root, recursive: true);
        }
    }

    [Fact]
    public void FailedHoldoutExportDoesNotAdvanceTheCapturePhase()
    {
        var session = CompleteHoldouts();
        var root = Path.Combine(Path.GetTempPath(), $"pal-calibrator-{Guid.NewGuid():N}");
        Directory.CreateDirectory(root);
        try
        {
            var destination = Path.Combine(root, "sealed-holdouts.json");
            File.WriteAllText(destination, "do not replace");

            Assert.Throws<IOException>(() =>
                LandmarkCaptureExporter.WriteHoldoutsNew(destination, session));
            Assert.Equal(CapturePhase.HoldoutsReadyForExport, session.Phase);
            Assert.Equal("do not replace", File.ReadAllText(destination));
        }
        finally
        {
            Directory.Delete(root, recursive: true);
        }
    }

    [Fact]
    public void HoldoutExportRejectsAReparseParentWithoutAdvancing()
    {
        var session = CompleteHoldouts();
        var root = Path.Combine(Path.GetTempPath(), $"pal-calibrator-{Guid.NewGuid():N}");
        var outside = Path.Combine(Path.GetTempPath(), $"pal-calibrator-outside-{Guid.NewGuid():N}");
        Directory.CreateDirectory(root);
        Directory.CreateDirectory(outside);
        try
        {
            var linkedParent = Path.Combine(root, "linked");
            Directory.CreateSymbolicLink(linkedParent, outside);

            Assert.Throws<MapPackFailure>(() =>
                LandmarkCaptureExporter.WriteHoldoutsNew(
                    Path.Combine(linkedParent, "sealed-holdouts.json"),
                    session));
            Assert.Equal(CapturePhase.HoldoutsReadyForExport, session.Phase);
            Assert.False(File.Exists(Path.Combine(outside, "sealed-holdouts.json")));
        }
        finally
        {
            Directory.Delete(root, recursive: true);
            Directory.Delete(outside, recursive: true);
        }
    }

    [Fact]
    public void ZoomMapperMatchesPictureBoxIntegerLetterboxingAndExclusiveEdges()
    {
        Assert.True(ZoomedImageCoordinateMapper.TryMap(
            new Size(1_200, 900),
            new Size(2_048, 2_048),
            new Size(8_192, 8_192),
            new Point(150, 0),
            out var topLeft));
        Assert.Equal(0, topLeft.X, 6);
        Assert.Equal(0, topLeft.Y, 6);

        Assert.True(ZoomedImageCoordinateMapper.TryMap(
            new Size(1_200, 900),
            new Size(2_048, 2_048),
            new Size(8_192, 8_192),
            new Point(600, 450),
            out var center));
        Assert.Equal(4_096, center.X, 3);
        Assert.Equal(4_096, center.Y, 3);

        Assert.False(ZoomedImageCoordinateMapper.TryMap(
            new Size(1_200, 900),
            new Size(2_048, 2_048),
            new Size(8_192, 8_192),
            new Point(149, 450),
            out _));
        Assert.False(ZoomedImageCoordinateMapper.TryMap(
            new Size(1_200, 900),
            new Size(2_048, 2_048),
            new Size(8_192, 8_192),
            new Point(1_050, 450),
            out _));
        Assert.False(ZoomedImageCoordinateMapper.TryMap(
            new Size(1_200, 900),
            new Size(2_048, 2_048),
            new Size(8_192, 8_192),
            new Point(600, 900),
            out _));
    }

    [Fact]
    public void PreviewAspectRatioMustMatchTheAuthoritativeCoordinateSpace()
    {
        Assert.True(ZoomedImageCoordinateMapper.HasSameAspectRatio(
            new Size(2_048, 2_048),
            new Size(8_192, 8_192)));
        Assert.True(ZoomedImageCoordinateMapper.HasSameAspectRatio(
            new Size(1_024, 512),
            new Size(8_192, 4_096)));
        Assert.False(ZoomedImageCoordinateMapper.HasSameAspectRatio(
            new Size(2_048, 1_024),
            new Size(8_192, 8_192)));
        Assert.False(ZoomedImageCoordinateMapper.HasSameAspectRatio(
            Size.Empty,
            new Size(8_192, 8_192)));
    }

    private static LandmarkCaptureSession NewSession() =>
        new(Fixture.Build, CandidateContract());

    private static AssetContract CandidateContract() =>
        Fixture.ApprovedContract() with
        {
            Reviewed = false,
            ReviewId = string.Empty,
            ApprovedMappingSha256 = null,
            ApprovedSealedHoldoutSha256 = null,
        };

    private static LandmarkCaptureSession CompleteHoldouts()
    {
        var session = NewSession();
        foreach (var (zone, worldX, worldY) in Samples())
        {
            session.Capture(
                $"{zone.ToString().ToLowerInvariant()}-holdout",
                worldX,
                worldY,
                100 + (int)zone * 10,
                100 + (int)zone * 10);
        }
        return session;
    }

    private static LandmarkCaptureSession ApprovedSession()
    {
        var session = CompleteHoldouts();
        ExportHoldouts(session);
        session.ApproveReferenceCapture(
            session.CreateCommitment(),
            ApprovedContract(session, reviewed: true));
        return session;
    }

    private static void ExportHoldouts(LandmarkCaptureSession session)
    {
        var root = Path.Combine(Path.GetTempPath(), $"pal-calibrator-{Guid.NewGuid():N}");
        Directory.CreateDirectory(root);
        try
        {
            LandmarkCaptureExporter.WriteHoldoutsNew(
                Path.Combine(root, "sealed-holdouts.json"),
                session);
        }
        finally
        {
            Directory.Delete(root, recursive: true);
        }
    }

    private static AssetContract ApprovedContract(
        LandmarkCaptureSession session,
        bool reviewed)
    {
        var contract = Fixture.ApprovedContract();
        return contract with
        {
            GameBuildId = Fixture.Build,
            Reviewed = reviewed,
            ReviewId = reviewed ? "independent-review" : string.Empty,
            ApprovedSealedHoldoutSha256 = session.CreateCommitment().SealedHoldoutSha256,
        };
    }

    private static void CaptureReferences(LandmarkCaptureSession session)
    {
        foreach (var (zone, worldX, worldY) in Samples())
        {
            session.Capture(
                $"{zone.ToString().ToLowerInvariant()}-reference-1",
                worldX + 1_000,
                worldY + 1_000,
                101 + (int)zone * 10,
                102 + (int)zone * 10);
            session.Capture(
                $"{zone.ToString().ToLowerInvariant()}-reference-2",
                worldX + 2_000,
                worldY + 2_000,
                103 + (int)zone * 10,
                104 + (int)zone * 10);
        }
    }

    private static (LandmarkZone Zone, double WorldX, double WorldY)[] Samples() =>
    [
        (LandmarkZone.Center, -375_000d, 0d),
        (LandmarkZone.NorthWest, 100_000d, -500_000d),
        (LandmarkZone.NorthEast, 100_000d, 500_000d),
        (LandmarkZone.SouthWest, -900_000d, -500_000d),
        (LandmarkZone.SouthEast, -900_000d, 500_000d),
    ];
}
