using System.Security.Cryptography;
using System.Text;

namespace PalDataPack;

public static class Hashing
{
    public static string Sha256Hex(ReadOnlySpan<byte> bytes) =>
        Convert.ToHexStringLower(SHA256.HashData(bytes));

    public static string Sha256File(string path)
    {
        using var stream = new FileStream(
            path,
            FileMode.Open,
            FileAccess.Read,
            FileShare.Read,
            bufferSize: 1024 * 1024,
            FileOptions.SequentialScan);
        return Convert.ToHexStringLower(SHA256.HashData(stream));
    }

    public static string DatasetManifestId(
        int schemaMajor,
        string gameBuildId,
        string distributionScope,
        IReadOnlyList<SourceFingerprint> sources,
        IReadOnlyList<CapabilityStatus> capabilities,
        IReadOnlyList<PackageFile> files)
    {
        using var hash = IncrementalHash.CreateHash(HashAlgorithmName.SHA256);
        Append(hash, schemaMajor.ToString(System.Globalization.CultureInfo.InvariantCulture));
        Append(hash, gameBuildId);
        Append(hash, distributionScope);
        foreach (var source in sources)
        {
            Append(hash, source.SourceId);
            Append(hash, source.SourceKind);
            Append(hash, source.SourceSha256);
            Append(hash, source.Verified ? "1" : "0");
        }
        foreach (var capability in capabilities)
        {
            Append(hash, capability.CapabilityId);
            Append(hash, capability.Status.ToWireValue());
            foreach (var sourceId in capability.SourceIds)
            {
                Append(hash, sourceId);
            }
        }
        foreach (var file in files)
        {
            Append(hash, file.Path);
            Append(hash, file.RowCount.ToString(System.Globalization.CultureInfo.InvariantCulture));
            Append(hash, file.FileSizeBytes.ToString(System.Globalization.CultureInfo.InvariantCulture));
            Append(hash, file.Sha256);
        }
        return Convert.ToHexStringLower(hash.GetHashAndReset());
    }

    private static void Append(IncrementalHash hash, string value)
    {
        var bytes = Encoding.UTF8.GetBytes(value);
        Span<byte> length = stackalloc byte[4];
        System.Buffers.Binary.BinaryPrimitives.WriteUInt32LittleEndian(
            length,
            checked((uint)bytes.Length));
        hash.AppendData(length);
        hash.AppendData(bytes);
    }
}
