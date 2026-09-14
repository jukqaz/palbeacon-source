using System.Buffers.Binary;
using System.Security.Cryptography;
using System.Text;

namespace PalMapPack;

public static class ExitCodes
{
    public const int Success = 0;
    public const int Usage = 2;
    public const int MissingMapping = 12;
    public const int MountSerialization = 13;
    public const int AssetContractMismatch = 14;
    public const int CalibrationRejected = 15;
    public const int PackIntegrity = 16;
}

public sealed class MapPackFailure(int exitCode, string message) : Exception(message)
{
    public int ExitCode { get; } = exitCode;
}

public sealed record SourceContainerPath(string FullPath, string RelativeName);

public sealed record SourceContainerSnapshot(
    string RelativeName,
    long SizeBytes,
    long LastWriteUtcTicks,
    string Sha256,
    uint VolumeSerial = 0,
    ulong FileIndex = 0)
{
    public static SourceContainerSnapshot Capture(string root, string path)
    {
        var fullRoot = Path.GetFullPath(root);
        var fullPath = Path.GetFullPath(path);
        if (!IsWithin(fullRoot, fullPath))
        {
            throw new MapPackFailure(ExitCodes.PackIntegrity, "source container escaped the install root");
        }

        var snapshot = SecureFiles.SnapshotWithin(fullRoot, fullPath);
        var relativeName = Path.GetRelativePath(fullRoot, snapshot.FinalPath).Replace('\\', '/');
        return new SourceContainerSnapshot(
            relativeName,
            snapshot.SizeBytes,
            snapshot.LastWriteUtcTicks,
            snapshot.Sha256,
            snapshot.VolumeSerial,
            snapshot.FileIndex);
    }

    public static void EnsureUnchanged(
        IEnumerable<SourceContainerSnapshot> before,
        IEnumerable<SourceContainerSnapshot> after)
    {
        var first = before.OrderBy(item => item.RelativeName, StringComparer.Ordinal).ToArray();
        var second = after.OrderBy(item => item.RelativeName, StringComparer.Ordinal).ToArray();
        if (!first.SequenceEqual(second))
        {
            throw new MapPackFailure(
                ExitCodes.PackIntegrity,
                "Steam build or mounted container set changed during extraction");
        }
    }

    internal static bool IsWithin(string root, string candidate)
    {
        var rootWithSeparator = root.TrimEnd(Path.DirectorySeparatorChar, Path.AltDirectorySeparatorChar)
            + Path.DirectorySeparatorChar;
        return candidate.StartsWith(rootWithSeparator, StringComparison.OrdinalIgnoreCase);
    }
}

public static class AssetPaths
{
    private static readonly HashSet<string> ContainerExtensions =
        new(StringComparer.OrdinalIgnoreCase) { ".pak", ".utoc", ".ucas" };

    public static IReadOnlyList<SourceContainerPath> DiscoverContainers(string installPath)
    {
        var root = Path.GetFullPath(installPath);
        var paks = Path.Combine(root, "Pal", "Content", "Paks");
        if (!Directory.Exists(paks))
        {
            throw new MapPackFailure(ExitCodes.MountSerialization, "Palworld container directory was not found");
        }

        using var rootLease = SecureDirectoryLease.OpenExisting(root);
        using var paksLease = SecureDirectoryLease.OpenExisting(paks);
        var containers = EnumeratePlainFiles(paks)
            .Where(path => ContainerExtensions.Contains(Path.GetExtension(path)))
            .Select(path =>
            {
                SecureFiles.EnsureRegularFile(path);
                return path;
            })
            .Select(path => new SourceContainerPath(
                Path.GetFullPath(path),
                Path.GetRelativePath(root, path).Replace('\\', '/')))
            .OrderBy(path => path.RelativeName, StringComparer.Ordinal)
            .ToArray();
        if (containers.Length == 0)
        {
            throw new MapPackFailure(ExitCodes.MountSerialization, "no PAK/UTOC/UCAS containers were found");
        }
        return containers;
    }

    private static IEnumerable<string> EnumeratePlainFiles(string root)
    {
        var pending = new Stack<string>();
        pending.Push(root);
        while (pending.Count > 0)
        {
            var directory = pending.Pop();
            using var lease = SecureDirectoryLease.OpenExisting(directory);
            foreach (var entry in new DirectoryInfo(directory).EnumerateFileSystemInfos())
            {
                if ((entry.Attributes & FileAttributes.ReparsePoint) != 0)
                {
                    throw new MapPackFailure(
                        ExitCodes.PackIntegrity,
                        "source container tree contains a reparse point");
                }
                if (entry is DirectoryInfo child)
                {
                    pending.Push(child.FullName);
                }
                else if (entry is FileInfo file)
                {
                    yield return file.FullName;
                }
            }
        }
    }
}

public interface IExtractionInputGuard
{
    void EnsureUnchanged();
}

public sealed class ExtractionInputGuard : IExtractionInputGuard
{
    private readonly string _appId;
    private readonly string[] _libraryRoots;
    private readonly string _buildId;
    private readonly string _installPath;
    private readonly SourceContainerSnapshot[] _snapshots;

    private ExtractionInputGuard(
        string appId,
        string[] libraryRoots,
        SteamInstall install,
        SourceContainerSnapshot[] snapshots)
    {
        _appId = appId;
        _libraryRoots = libraryRoots;
        _buildId = install.BuildId;
        _installPath = install.InstallPath;
        _snapshots = snapshots;
    }

    public static ExtractionInputGuard Capture(string appId, IEnumerable<string> libraryRoots)
    {
        var roots = libraryRoots.Select(Path.GetFullPath)
            .Distinct(StringComparer.OrdinalIgnoreCase)
            .ToArray();
        var install = SteamInstallLocator.Locate(appId, roots);
        var snapshots = AssetPaths.DiscoverContainers(install.InstallPath)
            .Select(path => SourceContainerSnapshot.Capture(install.InstallPath, path.FullPath))
            .ToArray();
        return new ExtractionInputGuard(appId, roots, install, snapshots);
    }

    public IReadOnlyList<SourceContainerSnapshot> Snapshots => _snapshots;

    public void EnsureUnchanged()
    {
        var currentInstall = SteamInstallLocator.Locate(_appId, _libraryRoots);
        if (currentInstall.BuildId != _buildId
            || !string.Equals(currentInstall.InstallPath, _installPath, StringComparison.OrdinalIgnoreCase))
        {
            throw new MapPackFailure(
                ExitCodes.PackIntegrity,
                "Steam Build identity or install path changed during extraction");
        }
        var current = AssetPaths.DiscoverContainers(currentInstall.InstallPath)
            .Select(path => SourceContainerSnapshot.Capture(currentInstall.InstallPath, path.FullPath))
            .ToArray();
        SourceContainerSnapshot.EnsureUnchanged(_snapshots, current);
    }
}

public static class Hashing
{
    public static string Sha256Hex(ReadOnlySpan<byte> bytes) =>
        Convert.ToHexStringLower(SHA256.HashData(bytes));

    public static string Sha256File(string path)
    {
        var full = Path.GetFullPath(path);
        var parent = Directory.GetParent(full)?.FullName
            ?? throw new MapPackFailure(ExitCodes.PackIntegrity, "file has no protected parent");
        return SecureFiles.SnapshotWithin(parent, full).Sha256;
    }

    public static string SourceInputSetSha256(IEnumerable<SourceContainerSnapshot> containers)
    {
        var sorted = containers.OrderBy(item => item.RelativeName, StringComparer.Ordinal).ToArray();
        using var writer = new CanonicalHashWriter("pal-source-input-v1\0"u8);
        writer.WriteUInt32(checked((uint)sorted.Length));
        foreach (var container in sorted)
        {
            ValidateRelativeName(container.RelativeName);
            if (container.SizeBytes <= 0)
            {
                throw new MapPackFailure(ExitCodes.PackIntegrity, "source container size must be positive");
            }
            writer.WriteString(container.RelativeName);
            writer.WriteUInt64(checked((ulong)container.SizeBytes));
            writer.WriteHash(container.Sha256);
        }
        return writer.Finish();
    }

    public static string MapAssetSha256(RgbaMap map)
    {
        using var writer = new CanonicalHashWriter("pal-map-asset-v1\0"u8);
        writer.WriteUInt32(checked((uint)map.Width));
        writer.WriteUInt32(checked((uint)map.Height));
        writer.WriteBytes(map.Rgba);
        return writer.Finish();
    }

    internal static string TileSetSha256(IEnumerable<TileDescriptorDocument> tiles)
    {
        var sorted = tiles.OrderBy(tile => tile.Level)
            .ThenBy(tile => tile.Y)
            .ThenBy(tile => tile.X)
            .ToArray();
        using var writer = new CanonicalHashWriter("pal-tile-set-v1\0"u8);
        writer.WriteUInt32(checked((uint)sorted.Length));
        foreach (var tile in sorted)
        {
            writer.WriteByte(tile.Level);
            writer.WriteUInt32(tile.Y);
            writer.WriteUInt32(tile.X);
            writer.WriteString(tile.RelativePath);
            writer.WriteUInt64(checked((ulong)tile.SizeBytes));
            writer.WriteHash(tile.Sha256);
        }
        return writer.Finish();
    }

    internal static void ValidateHash(string value, string field)
    {
        if (value.Length != 64 || value.Any(character =>
                !(character is >= '0' and <= '9' or >= 'a' and <= 'f')))
        {
            throw new MapPackFailure(ExitCodes.PackIntegrity, $"{field} is not canonical lowercase SHA-256");
        }
    }

    internal static void ValidateRelativeName(string value)
    {
        if (string.IsNullOrWhiteSpace(value)
            || Path.IsPathRooted(value)
            || value.Contains('\\', StringComparison.Ordinal)
            || value.Split('/').Any(part => part is "" or "." or ".."))
        {
            throw new MapPackFailure(ExitCodes.PackIntegrity, "unsafe relative source container name");
        }
    }

    private sealed class CanonicalHashWriter(ReadOnlySpan<byte> domain) : IDisposable
    {
        private readonly IncrementalHash _hash = Create(domain);

        public void WriteByte(byte value) => _hash.AppendData([value]);

        public void WriteUInt32(uint value)
        {
            Span<byte> bytes = stackalloc byte[sizeof(uint)];
            BinaryPrimitives.WriteUInt32LittleEndian(bytes, value);
            _hash.AppendData(bytes);
        }

        public void WriteUInt64(ulong value)
        {
            Span<byte> bytes = stackalloc byte[sizeof(ulong)];
            BinaryPrimitives.WriteUInt64LittleEndian(bytes, value);
            _hash.AppendData(bytes);
        }

        public void WriteString(string value)
        {
            var bytes = Encoding.UTF8.GetBytes(value);
            WriteUInt32(checked((uint)bytes.Length));
            _hash.AppendData(bytes);
        }

        public void WriteHash(string value)
        {
            ValidateHash(value, "canonical hash input");
            _hash.AppendData(Convert.FromHexString(value));
        }

        public void WriteBytes(ReadOnlySpan<byte> bytes) => _hash.AppendData(bytes);

        public string Finish() => Convert.ToHexStringLower(_hash.GetHashAndReset());

        public void Dispose() => _hash.Dispose();

        private static IncrementalHash Create(ReadOnlySpan<byte> domain)
        {
            var hash = IncrementalHash.CreateHash(HashAlgorithmName.SHA256);
            hash.AppendData(domain);
            return hash;
        }
    }
}
