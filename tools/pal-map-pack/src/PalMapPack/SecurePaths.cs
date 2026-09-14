using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;

namespace PalMapPack;

internal sealed class SecureDirectoryLease : IDisposable
{
    private readonly List<SafeFileHandle> _handles;

    private SecureDirectoryLease(string fullPath, List<SafeFileHandle> handles)
    {
        FullPath = fullPath;
        _handles = handles;
    }

    public string FullPath { get; }

    public static SecureDirectoryLease OpenExisting(string path)
    {
        return OpenExisting(path, movable: false);
    }

    public static SecureDirectoryLease OpenMovable(string path)
    {
        return OpenExisting(path, movable: true);
    }

    private static SecureDirectoryLease OpenExisting(string path, bool movable)
    {
        if (!OperatingSystem.IsWindows())
        {
            throw Failure();
        }
        var full = Path.GetFullPath(path);
        var handles = new List<SafeFileHandle>();
        try
        {
            var handle = NativeMethods.Open(
                full,
                directory: true,
                readData: false,
                protectFromRename: true,
                deleteAccess: movable);
            NativeMethods.EnsurePlainUniquePath(handle, full, directory: true);
            handles.Add(handle);
            if (handles.Count == 0 || !Directory.Exists(full))
            {
                throw Failure();
            }
            return new SecureDirectoryLease(full, handles);
        }
        catch
        {
            handles.ForEach(handle => handle.Dispose());
            throw;
        }
    }

    public static SecureDirectoryLease Create(string path)
    {
        if (!OperatingSystem.IsWindows())
        {
            throw Failure();
        }
        var full = Path.GetFullPath(path);
        var parent = Directory.GetParent(full)?.FullName ?? throw Failure();
        using (OpenExisting(parent))
        {
            Directory.CreateDirectory(full);
        }
        return OpenExisting(full);
    }

    public void Dispose()
    {
        foreach (var handle in _handles)
        {
            handle.Dispose();
        }
        _handles.Clear();
    }

    public void MoveNoReplace(string target)
    {
        if (_handles.Count != 1)
        {
            throw Failure();
        }
        NativeMethods.MoveDirectoryNoReplace(_handles[0], FullPath, target);
    }

    private static MapPackFailure Failure() =>
        new(ExitCodes.PackIntegrity, "path is not a stable no-follow directory");
}

internal static class SecureFiles
{
    public static NativeMethods.FileIdentity IdentityWithin(string rootPath, string filePath)
    {
        using var root = SecureDirectoryLease.OpenExisting(rootPath);
        var fullPath = Path.GetFullPath(filePath);
        if (!SourceContainerSnapshot.IsWithin(root.FullPath, fullPath))
        {
            throw Failure();
        }
        var parentPath = Directory.GetParent(fullPath)?.FullName ?? throw Failure();
        using var parent = SecureDirectoryLease.OpenExisting(parentPath);
        using var handle = NativeMethods.Open(
            fullPath,
            directory: false,
            readData: false,
            protectFromRename: true);
        var identity = NativeMethods.EnsurePlainUniquePath(handle, fullPath, directory: false);
        if (!SourceContainerSnapshot.IsWithin(root.FullPath, identity.FinalPath)
            || identity.SizeBytes <= 0)
        {
            throw Failure();
        }
        return identity;
    }

    public static SecureFileSnapshot SnapshotWithin(string rootPath, string filePath)
    {
        using var root = SecureDirectoryLease.OpenExisting(rootPath);
        var fullPath = Path.GetFullPath(filePath);
        if (!SourceContainerSnapshot.IsWithin(root.FullPath, fullPath))
        {
            throw Failure();
        }
        var parentPath = Directory.GetParent(fullPath)?.FullName ?? throw Failure();
        using var parent = SecureDirectoryLease.OpenExisting(parentPath);
        using var handle = NativeMethods.Open(
            fullPath,
            directory: false,
            readData: true,
            protectFromRename: true);
        var identity = NativeMethods.EnsurePlainUniquePath(handle, fullPath, directory: false);
        if (!SourceContainerSnapshot.IsWithin(root.FullPath, identity.FinalPath))
        {
            throw Failure();
        }
        using var stream = new FileStream(handle, FileAccess.Read);
        if (identity.SizeBytes <= 0)
        {
            throw new MapPackFailure(
                ExitCodes.MountSerialization,
                "source file is missing or empty");
        }
        var digest = System.Security.Cryptography.SHA256.HashData(stream);
        return new SecureFileSnapshot(
            identity.FinalPath,
            identity.SizeBytes,
            identity.LastWriteUtcTicks,
            Convert.ToHexStringLower(digest),
            identity.VolumeSerial,
            identity.FileIndex);
    }

    public static byte[] ReadAllBytes(string path, int maximumBytes)
    {
        if (maximumBytes <= 0)
        {
            throw new ArgumentOutOfRangeException(nameof(maximumBytes));
        }
        var full = Path.GetFullPath(path);
        var parentPath = Directory.GetParent(full)?.FullName ?? throw Failure();
        using var parent = SecureDirectoryLease.OpenExisting(parentPath);
        using var handle = NativeMethods.Open(
            full,
            directory: false,
            readData: true,
            protectFromRename: true);
        var identity = NativeMethods.EnsurePlainUniquePath(handle, full, directory: false);
        if (identity.SizeBytes <= 0 || identity.SizeBytes > maximumBytes)
        {
            throw Failure();
        }
        using var stream = new FileStream(handle, FileAccess.Read);
        var bytes = new byte[checked((int)identity.SizeBytes)];
        stream.ReadExactly(bytes);
        return bytes;
    }

    public static void EnsureRegularFile(string path)
    {
        var full = Path.GetFullPath(path);
        var parentPath = Directory.GetParent(full)?.FullName ?? throw Failure();
        using var parent = SecureDirectoryLease.OpenExisting(parentPath);
        using var handle = NativeMethods.Open(
            full,
            directory: false,
            readData: true,
            protectFromRename: true);
        _ = NativeMethods.EnsurePlainUniquePath(handle, full, directory: false);
    }

    public static void WriteNewFile(string rootPath, string path, ReadOnlySpan<byte> bytes)
    {
        if (bytes.IsEmpty)
        {
            throw Failure();
        }
        using var root = SecureDirectoryLease.OpenExisting(rootPath);
        var full = Path.GetFullPath(path);
        if (!SourceContainerSnapshot.IsWithin(root.FullPath, full))
        {
            throw Failure();
        }
        var parentPath = Directory.GetParent(full)?.FullName ?? throw Failure();
        using var parent = SecureDirectoryLease.OpenExisting(parentPath);
        using var handle = NativeMethods.CreateNewFile(full);
        using (var stream = new FileStream(handle, FileAccess.Write, bufferSize: 4096, isAsync: false))
        {
            stream.Write(bytes);
            stream.Flush(flushToDisk: true);
            var identity = NativeMethods.EnsurePlainUniquePath(
                stream.SafeFileHandle,
                full,
                directory: false);
            if (identity.SizeBytes != bytes.Length
                || !SourceContainerSnapshot.IsWithin(root.FullPath, identity.FinalPath))
            {
                throw Failure();
            }
        }
    }

    private static MapPackFailure Failure() =>
        new(ExitCodes.PackIntegrity, "file path escaped its protected no-follow root");
}

internal sealed record SecureFileSnapshot(
    string FinalPath,
    long SizeBytes,
    long LastWriteUtcTicks,
    string Sha256,
    uint VolumeSerial,
    ulong FileIndex);

internal static class NativeMethods
{
    private const uint GenericRead = 0x80000000;
    private const uint GenericWrite = 0x40000000;
    private const uint FileReadAttributes = 0x80;
    private const uint DeleteAccess = 0x00010000;
    private const uint ShareRead = 0x1;
    private const uint ShareWrite = 0x2;
    private const uint ShareDelete = 0x4;
    private const uint OpenExisting = 3;
    private const uint CreateNew = 1;
    private const uint FileFlagBackupSemantics = 0x02000000;
    private const uint FileFlagOpenReparsePoint = 0x00200000;
    private const uint FileAttributeReparsePoint = 0x400;
    private const uint FileNameNormalized = 0;
    private const uint VolumeNameDos = 0;
    private const uint MoveFileWriteThrough = 0x8;
    private const uint ReplaceFileWriteThrough = 0x1;
    private const int FileDispositionInfoEx = 21;
    private const int FileRenameInfoEx = 22;
    private const uint FileDispositionDelete = 0x1;
    private const uint FileRenamePosixSemantics = 0x2;
    private const int ErrorFileNotFound = 2;
    private const int ErrorPathNotFound = 3;

    internal static SafeFileHandle Open(
        string path,
        bool directory,
        bool readData,
        bool protectFromRename,
        bool deleteAccess = false)
    {
        var handle = CreateFile(
            ToExtendedPath(path),
            (readData ? GenericRead : 0)
                | (deleteAccess ? DeleteAccess : 0)
                | FileReadAttributes,
            ShareRead | ShareWrite | (protectFromRename ? 0 : ShareDelete),
            IntPtr.Zero,
            OpenExisting,
            FileFlagOpenReparsePoint | (directory ? FileFlagBackupSemantics : 0),
            IntPtr.Zero);
        if (handle.IsInvalid)
        {
            var error = Marshal.GetLastWin32Error();
            handle.Dispose();
            throw new MapPackFailure(
                ExitCodes.PackIntegrity,
                $"Windows no-follow path open failed ({error})");
        }
        return handle;
    }

    internal static SafeFileHandle CreateNewFile(string path)
    {
        var handle = CreateFile(
            ToExtendedPath(path),
            GenericWrite | FileReadAttributes,
            ShareRead,
            IntPtr.Zero,
            CreateNew,
            FileFlagOpenReparsePoint,
            IntPtr.Zero);
        if (handle.IsInvalid)
        {
            handle.Dispose();
            throw new MapPackFailure(
                ExitCodes.PackIntegrity,
                "protected output file creation failed");
        }
        return handle;
    }

    internal static void EnsurePathEntryAbsentNoFollow(string path)
    {
        var handle = CreateFile(
            ToExtendedPath(path),
            FileReadAttributes,
            ShareRead | ShareWrite | ShareDelete,
            IntPtr.Zero,
            OpenExisting,
            FileFlagOpenReparsePoint | FileFlagBackupSemantics,
            IntPtr.Zero);
        if (handle.IsInvalid)
        {
            var error = Marshal.GetLastWin32Error();
            handle.Dispose();
            if (error is ErrorFileNotFound or ErrorPathNotFound)
            {
                return;
            }

            throw new MapPackFailure(
                ExitCodes.PackIntegrity,
                $"Windows no-follow residue probe failed ({error})");
        }

        handle.Dispose();
        throw new IOException(
            "protected owner-only cleanup found an unexpected filesystem entry");
    }

    internal static FileIdentity EnsurePlainUniquePath(
        SafeFileHandle handle,
        string expectedPath,
        bool directory)
    {
        if (!GetFileInformationByHandle(handle, out var information)
            || (!directory && information.NumberOfLinks != 1)
            || (information.FileAttributes & FileAttributeReparsePoint) != 0)
        {
            throw Failure();
        }
        var finalPath = GetFinalPath(handle);
        if (!string.Equals(
                Normalize(finalPath),
                Normalize(Path.GetFullPath(expectedPath)),
                StringComparison.OrdinalIgnoreCase))
        {
            throw Failure();
        }
        var fileIndex = ((ulong)information.FileIndexHigh << 32) | information.FileIndexLow;
        var fileSize = ((long)information.FileSizeHigh << 32) | information.FileSizeLow;
        var fileTime = ((long)(uint)information.LastWriteTime.dwHighDateTime << 32)
            | (uint)information.LastWriteTime.dwLowDateTime;
        return new FileIdentity(
            Normalize(finalPath),
            information.VolumeSerialNumber,
            fileIndex,
            fileSize,
            DateTime.FromFileTimeUtc(fileTime).Ticks);
    }

    internal static void MoveDirectoryNoReplace(
        SafeFileHandle sourceHandle,
        string source,
        string target)
    {
        _ = EnsurePlainUniquePath(sourceHandle, source, directory: true);
        if (Directory.Exists(target) || File.Exists(target))
        {
            throw new MapPackFailure(
                ExitCodes.PackIntegrity,
                "immutable version target already exists");
        }
        RenameByHandle(
            sourceHandle,
            target,
            FileRenamePosixSemantics,
            "same-volume handle-based directory move failed");
        _ = EnsurePlainUniquePath(sourceHandle, target, directory: true);
    }

    internal static void MoveFileNoReplace(
        SafeFileHandle sourceHandle,
        string source,
        string target,
        Action? beforeRename = null)
    {
        var sourceParent = Directory.GetParent(Path.GetFullPath(source))?.FullName;
        var targetParent = Directory.GetParent(Path.GetFullPath(target))?.FullName;
        if (sourceParent is null
            || !string.Equals(sourceParent, targetParent, StringComparison.OrdinalIgnoreCase))
        {
            throw new MapPackFailure(
                ExitCodes.PackIntegrity,
                "protected output move must stay in one directory");
        }

        _ = EnsurePlainUniquePath(sourceHandle, source, directory: false);
        if (File.Exists(target) || Directory.Exists(target))
        {
            throw new IOException("protected output target already exists");
        }

        beforeRename?.Invoke();
        RenameByHandle(
            sourceHandle,
            target,
            flags: 0,
            "same-volume handle-based file move failed");
        _ = EnsurePlainUniquePath(sourceHandle, target, directory: false);
    }

    internal static void MarkFileForDeletion(SafeFileHandle file)
    {
        var buffer = Marshal.AllocHGlobal(sizeof(uint));
        try
        {
            Marshal.WriteInt32(buffer, unchecked((int)FileDispositionDelete));
            if (!SetFileInformationByHandle(
                    file,
                    FileDispositionInfoEx,
                    buffer,
                    sizeof(uint)))
            {
                throw new MapPackFailure(
                    ExitCodes.PackIntegrity,
                    "handle-based protected output cleanup failed");
            }
        }
        finally
        {
            Marshal.FreeHGlobal(buffer);
        }
    }

    internal static string ResolveOwnedFilePath(
        SafeFileHandle file,
        string firstCandidate,
        string secondCandidate)
    {
        var actual = Normalize(GetFinalPath(file));
        foreach (var candidate in new[] { firstCandidate, secondCandidate })
        {
            var fullCandidate = Path.GetFullPath(candidate);
            if (!string.Equals(
                    actual,
                    Normalize(fullCandidate),
                    StringComparison.OrdinalIgnoreCase))
            {
                continue;
            }

            _ = EnsurePlainUniquePath(file, fullCandidate, directory: false);
            return fullCandidate;
        }

        throw Failure();
    }

    internal static void AtomicReplaceFile(string source, string target)
    {
        var sourceParent = Directory.GetParent(Path.GetFullPath(source))?.FullName;
        var targetParent = Directory.GetParent(Path.GetFullPath(target))?.FullName;
        if (sourceParent is null
            || !string.Equals(sourceParent, targetParent, StringComparison.OrdinalIgnoreCase))
        {
            throw new MapPackFailure(
                ExitCodes.PackIntegrity,
                "active pointer replacement must stay in one protected directory");
        }
        SecureFiles.EnsureRegularFile(source);
        bool replaced;
        if (File.Exists(target))
        {
            SecureFiles.EnsureRegularFile(target);
            replaced = ReplaceFile(
                ToExtendedPath(target),
                ToExtendedPath(source),
                null,
                ReplaceFileWriteThrough,
                IntPtr.Zero,
                IntPtr.Zero);
        }
        else
        {
            replaced = MoveFileEx(
                ToExtendedPath(source),
                ToExtendedPath(target),
                MoveFileWriteThrough);
        }
        if (!replaced)
        {
            throw new MapPackFailure(
                ExitCodes.PackIntegrity,
                "same-volume active pointer replacement failed");
        }
        SecureFiles.EnsureRegularFile(target);
    }

    private static void RenameByHandle(
        SafeFileHandle sourceHandle,
        string target,
        uint flags,
        string failureMessage)
    {
        var targetBytes = System.Text.Encoding.Unicode.GetBytes(ToExtendedPath(target));
        var rootOffset = IntPtr.Size == 8 ? 8 : 4;
        var lengthOffset = rootOffset + IntPtr.Size;
        var nameOffset = lengthOffset + sizeof(uint);
        var headerSize = IntPtr.Size == 8 ? 24 : 16;
        var bufferSize = checked(headerSize + targetBytes.Length);
        var buffer = Marshal.AllocHGlobal(bufferSize);
        try
        {
            Marshal.Copy(new byte[bufferSize], 0, buffer, bufferSize);
            Marshal.WriteInt32(buffer, unchecked((int)flags));
            Marshal.WriteIntPtr(buffer, rootOffset, IntPtr.Zero);
            Marshal.WriteInt32(buffer, lengthOffset, targetBytes.Length);
            Marshal.Copy(targetBytes, 0, buffer + nameOffset, targetBytes.Length);
            if (!SetFileInformationByHandle(
                    sourceHandle,
                    FileRenameInfoEx,
                    buffer,
                    checked((uint)bufferSize)))
            {
                throw new MapPackFailure(
                    ExitCodes.PackIntegrity,
                    failureMessage);
            }
        }
        finally
        {
            Marshal.FreeHGlobal(buffer);
        }
    }

    private static string GetFinalPath(SafeFileHandle handle)
    {
        var capacity = 512;
        while (capacity <= 32_768)
        {
            var buffer = new char[capacity];
            var length = GetFinalPathNameByHandle(
                handle,
                buffer,
                checked((uint)buffer.Length),
                FileNameNormalized | VolumeNameDos);
            if (length == 0)
            {
                throw Failure();
            }
            if (length < buffer.Length)
            {
                return new string(buffer, 0, checked((int)length));
            }
            capacity = checked((int)length + 1);
        }
        throw Failure();
    }

    private static string Normalize(string path)
    {
        const string extendedUnc = @"\\?\UNC\";
        const string extended = @"\\?\";
        if (path.StartsWith(extendedUnc, StringComparison.OrdinalIgnoreCase))
        {
            path = @"\\" + path[extendedUnc.Length..];
        }
        else if (path.StartsWith(extended, StringComparison.OrdinalIgnoreCase))
        {
            path = path[extended.Length..];
        }
        return Path.GetFullPath(path).TrimEnd(Path.DirectorySeparatorChar);
    }

    private static string ToExtendedPath(string path)
    {
        var full = Path.GetFullPath(path);
        if (full.StartsWith(@"\\?\", StringComparison.Ordinal))
        {
            return full;
        }
        if (full.StartsWith(@"\\", StringComparison.Ordinal))
        {
            return @"\\?\UNC\" + full[2..];
        }
        return @"\\?\" + full;
    }

    private static MapPackFailure Failure() =>
        new(ExitCodes.PackIntegrity, "Windows no-follow path validation failed");

    [DllImport("kernel32.dll", EntryPoint = "CreateFileW", SetLastError = true, CharSet = CharSet.Unicode)]
    private static extern SafeFileHandle CreateFile(
        string fileName,
        uint desiredAccess,
        uint shareMode,
        IntPtr securityAttributes,
        uint creationDisposition,
        uint flagsAndAttributes,
        IntPtr templateFile);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool GetFileInformationByHandle(
        SafeFileHandle file,
        out ByHandleFileInformation information);

    [DllImport("kernel32.dll", EntryPoint = "GetFinalPathNameByHandleW", SetLastError = true, CharSet = CharSet.Unicode)]
    private static extern uint GetFinalPathNameByHandle(
        SafeFileHandle file,
        [Out] char[] filePath,
        uint filePathLength,
        uint flags);

    [DllImport("kernel32.dll", EntryPoint = "MoveFileExW", SetLastError = true, CharSet = CharSet.Unicode)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool MoveFileEx(
        string existingFileName,
        string newFileName,
        uint flags);

    [DllImport("kernel32.dll", EntryPoint = "ReplaceFileW", SetLastError = true, CharSet = CharSet.Unicode)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool ReplaceFile(
        string replacedFileName,
        string replacementFileName,
        string? backupFileName,
        uint replaceFlags,
        IntPtr exclude,
        IntPtr reserved);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool SetFileInformationByHandle(
        SafeFileHandle file,
        int fileInformationClass,
        IntPtr fileInformation,
        uint bufferSize);

    [StructLayout(LayoutKind.Sequential)]
    private struct ByHandleFileInformation
    {
        public uint FileAttributes;
        public System.Runtime.InteropServices.ComTypes.FILETIME CreationTime;
        public System.Runtime.InteropServices.ComTypes.FILETIME LastAccessTime;
        public System.Runtime.InteropServices.ComTypes.FILETIME LastWriteTime;
        public uint VolumeSerialNumber;
        public uint FileSizeHigh;
        public uint FileSizeLow;
        public uint NumberOfLinks;
        public uint FileIndexHigh;
        public uint FileIndexLow;
    }

    internal sealed record FileIdentity(
        string FinalPath,
        uint VolumeSerial,
        ulong FileIndex,
        long SizeBytes,
        long LastWriteUtcTicks);
}
