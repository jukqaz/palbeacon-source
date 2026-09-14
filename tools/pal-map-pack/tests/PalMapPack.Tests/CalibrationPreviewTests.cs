using PalMapPack;

namespace PalMapPack.Tests;

public sealed class CalibrationPreviewTests
{
    [Fact]
    public void ExactMapProducesOneDeterministicContentBoundBmpPreview()
    {
        var source = SourceMap();
        var sourceHash = Hashing.MapAssetSha256(source);
        var root = Fixture.TempDirectory();
        try
        {
            var firstPath = Path.Combine(root, "first.bmp");
            var secondPath = Path.Combine(root, "second.bmp");
            var first = CalibrationPreviewBuilder.WriteDeterministicBmp(
                firstPath,
                source,
                sourceHash,
                width: 2,
                height: 2);
            var second = CalibrationPreviewBuilder.WriteDeterministicBmp(
                secondPath,
                source,
                sourceHash,
                width: 2,
                height: 2);

            Assert.Equal(first, second);
            Assert.Equal(File.ReadAllBytes(firstPath), File.ReadAllBytes(secondPath));
            Assert.Equal((2, 2), (first.WidthPx, first.HeightPx));
            Assert.Equal(70, first.FileSizeBytes);
            Assert.Equal(sourceHash, first.SourceMapAssetSha256);
            Assert.Equal(CalibrationPreviewBuilder.Algorithm, first.Algorithm);
            Assert.Equal(File.ReadAllBytes(firstPath),
                CalibrationPreviewBuilder.VerifyExactFile(firstPath, first, sourceHash));
        }
        finally
        {
            Directory.Delete(root, recursive: true);
        }
    }

    [Fact]
    public void PureArtifactAndDimensionSelectionRespectBoundPreviewIdentity()
    {
        var source = SourceMap();
        var sourceHash = Hashing.MapAssetSha256(source);
        var artifact = CalibrationPreviewBuilder.BuildDeterministicBmp(
            source,
            sourceHash,
            width: 2,
            height: 2);

        Assert.Equal(artifact.Contract.Sha256, Hashing.Sha256Hex(artifact.Bytes));
        Assert.Equal((8, 8), CalibrationPreviewBuilder.SelectDimensions(source, null));
        Assert.Equal((2, 2), CalibrationPreviewBuilder.SelectDimensions(source, artifact.Contract));
    }

    [Fact]
    public void ArtifactWriterRejectsDescriptorMutationBeforeCreatingAFile()
    {
        var source = SourceMap();
        var sourceHash = Hashing.MapAssetSha256(source);
        var artifact = CalibrationPreviewBuilder.BuildDeterministicBmp(source, sourceHash, 2, 2);
        var mutated = artifact with
        {
            Contract = artifact.Contract with { Sha256 = Hashing.Sha256Hex("mutated"u8) },
        };
        var root = Fixture.TempDirectory();
        var path = Path.Combine(root, "preview.bmp");
        try
        {
            Assert.Throws<MapPackFailure>(() =>
                CalibrationPreviewBuilder.WriteDeterministicBmp(path, mutated));
            Assert.False(File.Exists(path));
        }
        finally
        {
            Directory.Delete(root, recursive: true);
        }
    }

    [Fact]
    public void PreviewVerificationRejectsMutationAndWrongSourceIdentity()
    {
        var source = SourceMap();
        var sourceHash = Hashing.MapAssetSha256(source);
        var root = Fixture.TempDirectory();
        try
        {
            var path = Path.Combine(root, "preview.bmp");
            var preview = CalibrationPreviewBuilder.WriteDeterministicBmp(
                path,
                source,
                sourceHash,
                width: 2,
                height: 2);

            Assert.Throws<MapPackFailure>(() =>
                CalibrationPreviewBuilder.VerifyExactFile(
                    path,
                    preview,
                    Hashing.Sha256Hex("wrong-map"u8)));
            var bytes = File.ReadAllBytes(path);
            bytes[^1] ^= 1;
            File.WriteAllBytes(path, bytes);
            Assert.Throws<MapPackFailure>(() =>
                CalibrationPreviewBuilder.VerifyExactFile(path, preview, sourceHash));
        }
        finally
        {
            Directory.Delete(root, recursive: true);
        }
    }

    [Fact]
    public void PreviewVerificationRejectsAHashBoundNoncanonicalBmpHeader()
    {
        var source = SourceMap();
        var sourceHash = Hashing.MapAssetSha256(source);
        var root = Fixture.TempDirectory();
        try
        {
            var path = Path.Combine(root, "preview.bmp");
            var preview = CalibrationPreviewBuilder.WriteDeterministicBmp(
                path,
                source,
                sourceHash,
                width: 2,
                height: 2);
            var bytes = File.ReadAllBytes(path);
            bytes[6] = 1;
            File.WriteAllBytes(path, bytes);
            var rebound = preview with { Sha256 = Hashing.Sha256Hex(bytes) };

            Assert.Throws<MapPackFailure>(() =>
                CalibrationPreviewBuilder.VerifyExactFile(path, rebound, sourceHash));
        }
        finally
        {
            Directory.Delete(root, recursive: true);
        }
    }

    [Fact]
    public void CandidateRejectsPreviewDimensionsWithALyingSmallFileSize()
    {
        var approved = Fixture.ApprovedContract();
        var main = approved.MapRegions.Single(region => region.MapId == "MainMap");
        var fakePreview = main.CalibrationPreview! with
        {
            WidthPx = main.MapWidthPx,
            HeightPx = main.MapHeightPx,
            FileSizeBytes = 70,
        };
        var candidate = approved with
        {
            Reviewed = false,
            ReviewId = string.Empty,
            ApprovedMappingSha256 = null,
            ApprovedSealedHoldoutSha256 = null,
            MapRegions = approved.MapRegions.Select(region =>
                region == main
                    ? region with { CalibrationPreview = fakePreview }
                    : region).ToArray(),
        };

        Assert.Throws<MapPackFailure>(() =>
            candidate.RequireCalibrationCandidate(Fixture.Build));
    }

    [Fact]
    public void PreviewBuilderRejectsNonuniformAndUnboundInputs()
    {
        var source = SourceMap();
        var sourceHash = Hashing.MapAssetSha256(source);
        var root = Fixture.TempDirectory();
        try
        {
            Assert.Throws<MapPackFailure>(() =>
                CalibrationPreviewBuilder.WriteDeterministicBmp(
                    Path.Combine(root, "wrong-hash.bmp"),
                    source,
                    Hashing.Sha256Hex("wrong"u8),
                    2,
                    2));
            Assert.Throws<MapPackFailure>(() =>
                CalibrationPreviewBuilder.WriteDeterministicBmp(
                    Path.Combine(root, "nonuniform.bmp"),
                    source,
                    sourceHash,
                    2,
                    4));
        }
        finally
        {
            Directory.Delete(root, recursive: true);
        }
    }

    [Fact]
    public void PreviewWriterNeverReplacesDifferentExistingBytes()
    {
        var source = SourceMap();
        var sourceHash = Hashing.MapAssetSha256(source);
        var root = Fixture.TempDirectory();
        try
        {
            var path = Path.Combine(root, "preview.bmp");
            File.WriteAllBytes(path, "existing-private-preview"u8.ToArray());

            Assert.Throws<MapPackFailure>(() =>
                CalibrationPreviewBuilder.WriteDeterministicBmp(
                    path,
                    source,
                    sourceHash,
                    width: 2,
                    height: 2));
            Assert.Equal(
                "existing-private-preview"u8.ToArray(),
                File.ReadAllBytes(path));
        }
        finally
        {
            Directory.Delete(root, recursive: true);
        }
    }

    [Fact]
    public void PreviewWriterRejectsAHashBoundNoncanonicalArtifact()
    {
        var source = SourceMap();
        var sourceHash = Hashing.MapAssetSha256(source);
        var artifact = CalibrationPreviewBuilder.BuildDeterministicBmp(
            source,
            sourceHash,
            width: 2,
            height: 2);
        artifact.Bytes[6] = 1;
        var rebound = artifact with
        {
            Contract = artifact.Contract with
            {
                Sha256 = Hashing.Sha256Hex(artifact.Bytes),
            },
        };
        var root = Fixture.TempDirectory();
        try
        {
            var path = Path.Combine(root, "preview.bmp");
            Assert.Throws<MapPackFailure>(() =>
                CalibrationPreviewBuilder.WriteDeterministicBmp(path, rebound));
            Assert.False(File.Exists(path));
        }
        finally
        {
            Directory.Delete(root, recursive: true);
        }
    }

    private static RgbaMap SourceMap()
    {
        var rgba = new byte[8 * 8 * 4];
        for (var y = 0; y < 8; y++)
        {
            for (var x = 0; x < 8; x++)
            {
                var offset = (y * 8 + x) * 4;
                rgba[offset] = (byte)(x * 16);
                rgba[offset + 1] = (byte)(y * 16);
                rgba[offset + 2] = (byte)((x + y) * 8);
                rgba[offset + 3] = 255;
            }
        }
        return new RgbaMap(8, 8, rgba);
    }
}
