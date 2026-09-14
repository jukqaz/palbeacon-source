using System.Buffers.Binary;

namespace PalMapPack;

public sealed record CalibrationPreviewArtifact(
    byte[] Bytes,
    CalibrationPreviewContract Contract);

public static class CalibrationPreviewBuilder
{
    public const string Algorithm = "uniform-box-average-rgba8-to-bmp24-v1";
    public const int MaximumFileBytes = 64 * 1024 * 1024;
    public const int DefaultWidth = 2048;
    public const int DefaultHeight = 2048;
    private const int BitmapHeaderBytes = 54;
    private const int MaximumScale = 16;

    public static CalibrationPreviewArtifact BuildDeterministicBmp(
        RgbaMap source,
        string sourceMapAssetSha256,
        int width,
        int height)
    {
        ArgumentNullException.ThrowIfNull(source);
        if (!AssetContract.CanonicalNonzeroSha256(sourceMapAssetSha256)
            || Hashing.MapAssetSha256(source) != sourceMapAssetSha256)
        {
            throw Failure("calibration preview source hash does not match its decoded map");
        }
        if (!TryExpectedFileSize(width, height, out var expectedFileSize)
            || expectedFileSize > MaximumFileBytes)
        {
            throw Failure("calibration preview exceeds the deterministic file-size budget");
        }
        var runtimeMap = BuildDeterministicRuntimeMap(
            source,
            sourceMapAssetSha256,
            width,
            height);
        var bmp = EncodeBmp24(width, height, runtimeMap.Rgba);
        return new CalibrationPreviewArtifact(
            bmp,
            new CalibrationPreviewContract(
                width,
                height,
                bmp.LongLength,
                Hashing.Sha256Hex(bmp),
                sourceMapAssetSha256,
                Algorithm));
    }

    /// <summary>
    /// Produces the bounded, deterministic raster used by the runtime map pack.
    ///
    /// Keeping this operation in the extraction gate prevents the overlay from decoding and
    /// retaining the full 8192px source texture. The source texture remains content-bound by the
    /// exact-Build contract before this derived raster is created.
    /// </summary>
    public static RgbaMap BuildDeterministicRuntimeMap(
        RgbaMap source,
        string sourceMapAssetSha256,
        int width,
        int height)
    {
        ArgumentNullException.ThrowIfNull(source);
        if (!AssetContract.CanonicalNonzeroSha256(sourceMapAssetSha256)
            || Hashing.MapAssetSha256(source) != sourceMapAssetSha256)
        {
            throw Failure("runtime map source hash does not match its decoded map");
        }
        return new RgbaMap(width, height, Downsample(source, width, height));
    }

    public static (int Width, int Height) SelectDimensions(
        RgbaMap source,
        CalibrationPreviewContract? boundPreview)
    {
        ArgumentNullException.ThrowIfNull(source);
        if (boundPreview is not null)
        {
            return (boundPreview.WidthPx, boundPreview.HeightPx);
        }
        for (var scale = 1; scale <= MaximumScale; scale++)
        {
            if (source.Width % scale == 0
                && source.Height % scale == 0
                && source.Width / scale <= DefaultWidth
                && source.Height / scale <= DefaultHeight)
            {
                return (source.Width / scale, source.Height / scale);
            }
        }
        throw Failure("decoded map has no bounded deterministic calibration preview scale");
    }

    public static CalibrationPreviewContract WriteDeterministicBmp(
        string path,
        RgbaMap source,
        string sourceMapAssetSha256,
        int width = DefaultWidth,
        int height = DefaultHeight)
    {
        var artifact = BuildDeterministicBmp(source, sourceMapAssetSha256, width, height);
        WriteDeterministicBmp(path, artifact);
        return artifact.Contract;
    }

    public static void WriteDeterministicBmp(
        string path,
        CalibrationPreviewArtifact artifact)
    {
        ArgumentNullException.ThrowIfNull(artifact);
        var bmp = artifact.Bytes;
        var descriptor = artifact.Contract;
        if (bmp.LongLength != descriptor.FileSizeBytes
            || Hashing.Sha256Hex(bmp) != descriptor.Sha256
            || descriptor.Algorithm != Algorithm
            || !AssetContract.CanonicalNonzeroSha256(descriptor.SourceMapAssetSha256)
            || !HasExactDeterministicLayout(descriptor)
            || !HasCanonicalHeader(bmp, descriptor.WidthPx, descriptor.HeightPx))
        {
            throw Failure("calibration preview artifact does not match its descriptor");
        }
        var destination = Path.GetFullPath(path);
        var parent = Directory.GetParent(destination)?.FullName
            ?? throw Failure("calibration preview output has no parent directory");
        if (!Directory.Exists(parent))
        {
            throw Failure("calibration preview output parent does not exist");
        }
        var temporary = Path.Combine(
            parent,
            $".{Path.GetFileName(destination)}.{Guid.NewGuid():N}.tmp");
        try
        {
            using var parentLease = SecureDirectoryLease.OpenExisting(parent);
            if (File.Exists(destination))
            {
                var existing = SecureFiles.ReadAllBytes(destination, MaximumFileBytes);
                if (existing.AsSpan().SequenceEqual(bmp))
                {
                    return;
                }
                throw Failure("existing calibration preview differs and will not be replaced");
            }
            using (var stream = new FileStream(
                temporary,
                FileMode.CreateNew,
                FileAccess.Write,
                FileShare.None,
                bufferSize: 64 * 1024,
                FileOptions.WriteThrough))
            {
                stream.Write(bmp);
                stream.Flush(flushToDisk: true);
            }
            SecureFiles.EnsureRegularFile(temporary);
            File.Move(temporary, destination);
            SecureFiles.EnsureRegularFile(destination);
        }
        catch (Exception error) when (error is IOException
            or UnauthorizedAccessException
            or MapPackFailure)
        {
            throw Failure("calibration preview could not be written safely");
        }
        finally
        {
            if (File.Exists(temporary))
            {
                File.Delete(temporary);
            }
        }
    }

    public static byte[] VerifyExactFile(
        string path,
        CalibrationPreviewContract expected,
        string expectedSourceMapAssetSha256)
    {
        ArgumentNullException.ThrowIfNull(expected);
        try
        {
            if (expected.SourceMapAssetSha256 != expectedSourceMapAssetSha256
                || expected.Algorithm != Algorithm
                || !HasExactDeterministicLayout(expected)
                || !AssetContract.CanonicalNonzeroSha256(expected.Sha256))
            {
                throw Failure("calibration preview contract is invalid");
            }
            var bytes = SecureFiles.ReadAllBytes(path, MaximumFileBytes);
            if (bytes.LongLength != expected.FileSizeBytes
                || Hashing.Sha256Hex(bytes) != expected.Sha256
                || !HasCanonicalHeader(bytes, expected.WidthPx, expected.HeightPx))
            {
                throw Failure("calibration preview does not match the exact-Build contract");
            }
            return bytes;
        }
        catch (Exception error) when (error is IOException
            or UnauthorizedAccessException
            or MapPackFailure)
        {
            throw Failure("calibration preview is missing, unsafe, or does not match");
        }
    }

    internal static bool HasExactDeterministicLayout(CalibrationPreviewContract preview) =>
        TryExpectedFileSize(preview.WidthPx, preview.HeightPx, out var expectedBytes)
        && expectedBytes == preview.FileSizeBytes
        && expectedBytes <= MaximumFileBytes;

    private static bool TryExpectedFileSize(int width, int height, out long fileSize)
    {
        fileSize = 0;
        if (width <= 0 || height <= 0)
        {
            return false;
        }
        try
        {
            var rowBytes = checked((long)width * 3);
            var rowStride = checked((rowBytes + 3) & ~3L);
            fileSize = checked(BitmapHeaderBytes + rowStride * height);
            return fileSize > BitmapHeaderBytes;
        }
        catch (OverflowException)
        {
            return false;
        }
    }

    private static bool HasCanonicalHeader(byte[] bytes, int width, int height)
    {
        if (bytes.Length < BitmapHeaderBytes
            || bytes[0] != (byte)'B'
            || bytes[1] != (byte)'M'
            || BinaryPrimitives.ReadUInt32LittleEndian(bytes.AsSpan(2, 4)) != bytes.Length
            || bytes.AsSpan(6, 4).IndexOfAnyExcept((byte)0) >= 0
            || BinaryPrimitives.ReadUInt32LittleEndian(bytes.AsSpan(10, 4)) != BitmapHeaderBytes
            || BinaryPrimitives.ReadUInt32LittleEndian(bytes.AsSpan(14, 4)) != 40
            || BinaryPrimitives.ReadInt32LittleEndian(bytes.AsSpan(18, 4)) != width
            || BinaryPrimitives.ReadInt32LittleEndian(bytes.AsSpan(22, 4)) != height
            || BinaryPrimitives.ReadUInt16LittleEndian(bytes.AsSpan(26, 2)) != 1
            || BinaryPrimitives.ReadUInt16LittleEndian(bytes.AsSpan(28, 2)) != 24
            || BinaryPrimitives.ReadUInt32LittleEndian(bytes.AsSpan(30, 4)) != 0
            || bytes.AsSpan(38, 16).IndexOfAnyExcept((byte)0) >= 0)
        {
            return false;
        }
        var pixelBytes = checked(bytes.Length - BitmapHeaderBytes);
        return BinaryPrimitives.ReadUInt32LittleEndian(bytes.AsSpan(34, 4)) == pixelBytes;
    }

    private static byte[] Downsample(RgbaMap source, int width, int height)
    {
        if (width <= 0
            || height <= 0
            || width > source.Width
            || height > source.Height
            || source.Width % width != 0
            || source.Height % height != 0)
        {
            throw Failure("calibration preview dimensions must evenly divide the decoded map");
        }
        var scaleX = source.Width / width;
        var scaleY = source.Height / height;
        if (scaleX != scaleY || scaleX > MaximumScale)
        {
            throw Failure("calibration preview requires one bounded uniform integer scale");
        }
        var count = checked((ulong)scaleX * (ulong)scaleY);
        var output = new byte[checked(width * height * 4)];
        for (var destinationY = 0; destinationY < height; destinationY++)
        {
            for (var destinationX = 0; destinationX < width; destinationX++)
            {
                ulong red = 0;
                ulong green = 0;
                ulong blue = 0;
                ulong alpha = 0;
                for (var offsetY = 0; offsetY < scaleY; offsetY++)
                {
                    var sourceY = destinationY * scaleY + offsetY;
                    for (var offsetX = 0; offsetX < scaleX; offsetX++)
                    {
                        var sourceX = destinationX * scaleX + offsetX;
                        var sourceOffset = checked((sourceY * source.Width + sourceX) * 4);
                        red += source.Rgba[sourceOffset];
                        green += source.Rgba[sourceOffset + 1];
                        blue += source.Rgba[sourceOffset + 2];
                        alpha += source.Rgba[sourceOffset + 3];
                    }
                }
                var destinationOffset = checked((destinationY * width + destinationX) * 4);
                output[destinationOffset] = Average(red, count);
                output[destinationOffset + 1] = Average(green, count);
                output[destinationOffset + 2] = Average(blue, count);
                output[destinationOffset + 3] = Average(alpha, count);
            }
        }
        return output;
    }

    private static byte[] EncodeBmp24(int width, int height, byte[] rgba)
    {
        var rowBytes = checked(width * 3);
        var rowStride = checked((rowBytes + 3) & ~3);
        var pixelBytes = checked(rowStride * height);
        var fileBytes = checked(BitmapHeaderBytes + pixelBytes);
        if (fileBytes > MaximumFileBytes)
        {
            throw Failure("calibration preview exceeds the file-size budget");
        }
        var bmp = new byte[fileBytes];
        bmp[0] = (byte)'B';
        bmp[1] = (byte)'M';
        BinaryPrimitives.WriteUInt32LittleEndian(bmp.AsSpan(2, 4), checked((uint)fileBytes));
        BinaryPrimitives.WriteUInt32LittleEndian(bmp.AsSpan(10, 4), BitmapHeaderBytes);
        BinaryPrimitives.WriteUInt32LittleEndian(bmp.AsSpan(14, 4), 40);
        BinaryPrimitives.WriteInt32LittleEndian(bmp.AsSpan(18, 4), width);
        BinaryPrimitives.WriteInt32LittleEndian(bmp.AsSpan(22, 4), height);
        BinaryPrimitives.WriteUInt16LittleEndian(bmp.AsSpan(26, 2), 1);
        BinaryPrimitives.WriteUInt16LittleEndian(bmp.AsSpan(28, 2), 24);
        BinaryPrimitives.WriteUInt32LittleEndian(bmp.AsSpan(34, 4), checked((uint)pixelBytes));
        for (var destinationRow = 0; destinationRow < height; destinationRow++)
        {
            var sourceY = height - destinationRow - 1;
            var destinationOffset = BitmapHeaderBytes + destinationRow * rowStride;
            for (var x = 0; x < width; x++)
            {
                var sourceOffset = (sourceY * width + x) * 4;
                var pixelOffset = destinationOffset + x * 3;
                bmp[pixelOffset] = rgba[sourceOffset + 2];
                bmp[pixelOffset + 1] = rgba[sourceOffset + 1];
                bmp[pixelOffset + 2] = rgba[sourceOffset];
            }
        }
        return bmp;
    }

    private static byte Average(ulong sum, ulong count) =>
        checked((byte)((sum + count / 2) / count));

    private static MapPackFailure Failure(string message) =>
        new(ExitCodes.AssetContractMismatch, message);
}
