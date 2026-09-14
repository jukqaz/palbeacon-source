using System.Security.AccessControl;
using System.Security.Principal;
using System.Runtime.InteropServices;

using PalMapPack;
using Xunit.Sdk;

namespace PalMapPack.Tests;

public sealed class ProtectedFileIOTests
{
    [Fact]
    public void WriteNewOwnerOnlyAtomicWritesBytesWithExactlyOneOwnerAllowAce()
    {
        if (!OperatingSystem.IsWindows())
        {
            return;
        }

        using var directory = PrivateTemporaryDirectory.Create();
        var path = Path.Combine(directory.Path, "observation.json");

        ProtectedFileIO.WriteNewOwnerOnlyAtomic(path, "one"u8);

        Assert.Equal("one", File.ReadAllText(path));
        AssertOwnerOnly(path);
    }

    [Fact]
    public void WriteNewOwnerOnlyAtomicNeverOverwritesAnExistingOutput()
    {
        if (!OperatingSystem.IsWindows())
        {
            return;
        }

        using var directory = PrivateTemporaryDirectory.Create();
        var path = Path.Combine(directory.Path, "observation.json");
        ProtectedFileIO.WriteNewOwnerOnlyAtomic(path, "one"u8);

        Assert.Throws<IOException>(() =>
            ProtectedFileIO.WriteNewOwnerOnlyAtomic(path, "two"u8));

        Assert.Equal("one", File.ReadAllText(path));
        AssertOwnerOnly(path);
    }

    [Fact]
    public void FinalAclValidationFailureRemovesFinalAndTemporaryOutputs()
    {
        if (!OperatingSystem.IsWindows())
        {
            return;
        }

        using var directory = PrivateTemporaryDirectory.Create();
        var path = Path.Combine(directory.Path, "observation.json");
        var validationCalls = 0;

        Assert.Throws<IOException>(() =>
            ProtectedFileIO.WriteNewOwnerOnlyAtomicCore(
                path,
                "one"u8,
                afterTemporaryIdentityValidated: null,
                beforeAclValidation: finalValidation =>
                {
                    validationCalls++;
                    if (finalValidation)
                    {
                        throw new IOException("simulated final ACL publication failure");
                    }
                },
                beforeHandleRename: null));

        Assert.Equal(2, validationCalls);
        Assert.False(File.Exists(path));
        Assert.Empty(Directory.EnumerateFileSystemEntries(directory.Path));
    }

    [Fact]
    public void RetainedCreateHandlePreventsTemporaryObjectReplacement()
    {
        if (!OperatingSystem.IsWindows())
        {
            return;
        }

        using var directory = PrivateTemporaryDirectory.Create();
        var path = Path.Combine(directory.Path, "observation.json");
        var replacementAttempted = false;

        ProtectedFileIO.WriteNewOwnerOnlyAtomicCore(
            path,
            "one"u8,
            temporary =>
            {
                var attacker = Path.Combine(directory.Path, "attacker.json");
                File.WriteAllText(attacker, "attacker");
                try
                {
                    replacementAttempted = true;
                    var replacementError = Record.Exception(() =>
                        File.Move(attacker, temporary, overwrite: true));
                    Assert.True(
                        replacementError is IOException or UnauthorizedAccessException,
                        $"expected a sharing denial, got {replacementError?.GetType().Name ?? "none"}");
                }
                finally
                {
                    if (File.Exists(attacker))
                    {
                        File.Delete(attacker);
                    }
                }
            },
            beforeAclValidation: null,
            beforeHandleRename: null);

        Assert.True(replacementAttempted);
        Assert.Equal("one", File.ReadAllText(path));
        AssertOwnerOnly(path);
    }

    [Fact]
    public void NativeNoReplaceRenamePreservesAConcurrentDestination()
    {
        if (!OperatingSystem.IsWindows())
        {
            return;
        }

        using var directory = PrivateTemporaryDirectory.Create();
        var path = Path.Combine(directory.Path, "observation.json");

        Assert.Throws<MapPackFailure>(() =>
            ProtectedFileIO.WriteNewOwnerOnlyAtomicCore(
                path,
                "one"u8,
                afterTemporaryIdentityValidated: null,
                beforeAclValidation: null,
                beforeHandleRename: () => File.WriteAllText(path, "concurrent")));

        Assert.Equal("concurrent", File.ReadAllText(path));
        Assert.DoesNotContain(
            Directory.EnumerateFileSystemEntries(directory.Path),
            candidate => Path.GetFileName(candidate).EndsWith(".tmp", StringComparison.Ordinal));
    }

    [Fact]
    public void DirectoryResidueIsReportedWithoutDeletingUntrustedContents()
    {
        if (!OperatingSystem.IsWindows())
        {
            return;
        }

        using var directory = PrivateTemporaryDirectory.Create();
        var residue = Path.Combine(directory.Path, "unexpected-residue");
        Directory.CreateDirectory(residue);
        var sentinel = Path.Combine(residue, "sentinel.txt");
        File.WriteAllText(sentinel, "do-not-delete");

        Assert.Throws<IOException>(() =>
            ProtectedFileIO.EnsureNoCreatedPathResidue(residue));

        Assert.True(Directory.Exists(residue));
        Assert.Equal("do-not-delete", File.ReadAllText(sentinel));
    }

    [Fact]
    public void AbsentPathIsAcceptedByNoFollowResidueProbe()
    {
        if (!OperatingSystem.IsWindows())
        {
            return;
        }

        using var directory = PrivateTemporaryDirectory.Create();
        var absent = Path.Combine(directory.Path, "absent.json");

        NativeMethods.EnsurePathEntryAbsentNoFollow(absent);
    }

    [Fact]
    public void DanglingFileSymlinkIsReportedAsResidue()
    {
        if (!OperatingSystem.IsWindows())
        {
            return;
        }

        using var directory = PrivateTemporaryDirectory.Create();
        var missingTarget = Path.Combine(directory.Path, "missing-target.json");
        var danglingLink = Path.Combine(directory.Path, "dangling.json");
        try
        {
            File.CreateSymbolicLink(danglingLink, missingTarget);
        }
        catch (Exception error)
            when (error is UnauthorizedAccessException
                or IOException
                or PlatformNotSupportedException)
        {
            throw SkipException.ForSkip(
                $"Windows symlink creation is unavailable: {error.GetType().Name}");
        }

        Assert.Throws<IOException>(() =>
            NativeMethods.EnsurePathEntryAbsentNoFollow(danglingLink));
    }

    [Fact]
    public void OwnerOnlyOutputRejectsASecondHardlinkIdentity()
    {
        if (!OperatingSystem.IsWindows())
        {
            return;
        }

        using var directory = PrivateTemporaryDirectory.Create();
        var path = Path.Combine(directory.Path, "observation.json");
        var alias = Path.Combine(directory.Path, "observation-alias.json");
        ProtectedFileIO.WriteNewOwnerOnlyAtomic(path, "one"u8);
        Assert.True(CreateHardLink(alias, path, IntPtr.Zero));

        Assert.Throws<MapPackFailure>(() =>
            SecureFiles.EnsureRegularFile(path));
    }

    private static void AssertOwnerOnly(string path)
    {
        using var identity = WindowsIdentity.GetCurrent();
        var currentUser = identity.User
            ?? throw new InvalidOperationException("current token has no user SID");
        var security = new FileInfo(path).GetAccessControl(
            AccessControlSections.Owner | AccessControlSections.Access);
        var rules = security.GetAccessRules(
                includeExplicit: true,
                includeInherited: true,
                typeof(SecurityIdentifier))
            .Cast<FileSystemAccessRule>()
            .ToArray();

        Assert.True(security.AreAccessRulesProtected);
        Assert.Equal(currentUser, security.GetOwner(typeof(SecurityIdentifier)));
        var rule = Assert.Single(rules);
        Assert.Equal(currentUser, rule.IdentityReference);
        Assert.Equal(AccessControlType.Allow, rule.AccessControlType);
        Assert.Equal(FileSystemRights.FullControl, rule.FileSystemRights);
        Assert.False(rule.IsInherited);

        foreach (var forbidden in ForbiddenSids())
        {
            Assert.DoesNotContain(
                rules,
                candidate => forbidden.Equals(candidate.IdentityReference));
        }
    }

    private static IEnumerable<SecurityIdentifier> ForbiddenSids()
    {
        yield return new SecurityIdentifier(WellKnownSidType.WorldSid, null);
        yield return new SecurityIdentifier(WellKnownSidType.BuiltinUsersSid, null);
        yield return new SecurityIdentifier(WellKnownSidType.BuiltinAdministratorsSid, null);
        yield return new SecurityIdentifier(WellKnownSidType.LocalSystemSid, null);
        yield return new SecurityIdentifier(WellKnownSidType.AnonymousSid, null);
    }

    private sealed class PrivateTemporaryDirectory : IDisposable
    {
        private PrivateTemporaryDirectory(string path)
        {
            Path = path;
        }

        public string Path { get; }

        public static PrivateTemporaryDirectory Create()
        {
            var path = System.IO.Path.Combine(
                System.IO.Path.GetTempPath(),
                $"pal-map-pack-owner-only-{Guid.NewGuid():N}");
            Directory.CreateDirectory(path);

            using var identity = WindowsIdentity.GetCurrent();
            var currentUser = identity.User
                ?? throw new InvalidOperationException("current token has no user SID");
            var security = new DirectorySecurity();
            security.SetAccessRuleProtection(isProtected: true, preserveInheritance: false);
            security.AddAccessRule(
                new FileSystemAccessRule(
                    currentUser,
                    FileSystemRights.FullControl,
                    InheritanceFlags.ContainerInherit | InheritanceFlags.ObjectInherit,
                    PropagationFlags.None,
                    AccessControlType.Allow));
            new DirectoryInfo(path).SetAccessControl(security);

            return new PrivateTemporaryDirectory(path);
        }

        public void Dispose()
        {
            if (Directory.Exists(Path))
            {
                Directory.Delete(Path, recursive: true);
            }
        }
    }

    [DllImport(
        "kernel32.dll",
        EntryPoint = "CreateHardLinkW",
        SetLastError = true,
        CharSet = CharSet.Unicode)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool CreateHardLink(
        string fileName,
        string existingFileName,
        IntPtr securityAttributes);
}
