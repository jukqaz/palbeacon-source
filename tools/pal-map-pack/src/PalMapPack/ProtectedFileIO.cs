using System.Runtime.CompilerServices;
using System.Security.AccessControl;
using System.Security.Principal;

[assembly: InternalsVisibleTo("PalMapPack.Tests")]

namespace PalMapPack;

/// <summary>
/// Narrow public boundary for local tools that need the map packer's Windows
/// no-follow file guarantees without exposing its handle primitives.
/// </summary>
public static class ProtectedFileIO
{
    public static byte[] ReadAllBytes(string path, int maximumBytes) =>
        SecureFiles.ReadAllBytes(path, maximumBytes);

    public static void WriteNewAtomic(string path, ReadOnlySpan<byte> bytes)
    {
        ArgumentNullException.ThrowIfNull(path);
        if (bytes.IsEmpty)
        {
            throw new IOException("protected output cannot be empty");
        }

        var destination = Path.GetFullPath(path);
        var parent = Directory.GetParent(destination)?.FullName
            ?? throw new IOException("protected output requires an existing parent directory");
        if (!Directory.Exists(parent))
        {
            throw new DirectoryNotFoundException(parent);
        }

        using var parentLease = SecureDirectoryLease.OpenExisting(parent);
        if (File.Exists(destination) || Directory.Exists(destination))
        {
            throw new IOException("protected output already exists and will not be replaced");
        }

        var temporary = Path.Combine(
            parent,
            $".{Path.GetFileName(destination)}.{Guid.NewGuid():N}.tmp");
        try
        {
            SecureFiles.WriteNewFile(parent, temporary, bytes);
            SecureFiles.EnsureRegularFile(temporary);
            if (File.Exists(destination) || Directory.Exists(destination))
            {
                throw new IOException("protected output appeared before publication");
            }
            File.Move(temporary, destination);
            SecureFiles.EnsureRegularFile(destination);
        }
        finally
        {
            if (File.Exists(temporary))
            {
                SecureFiles.EnsureRegularFile(temporary);
                File.Delete(temporary);
            }
        }
    }

    public static void WriteNewOwnerOnlyAtomic(string path, ReadOnlySpan<byte> bytes) =>
        WriteNewOwnerOnlyAtomicCore(
            path,
            bytes,
            afterTemporaryIdentityValidated: null,
            beforeAclValidation: null,
            beforeHandleRename: null);

    internal static void WriteNewOwnerOnlyAtomicCore(
        string path,
        ReadOnlySpan<byte> bytes,
        Action<string>? afterTemporaryIdentityValidated,
        Action<bool>? beforeAclValidation,
        Action? beforeHandleRename)
    {
        ArgumentNullException.ThrowIfNull(path);
        if (bytes.IsEmpty)
        {
            throw new IOException("protected owner-only output cannot be empty");
        }

        var destination = Path.GetFullPath(path);
        var parent = Directory.GetParent(destination)?.FullName
            ?? throw new IOException(
                "protected owner-only output requires an existing parent directory");
        if (!Directory.Exists(parent))
        {
            throw new DirectoryNotFoundException(parent);
        }

        using var parentLease = SecureDirectoryLease.OpenExisting(parent);
        if (File.Exists(destination) || Directory.Exists(destination))
        {
            throw new IOException(
                "protected owner-only output already exists and will not be replaced");
        }

        var temporary = Path.Combine(
            parent,
            $".{Path.GetFileName(destination)}.{Guid.NewGuid():N}.tmp");
        FileStream? file = null;
        try
        {
            var currentUser = CurrentTokenUser();
            var ownerOnlyAcl = BuildOwnerOnlyAcl(currentUser);
            file = new FileInfo(temporary).Create(
                FileMode.CreateNew,
                FileSystemRights.FullControl,
                FileShare.None,
                bufferSize: 4096,
                FileOptions.WriteThrough,
                ownerOnlyAcl);
            file.Write(bytes);
            file.Flush(flushToDisk: true);

            var createdIdentity = NativeMethods.EnsurePlainUniquePath(
                file.SafeFileHandle,
                temporary,
                directory: false);
            if (createdIdentity.SizeBytes != bytes.Length
                || !SourceContainerSnapshot.IsWithin(parentLease.FullPath, createdIdentity.FinalPath))
            {
                throw new IOException(
                    "protected owner-only output identity changed during creation");
            }

            afterTemporaryIdentityValidated?.Invoke(temporary);
            var identityBeforeAcl = NativeMethods.EnsurePlainUniquePath(
                file.SafeFileHandle,
                temporary,
                directory: false);
            EnsureSameFileIdentity(createdIdentity, identityBeforeAcl, bytes.Length);

            file.SetAccessControl(ownerOnlyAcl);
            beforeAclValidation?.Invoke(false);
            ValidateOwnerOnlyAcl(file, currentUser);

            if (File.Exists(destination) || Directory.Exists(destination))
            {
                throw new IOException(
                    "protected owner-only output appeared before publication");
            }

            NativeMethods.MoveFileNoReplace(
                file.SafeFileHandle,
                temporary,
                destination,
                beforeHandleRename);
            var publishedIdentity = NativeMethods.EnsurePlainUniquePath(
                file.SafeFileHandle,
                destination,
                directory: false);
            EnsureSameFileIdentity(createdIdentity, publishedIdentity, bytes.Length);
            beforeAclValidation?.Invoke(true);
            ValidateOwnerOnlyAcl(file, currentUser);

            file.Dispose();
            file = null;
        }
        catch (Exception publicationError)
        {
            var cleanupErrors = new List<Exception>();
            string? ownedPath = null;
            if (file is not null)
            {
                try
                {
                    ownedPath = NativeMethods.ResolveOwnedFilePath(
                        file.SafeFileHandle,
                        temporary,
                        destination);
                }
                catch (Exception cleanupError)
                {
                    cleanupErrors.Add(cleanupError);
                }

                try
                {
                    NativeMethods.MarkFileForDeletion(file.SafeFileHandle);
                }
                catch (Exception cleanupError)
                {
                    cleanupErrors.Add(cleanupError);
                }

                try
                {
                    file.Dispose();
                }
                catch (Exception cleanupError)
                {
                    cleanupErrors.Add(cleanupError);
                }
                file = null;
            }

            try
            {
                if (ownedPath is not null)
                {
                    EnsureNoCreatedPathResidue(ownedPath);
                }
                else
                {
                    EnsureNoCreatedPathResidue(temporary);
                    EnsureNoCreatedPathResidue(destination);
                }
            }
            catch (Exception cleanupError)
            {
                cleanupErrors.Add(cleanupError);
            }

            if (cleanupErrors.Count != 0)
            {
                cleanupErrors.Insert(0, publicationError);
                throw new AggregateException(
                    "protected owner-only output publication and cleanup failed",
                    cleanupErrors);
            }

            throw;
        }
        finally
        {
            file?.Dispose();
        }
    }

    internal static void EnsureNoCreatedPathResidue(string path)
    {
        NativeMethods.EnsurePathEntryAbsentNoFollow(path);
    }

    private static FileSecurity BuildOwnerOnlyAcl(SecurityIdentifier currentUser)
    {
        var ownerOnlyAcl = new FileSecurity();
        ownerOnlyAcl.SetOwner(currentUser);
        ownerOnlyAcl.SetAccessRuleProtection(isProtected: true, preserveInheritance: false);
        ownerOnlyAcl.AddAccessRule(
            new FileSystemAccessRule(
                currentUser,
                FileSystemRights.FullControl,
                AccessControlType.Allow));
        return ownerOnlyAcl;
    }

    private static void ValidateOwnerOnlyAcl(
        FileStream file,
        SecurityIdentifier currentUser)
    {
        var security = file.GetAccessControl();
        var rules = security.GetAccessRules(
                includeExplicit: true,
                includeInherited: true,
                typeof(SecurityIdentifier))
            .Cast<FileSystemAccessRule>()
            .ToArray();

        if (!security.AreAccessRulesProtected
            || !currentUser.Equals(security.GetOwner(typeof(SecurityIdentifier)))
            || rules.Length != 1)
        {
            throw new IOException(
                "protected owner-only output security descriptor is not exclusive");
        }

        var rule = rules[0];
        if (!currentUser.Equals(rule.IdentityReference)
            || rule.AccessControlType != AccessControlType.Allow
            || rule.FileSystemRights != FileSystemRights.FullControl
            || rule.IsInherited)
        {
            throw new IOException(
                "protected owner-only output contains an unexpected access rule");
        }
    }

    private static SecurityIdentifier CurrentTokenUser()
    {
        using var identity = WindowsIdentity.GetCurrent(TokenAccessLevels.Query);
        return identity.User
            ?? throw new IOException("current Windows token has no user SID");
    }

    private static void EnsureSameFileIdentity(
        NativeMethods.FileIdentity expected,
        NativeMethods.FileIdentity actual,
        int expectedSize)
    {
        if (actual.VolumeSerial != expected.VolumeSerial
            || actual.FileIndex != expected.FileIndex
            || actual.SizeBytes != expectedSize)
        {
            throw new IOException(
                "protected owner-only output object identity changed during publication");
        }
    }
}
