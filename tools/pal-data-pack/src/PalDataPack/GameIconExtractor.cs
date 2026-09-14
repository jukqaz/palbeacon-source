using System.Text;
using System.Text.Json;
using System.Text.Json.Serialization;
using CUE4Parse.UE4.Assets.Exports.Engine;
using CUE4Parse.UE4.Assets.Exports.Texture;
using CUE4Parse_Conversion.Textures;
using SixLabors.ImageSharp;
using SixLabors.ImageSharp.Formats.Png;
using SixLabors.ImageSharp.PixelFormats;
using SixLabors.ImageSharp.Processing;

namespace PalDataPack;

public sealed record GameIconAlphaBounds(
    int X,
    int Y,
    int Width,
    int Height);

public sealed record ExtractedGameIcon(
    string GroupId,
    string LogicalId,
    string PackagePath,
    string SourceRelativePath,
    string ThumbnailRelativePath,
    int SourceWidth,
    int SourceHeight,
    string SourcePixelFormat,
    string SourceDecodedRgbaSha256,
    string SourcePngSha256,
    string ThumbnailPngSha256,
    GameIconAlphaBounds AlphaBounds);

public sealed record GameIconPackageManifest(
    int SchemaMajor,
    string GameBuildId,
    string MappingSha256,
    string ReviewId,
    string DistributionScope,
    string SourceKind,
    bool OriginalGameAssetsOnly,
    int ThumbnailSizePx,
    int ThumbnailContentSizePx,
    string ThumbnailTransformation,
    IReadOnlyList<ExtractedGameIcon> Icons);

public sealed record GameIconExtractionResult(
    string OutputDirectory,
    string ManifestPath,
    int IconCount,
    long TotalPngBytes);

public static class GameIconExtractor
{
    private static readonly JsonSerializerOptions JsonOptions = new()
    {
        PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower,
        WriteIndented = true,
        UnmappedMemberHandling = JsonUnmappedMemberHandling.Disallow,
        MaxDepth = 64,
    };

    private static readonly PngEncoder PngEncoder = new()
    {
        ColorType = PngColorType.RgbWithAlpha,
        CompressionLevel = PngCompressionLevel.Level9,
    };

    public static GameIconExtractionResult Extract(
        SteamInstall install,
        string mappingPath,
        AssetContract sourceContract,
        GameIconContract iconContract,
        string outputDirectory)
    {
        var probe = GameIconProbe.Probe(
            install,
            mappingPath,
            sourceContract);
        iconContract.RequireProbe(probe);
        var fullOutput = Path.GetFullPath(outputDirectory);
        if (Directory.Exists(fullOutput) || File.Exists(fullOutput))
        {
            throw Failure("game icon output must not already exist");
        }
        var parent = Path.GetDirectoryName(fullOutput)
            ?? throw Failure("game icon output parent is invalid");
        Directory.CreateDirectory(parent);
        SecureInput.EnsurePlainDirectory(parent);
        var staging = Path.Combine(
            parent,
            $".{Path.GetFileName(fullOutput)}.{Guid.NewGuid():N}.staging");
        Directory.CreateDirectory(staging);

        try
        {
            var provider = SourceContractProbe.OpenProvider(
                install.InstallPath,
                mappingPath);
            var icons = new List<ExtractedGameIcon>();
            long totalPngBytes = 0;
            foreach (var group in probe.Groups)
            {
                var definition = GameIconProbe.Definitions.Single(value =>
                    string.Equals(
                        value.GroupId,
                        group.GroupId,
                        StringComparison.Ordinal));
                var sourceDirectory = Path.Combine(
                    staging,
                    "source",
                    group.GroupId);
                var thumbnailDirectory = Path.Combine(
                    staging,
                    "thumbnails",
                    group.GroupId);
                Directory.CreateDirectory(sourceDirectory);
                Directory.CreateDirectory(thumbnailDirectory);

                foreach (var packagePath in group.PackagePaths)
                {
                    var packageKey = Hashing.Sha256Hex(
                        Encoding.UTF8.GetBytes(packagePath))[..20];
                    var sourceRelativePath =
                        $"source/{group.GroupId}/{packageKey}.png";
                    var thumbnailRelativePath =
                        $"thumbnails/{group.GroupId}/{packageKey}.png";
                    var sourceDestination = Path.Combine(
                        staging,
                        sourceRelativePath.Replace(
                            '/',
                            Path.DirectorySeparatorChar));
                    var thumbnailDestination = Path.Combine(
                        staging,
                        thumbnailRelativePath.Replace(
                            '/',
                            Path.DirectorySeparatorChar));

                    var texture = provider.LoadPackageObject<UTexture2D>(packagePath);
                    var decoded = texture.Decode()
                        ?? throw Failure(
                            $"game icon did not decode: {packagePath}");
                    if (decoded.PixelFormat is not (
                            EPixelFormat.PF_R8G8B8A8
                            or EPixelFormat.PF_B8G8R8A8)
                        || decoded.Width is <= 0 or > 4096
                        || decoded.Height is <= 0 or > 4096
                        || decoded.Data.Length
                            != checked(decoded.Width * decoded.Height * 4))
                    {
                        throw Failure(
                            $"game icon pixel contract changed: {packagePath}; "
                            + $"format={decoded.PixelFormat}; "
                            + $"size={decoded.Width}x{decoded.Height}; "
                            + $"bytes={decoded.Data.Length}");
                    }
                    using var image = DecodeImage(
                        decoded.Data,
                        decoded.Width,
                        decoded.Height,
                        decoded.PixelFormat);
                    var alphaBounds = FindAlphaBounds(image)
                        ?? throw Failure(
                            $"game icon has no visible pixels: {packagePath}");
                    var rgbaBytes = new byte[
                        checked(decoded.Width * decoded.Height * 4)];
                    image.CopyPixelDataTo(rgbaBytes);
                    var sourceDecodedRgbaSha256 =
                        Hashing.Sha256Hex(rgbaBytes);

                    image.Save(sourceDestination, PngEncoder);
                    using var content = image.Clone(context =>
                        context.Crop(new Rectangle(
                            alphaBounds.X,
                            alphaBounds.Y,
                            alphaBounds.Width,
                            alphaBounds.Height)));
                    content.Mutate(context => context.Resize(new ResizeOptions
                    {
                        Size = new Size(
                            iconContract.ThumbnailContentSizePx,
                            iconContract.ThumbnailContentSizePx),
                        Mode = ResizeMode.Max,
                        Sampler = KnownResamplers.Bicubic,
                        Compand = true,
                    }));
                    using var thumbnail = new Image<Rgba32>(
                        iconContract.ThumbnailSizePx,
                        iconContract.ThumbnailSizePx,
                        Color.Transparent);
                    thumbnail.Mutate(context => context.DrawImage(
                        content,
                        new Point(
                            (thumbnail.Width - content.Width) / 2,
                            (thumbnail.Height - content.Height) / 2),
                        1f));
                    thumbnail.Save(thumbnailDestination, PngEncoder);

                    var sourceInfo = new FileInfo(sourceDestination);
                    var thumbnailInfo = new FileInfo(thumbnailDestination);
                    totalPngBytes += sourceInfo.Length + thumbnailInfo.Length;
                    icons.Add(new ExtractedGameIcon(
                        group.GroupId,
                        GameIconProbe.LogicalId(packagePath, definition),
                        packagePath,
                        sourceRelativePath,
                        thumbnailRelativePath,
                        decoded.Width,
                        decoded.Height,
                        decoded.PixelFormat.ToString(),
                        sourceDecodedRgbaSha256,
                        Hashing.Sha256File(sourceDestination),
                        Hashing.Sha256File(thumbnailDestination),
                        alphaBounds));
                }
            }

            var manifest = new GameIconPackageManifest(
                CatalogContract.SchemaMajor,
                install.BuildId,
                probe.MappingSha256,
                iconContract.ReviewId,
                iconContract.DistributionScope,
                "installed_game_asset",
                OriginalGameAssetsOnly: true,
                iconContract.ThumbnailSizePx,
                iconContract.ThumbnailContentSizePx,
                "alpha_bounds_crop_then_bicubic_fit_and_center",
                icons);
            var manifestPath = Path.Combine(staging, "manifest.json");
            File.WriteAllBytes(
                manifestPath,
                JsonSerializer.SerializeToUtf8Bytes(manifest, JsonOptions));
            using (var stream = new FileStream(
                       manifestPath,
                       FileMode.Open,
                       FileAccess.Read,
                       FileShare.Read,
                       64 * 1024,
                       FileOptions.SequentialScan))
            {
                _ = JsonSerializer.Deserialize<GameIconPackageManifest>(
                        stream,
                        JsonOptions)
                    ?? throw Failure("game icon manifest could not be reopened");
            }
            if (icons.Count != probe.Groups.Sum(group => group.TextureCount)
                || icons
                    .GroupBy(
                        icon => $"{icon.GroupId}\0{icon.LogicalId}",
                        StringComparer.OrdinalIgnoreCase)
                    .Any(group => group.Count() != 1)
                || icons.Any(icon =>
                    !File.Exists(Path.Combine(
                        staging,
                        icon.SourceRelativePath.Replace(
                            '/',
                            Path.DirectorySeparatorChar)))
                    || !File.Exists(Path.Combine(
                        staging,
                        icon.ThumbnailRelativePath.Replace(
                            '/',
                            Path.DirectorySeparatorChar)))))
            {
                throw Failure("game icon package validation failed");
            }
            Directory.Move(staging, fullOutput);
            return new GameIconExtractionResult(
                fullOutput,
                Path.Combine(fullOutput, "manifest.json"),
                icons.Count,
                totalPngBytes);
        }
        catch (DataPackFailure)
        {
            throw;
        }
        catch (Exception error)
        {
            throw Failure(
                $"game icon extraction failed ({error.GetType().Name})");
        }
        finally
        {
            if (Directory.Exists(staging))
            {
                Directory.Delete(staging, recursive: true);
            }
        }
    }

    public static GameIconAlphaBounds? FindAlphaBounds(Image<Rgba32> image)
    {
        var minimumX = image.Width;
        var minimumY = image.Height;
        var maximumX = -1;
        var maximumY = -1;
        image.ProcessPixelRows(accessor =>
        {
            for (var y = 0; y < accessor.Height; y++)
            {
                var row = accessor.GetRowSpan(y);
                for (var x = 0; x < row.Length; x++)
                {
                    if (row[x].A == 0)
                    {
                        continue;
                    }
                    minimumX = Math.Min(minimumX, x);
                    minimumY = Math.Min(minimumY, y);
                    maximumX = Math.Max(maximumX, x);
                    maximumY = Math.Max(maximumY, y);
                }
            }
        });
        return maximumX < minimumX || maximumY < minimumY
            ? null
            : new GameIconAlphaBounds(
                minimumX,
                minimumY,
                maximumX - minimumX + 1,
                maximumY - minimumY + 1);
    }

    private static Image<Rgba32> DecodeImage(
        byte[] data,
        int width,
        int height,
        EPixelFormat pixelFormat)
    {
        if (pixelFormat == EPixelFormat.PF_R8G8B8A8)
        {
            return Image.LoadPixelData<Rgba32>(data, width, height);
        }
        using var bgra = Image.LoadPixelData<Bgra32>(data, width, height);
        return bgra.CloneAs<Rgba32>();
    }

    private static DataPackFailure Failure(string message) =>
        new(DataPackExitCode.SourceContractMismatch, message);
}
