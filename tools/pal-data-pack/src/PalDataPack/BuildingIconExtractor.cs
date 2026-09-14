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

public sealed record ExtractedBuildingIcon(
    string BuildingId,
    string IconSourcePath,
    string PackagePath,
    string RelativePath,
    int SourceWidth,
    int SourceHeight,
    string OutputSha256);

public sealed record BuildingIconManifest(
    int SchemaMajor,
    string GameBuildId,
    string MappingSha256,
    string WorldCatalogSha256,
    string LocalizationOrigin,
    bool OriginalGameAssetsOnly,
    int ThumbnailSizePx,
    IReadOnlyList<ExtractedBuildingIcon> Icons,
    IReadOnlyList<string> MissingOriginalBuildingIds);

public sealed record BuildingIconExtractionResult(
    string OutputDirectory,
    string ManifestPath,
    int IconCount,
    int MissingOriginalCount,
    long TotalPngBytes);

public static class BuildingIconExtractor
{
    private const int MaximumWorldCatalogBytes = 32 * 1024 * 1024;
    private const int ThumbnailSizePx = 128;

    private static readonly JsonSerializerOptions JsonOptions = new()
    {
        PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower,
        WriteIndented = true,
        UnmappedMemberHandling = JsonUnmappedMemberHandling.Disallow,
        MaxDepth = 64,
    };

    public static BuildingIconExtractionResult Extract(
        SteamInstall install,
        string mappingPath,
        AssetContract worldContract,
        string worldCatalogPath,
        string outputDirectory)
    {
        SecureInput.EnsureRegularFile(mappingPath);
        var mappingSha256 = Hashing.Sha256File(mappingPath);
        worldContract.RequireExtractionIdentity(install.BuildId, mappingSha256);
        var catalogBytes = SecureInput.ReadBoundedRegularFile(
            worldCatalogPath,
            MaximumWorldCatalogBytes);
        var catalogSha256 = Hashing.Sha256Hex(catalogBytes);
        if (worldContract.ExpectedOutputSha256 is null
            || !string.Equals(
                catalogSha256,
                worldContract.ExpectedOutputSha256,
                StringComparison.Ordinal))
        {
            throw Failure("building icons require the reviewed exact-Build world catalog");
        }
        var catalog = JsonSerializer.Deserialize<VerifiedWorldCatalogDocument>(
                catalogBytes,
                JsonOptions)
            ?? throw Failure("world catalog is empty");
        if (!catalog.Verified
            || catalog.GameBuildId != $"steam:{install.BuildId}"
            || catalog.MappingSha256 != mappingSha256)
        {
            throw Failure("world catalog identity does not match the icon inputs");
        }

        var fullOutput = Path.GetFullPath(outputDirectory);
        if (Directory.Exists(fullOutput) || File.Exists(fullOutput))
        {
            throw Failure("building icon output must not already exist");
        }
        var parent = Path.GetDirectoryName(fullOutput)
            ?? throw Failure("building icon output parent is invalid");
        Directory.CreateDirectory(parent);
        SecureInput.EnsurePlainDirectory(parent);
        var staging = Path.Combine(
            parent,
            $".{Path.GetFileName(fullOutput)}.{Guid.NewGuid():N}.staging");
        Directory.CreateDirectory(staging);
        Directory.CreateDirectory(Path.Combine(staging, "icons"));

        try
        {
            var provider = SourceContractProbe.OpenProvider(
                install.InstallPath,
                mappingPath);
            var icons = new List<ExtractedBuildingIcon>();
            var missing = new List<string>();
            long totalBytes = 0;
            foreach (var building in catalog.Buildings
                         .Where(value =>
                             !value.LocalizationFallback
                             && !value.InDevelopment)
                         .OrderBy(value => value.BuildingId, StringComparer.Ordinal))
            {
                if (string.IsNullOrWhiteSpace(building.IconSourcePath))
                {
                    missing.Add(building.BuildingId);
                    continue;
                }
                var packagePath = NormalizeObjectPath(building.IconSourcePath);
                var fileName = Hashing.Sha256Hex(
                    Encoding.UTF8.GetBytes(packagePath))[..20] + ".png";
                var relativePath = $"icons/{fileName}";
                var destination = Path.Combine(staging, "icons", fileName);
                var texture = provider.LoadPackageObject<UTexture2D>(packagePath);
                var decoded = texture.Decode()
                    ?? throw Failure($"building icon did not decode: {packagePath}");
                if (decoded.PixelFormat is not (
                        EPixelFormat.PF_R8G8B8A8 or EPixelFormat.PF_B8G8R8A8)
                    || decoded.Width is <= 0 or > 4096
                    || decoded.Height is <= 0 or > 4096
                    || decoded.Data.Length != checked(decoded.Width * decoded.Height * 4))
                {
                    throw Failure(
                        $"building icon pixel contract changed: {packagePath}");
                }
                using var image = DecodeImage(
                    decoded.Data,
                    decoded.Width,
                    decoded.Height,
                    decoded.PixelFormat);
                image.Mutate(context => context.Resize(new ResizeOptions
                {
                    Size = new Size(ThumbnailSizePx, ThumbnailSizePx),
                    Mode = ResizeMode.Max,
                    Sampler = KnownResamplers.Bicubic,
                    Compand = true,
                }));
                image.Save(destination, new PngEncoder
                {
                    ColorType = PngColorType.RgbWithAlpha,
                    CompressionLevel = PngCompressionLevel.Level9,
                });
                var info = new FileInfo(destination);
                totalBytes += info.Length;
                icons.Add(new ExtractedBuildingIcon(
                    building.BuildingId,
                    building.IconSourcePath,
                    packagePath,
                    relativePath,
                    decoded.Width,
                    decoded.Height,
                    Hashing.Sha256File(destination)));
            }
            var manifest = new BuildingIconManifest(
                CatalogContract.SchemaMajor,
                install.BuildId,
                mappingSha256,
                catalogSha256,
                "game_asset",
                OriginalGameAssetsOnly: true,
                ThumbnailSizePx,
                icons,
                missing);
            var manifestPath = Path.Combine(staging, "manifest.json");
            File.WriteAllBytes(
                manifestPath,
                JsonSerializer.SerializeToUtf8Bytes(manifest, JsonOptions));
            if (icons.Count == 0
                || icons.Any(icon =>
                    !File.Exists(Path.Combine(
                        staging,
                        icon.RelativePath.Replace(
                            '/',
                            Path.DirectorySeparatorChar)))))
            {
                throw Failure("building icon package validation failed");
            }
            Directory.Move(staging, fullOutput);
            return new BuildingIconExtractionResult(
                fullOutput,
                Path.Combine(fullOutput, "manifest.json"),
                icons.Count,
                missing.Count,
                totalBytes);
        }
        catch (DataPackFailure)
        {
            throw;
        }
        catch (Exception error)
        {
            throw Failure(
                $"building icon extraction failed ({error.GetType().Name})");
        }
        finally
        {
            if (Directory.Exists(staging))
            {
                Directory.Delete(staging, recursive: true);
            }
        }
    }

    private static string NormalizeObjectPath(string source)
    {
        const string prefix = "/Game/";
        if (!source.StartsWith(prefix, StringComparison.Ordinal)
            || source.Any(char.IsControl))
        {
            throw Failure("building icon source path is invalid");
        }
        var objectSeparator = source.LastIndexOf('.');
        var package = objectSeparator > prefix.Length
            ? source[..objectSeparator]
            : source;
        return "Pal/Content/" + package[prefix.Length..];
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
