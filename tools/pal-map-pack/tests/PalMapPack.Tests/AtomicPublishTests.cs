using PalMapPack;
using System.Runtime.InteropServices;

namespace PalMapPack.Tests;

public sealed class AtomicPublishTests
{
    [Fact]
    public void FailedValidationNeverReplacesExistingValidatedPack()
    {
        var root = Fixture.TempDirectory();
        var target = Path.Combine(root, Fixture.Build);
        var oldStaging = AtomicPublisher.CreateStagingDirectory(root, Fixture.Build);
        File.WriteAllText(Path.Combine(oldStaging, "manifest.json"), "old-valid");
        AtomicPublisher.Publish(oldStaging, target, _ => true);
        var staging = AtomicPublisher.CreateStagingDirectory(root, Fixture.Build);
        File.WriteAllText(Path.Combine(staging, "manifest.json"), "new");

        Assert.Throws<MapPackFailure>(() =>
            AtomicPublisher.Publish(staging, target, _ => false));

        var active = AtomicPublisher.ResolveActivePath(root, Fixture.Build);
        Assert.Equal("old-valid", File.ReadAllText(Path.Combine(active, "manifest.json")));
    }

    [Fact]
    public void PublishWritesManifestLastAndReplacesValidatedPack()
    {
        var root = Fixture.TempDirectory();
        var target = Path.Combine(root, Fixture.Build);
        var staging = AtomicPublisher.CreateStagingDirectory(root, Fixture.Build);
        File.WriteAllText(Path.Combine(staging, "transform.json"), "{}");
        File.WriteAllText(Path.Combine(staging, "manifest.json"), "new");

        AtomicPublisher.Publish(staging, target, path =>
            File.Exists(Path.Combine(path, "manifest.json")));

        var active = AtomicPublisher.ResolveActivePath(root, Fixture.Build);
        Assert.Equal("new", File.ReadAllText(Path.Combine(active, "manifest.json")));
        Assert.False(Directory.Exists(target));
        Assert.True(File.Exists(Path.Combine(root, $"{Fixture.Build}.active.json")));
        Assert.Empty(Directory.EnumerateDirectories(root, "*.backup-*"));
    }

    [Fact]
    public void StagingAndTargetMustRemainUnderDatasetRoot()
    {
        var root = Fixture.TempDirectory();
        var outside = Fixture.TempDirectory();
        var staging = AtomicPublisher.CreateStagingDirectory(root, Fixture.Build);
        Assert.Throws<MapPackFailure>(() =>
            AtomicPublisher.Publish(staging, Path.Combine(outside, Fixture.Build), _ => true));
    }

    [Fact]
    public void DatasetRootRejectsAReparseAncestor()
    {
        var parent = Fixture.TempDirectory();
        var outside = Fixture.TempDirectory();
        var linkedRoot = Path.Combine(parent, "datasets");
        try
        {
            Directory.CreateSymbolicLink(linkedRoot, outside);
        }
        catch (Exception error) when (error is IOException or UnauthorizedAccessException)
        {
            return;
        }

        var failure = Assert.Throws<MapPackFailure>(() =>
            AtomicPublisher.CreateStagingDirectory(linkedRoot, Fixture.Build));

        Assert.Equal(ExitCodes.PackIntegrity, failure.ExitCode);
    }

    [Fact]
    public void ActivePointerRejectsHardLinkAliases()
    {
        var root = Fixture.TempDirectory();
        var target = Path.Combine(root, Fixture.Build);
        var staging = AtomicPublisher.CreateStagingDirectory(root, Fixture.Build);
        File.WriteAllText(Path.Combine(staging, "manifest.json"), "valid");
        AtomicPublisher.Publish(staging, target, _ => true);
        var pointer = Path.Combine(root, $"{Fixture.Build}.active.json");
        Assert.True(CreateHardLink(
            Path.Combine(root, "pointer-alias.json"),
            pointer,
            IntPtr.Zero));

        var failure = Assert.Throws<MapPackFailure>(() =>
            AtomicPublisher.ResolveActivePath(root, Fixture.Build));

        Assert.Equal(ExitCodes.PackIntegrity, failure.ExitCode);
    }

    [Theory]
    [InlineData(PublishCheckpoint.StagingValidated)]
    [InlineData(PublishCheckpoint.VersionInstalled)]
    [InlineData(PublishCheckpoint.BeforeAtomicActivation)]
    [InlineData(PublishCheckpoint.AfterAtomicActivation)]
    public void EveryPublishInterruptionPointLeavesAValidatedActivePack(
        PublishCheckpoint interruptedAt)
    {
        var root = Fixture.TempDirectory();
        var target = Path.Combine(root, Fixture.Build);
        var oldStaging = AtomicPublisher.CreateStagingDirectory(root, Fixture.Build);
        File.WriteAllText(Path.Combine(oldStaging, "manifest.json"), "old-valid");
        AtomicPublisher.Publish(oldStaging, target, path =>
            File.ReadAllText(Path.Combine(path, "manifest.json"))
                .EndsWith("-valid", StringComparison.Ordinal));
        var staging = AtomicPublisher.CreateStagingDirectory(root, Fixture.Build);
        File.WriteAllText(Path.Combine(staging, "manifest.json"), "new-valid");

        Assert.Throws<SimulatedInterruption>(() =>
            AtomicPublisher.Publish(
                staging,
                target,
                path => File.ReadAllText(Path.Combine(path, "manifest.json"))
                    .EndsWith("-valid", StringComparison.Ordinal),
                checkpoint =>
                {
                    if (checkpoint == interruptedAt)
                    {
                        throw new SimulatedInterruption();
                    }
                }));

        var activePath = AtomicPublisher.ResolveActivePath(root, Fixture.Build);
        var active = File.ReadAllText(Path.Combine(activePath, "manifest.json"));
        Assert.Contains(active, new[] { "old-valid", "new-valid" });
    }

    private sealed class SimulatedInterruption : Exception;

    [DllImport("kernel32.dll", EntryPoint = "CreateHardLinkW", SetLastError = true, CharSet = CharSet.Unicode)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool CreateHardLink(
        string fileName,
        string existingFileName,
        IntPtr securityAttributes);
}
