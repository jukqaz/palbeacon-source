using System.Text;
using System.Text.Json;
using CUE4Parse.UE4.Assets.Exports.Texture;
using CUE4Parse.UE4.Assets.Exports.Engine;
using CUE4Parse_Conversion.Textures;
using SixLabors.ImageSharp;
using SixLabors.ImageSharp.Formats.Png;
using SixLabors.ImageSharp.PixelFormats;
using SixLabors.ImageSharp.Processing;

namespace PalDataPack;

public sealed record ExtractedItemIcon(
    string IconName,
    string PackagePath,
    string RelativePath,
    string MatchKind,
    int ReferencingItemCount,
    bool LegalInGame,
    int SourceWidth,
    int SourceHeight,
    string OutputSha256);

public sealed record ItemIconPackageManifest(
    int SchemaMajor,
    string GameBuildId,
    string MappingSha256,
    string ItemCatalogSha256,
    string ReviewId,
    int ThumbnailSizePx,
    int UniqueTextureCount,
    IReadOnlyList<ExtractedItemIcon> Icons,
    IReadOnlyList<string> LegalMissingIconNames);

public sealed record ItemIconExtractionResult(
    string OutputDirectory,
    string ManifestPath,
    int IconNameCount,
    int UniqueTextureCount,
    long TotalPngBytes);

public static class ItemIconExtractor
{
    private static readonly JsonSerializerOptions ManifestJsonOptions = new()
    {
        WriteIndented = true,
        PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower,
    };

    public static ItemIconExtractionResult Extract(
        SteamInstall install,
        string mappingPath,
        AssetContract itemCatalogContract,
        string itemCatalogPath,
        ItemIconContract iconContract,
        string outputDirectory)
    {
        var probe = ItemIconProbe.Probe(
            install,
            mappingPath,
            itemCatalogContract,
            itemCatalogPath);
        iconContract.RequireProbe(probe);
        var fullOutput = Path.GetFullPath(outputDirectory);
        if (Directory.Exists(fullOutput) || File.Exists(fullOutput))
        {
            throw Failure("item icon output must not already exist");
        }
        var parent = Path.GetDirectoryName(fullOutput)
            ?? throw Failure("item icon output parent is invalid");
        Directory.CreateDirectory(parent);
        SecureInput.EnsurePlainDirectory(parent);
        var staging = Path.Combine(
            parent,
            $".{Path.GetFileName(fullOutput)}.{Guid.NewGuid():N}.staging");
        Directory.CreateDirectory(staging);
        var iconDirectory = Path.Combine(staging, "icons");
        Directory.CreateDirectory(iconDirectory);

        try
        {
            var provider = SourceContractProbe.OpenProvider(
                install.InstallPath,
                mappingPath);
            var extractedByPackage = new Dictionary<string, ExtractedTexture>(
                StringComparer.OrdinalIgnoreCase);
            foreach (var packagePath in probe.Matches
                         .Select(match => match.PackagePath)
                         .Distinct(StringComparer.OrdinalIgnoreCase)
                         .Order(StringComparer.OrdinalIgnoreCase))
            {
                var packageKey = Hashing.Sha256Hex(
                    Encoding.UTF8.GetBytes(packagePath))[..20];
                var relativePath = $"icons/{packageKey}.png";
                var destination = Path.Combine(
                    staging,
                    relativePath.Replace('/', Path.DirectorySeparatorChar));
                var texture = provider.LoadPackageObject<UTexture2D>(packagePath);
                var decoded = texture.Decode()
                    ?? throw Failure($"item icon did not decode: {packagePath}");
                if (decoded.PixelFormat is not (
                        EPixelFormat.PF_R8G8B8A8 or EPixelFormat.PF_B8G8R8A8)
                    || decoded.Width is <= 0 or > 4096
                    || decoded.Height is <= 0 or > 4096
                    || decoded.Data.Length != checked(decoded.Width * decoded.Height * 4))
                {
                    throw Failure(
                        $"item icon pixel contract changed: {packagePath}; "
                        + $"format={decoded.PixelFormat}; "
                        + $"size={decoded.Width}x{decoded.Height}; "
                        + $"bytes={decoded.Data.Length}");
                }
                using var image = DecodeImage(
                    decoded.Data,
                    decoded.Width,
                    decoded.Height,
                    decoded.PixelFormat);
                image.Mutate(context => context.Resize(new ResizeOptions
                {
                    Size = new Size(
                        iconContract.ThumbnailSizePx,
                        iconContract.ThumbnailSizePx),
                    Mode = ResizeMode.Max,
                    Sampler = KnownResamplers.Bicubic,
                    Compand = true,
                }));
                image.Save(destination, new PngEncoder
                {
                    ColorType = PngColorType.RgbWithAlpha,
                    CompressionLevel = PngCompressionLevel.Level9,
                });
                var outputSha256 = Hashing.Sha256File(destination);
                extractedByPackage.Add(
                    packagePath,
                    new ExtractedTexture(
                        relativePath,
                        decoded.Width,
                        decoded.Height,
                        outputSha256,
                        new FileInfo(destination).Length));
            }

            var icons = probe.Matches
                .OrderBy(match => match.IconName, StringComparer.Ordinal)
                .Select(match =>
                {
                    var texture = extractedByPackage[match.PackagePath];
                    return new ExtractedItemIcon(
                        match.IconName,
                        match.PackagePath,
                        texture.RelativePath,
                        match.MatchKind,
                        match.ReferencingItemCount,
                        match.LegalInGame,
                        texture.SourceWidth,
                        texture.SourceHeight,
                        texture.OutputSha256);
                })
                .ToArray();
            var manifest = new ItemIconPackageManifest(
                CatalogContract.SchemaMajor,
                install.BuildId,
                probe.MappingSha256,
                probe.ItemCatalogSha256,
                iconContract.ReviewId,
                iconContract.ThumbnailSizePx,
                extractedByPackage.Count,
                icons,
                probe.LegalMissingIconNames);
            var manifestPath = Path.Combine(staging, "manifest.json");
            File.WriteAllBytes(
                manifestPath,
                JsonSerializer.SerializeToUtf8Bytes(manifest, ManifestJsonOptions));
            using (var stream = new FileStream(
                       manifestPath,
                       FileMode.Open,
                       FileAccess.Read,
                       FileShare.Read,
                       64 * 1024,
                       FileOptions.SequentialScan))
            {
                _ = JsonSerializer.Deserialize<ItemIconPackageManifest>(
                        stream,
                        ManifestJsonOptions)
                    ?? throw Failure("item icon manifest could not be reopened");
            }
            if (icons.Length != probe.Matches.Count
                || extractedByPackage.Count == 0
                || extractedByPackage.Values.Any(texture =>
                    !File.Exists(Path.Combine(
                        staging,
                        texture.RelativePath.Replace(
                            '/',
                            Path.DirectorySeparatorChar)))))
            {
                throw Failure("item icon package validation failed");
            }
            Directory.Move(staging, fullOutput);
            return new ItemIconExtractionResult(
                fullOutput,
                Path.Combine(fullOutput, "manifest.json"),
                icons.Length,
                extractedByPackage.Count,
                extractedByPackage.Values.Sum(texture => texture.OutputBytes));
        }
        catch (DataPackFailure)
        {
            throw;
        }
        catch (Exception error)
        {
            throw Failure($"item icon extraction failed ({error.GetType().Name})");
        }
        finally
        {
            if (Directory.Exists(staging))
            {
                Directory.Delete(staging, recursive: true);
            }
        }
    }

    private static DataPackFailure Failure(string message) =>
        new(DataPackExitCode.SourceContractMismatch, message);

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

    private sealed record ExtractedTexture(
        string RelativePath,
        int SourceWidth,
        int SourceHeight,
        string OutputSha256,
        long OutputBytes);
}
