using System.Drawing;
using System.Reflection;
using System.Security.AccessControl;
using System.Security.Cryptography;
using System.Security.Principal;
using System.Text;
using System.Text.Json;
using System.Globalization;
using PalMapPack.Calibrator;

namespace PalMapPack.Tests;

public sealed class LocalAlignmentObservationExporterTests
{
    private const string ExactBuild = "24181527";
    private const string ExactMapSha =
        "aa81bdddbc525898b0a70b238cc936d0f8b0416c4ead922797b066d871938d8b";

    [Fact]
    public void BuildCanonicalBytesProducesTheExactNineFieldUtf8Document()
    {
        var nonce = Enumerable.Range(0, 16).Select(value => (byte)value).ToArray();

        var bytes = LocalAlignmentObservationExporter.BuildCanonicalBytes(
            ExactBuild,
            ExactMapSha,
            new Size(2_048, 2_048),
            new PointF(100.25f, 199.75f),
            nonce);

        const string expected =
            "{\"schema\":\"pal_companion.local_alignment_observation.v1\","
            + "\"claim\":\"independent_native_marker_observation_not_gate_b\","
            + "\"game_build_id\":24181527,"
            + "\"map_sha256\":\"aa81bdddbc525898b0a70b238cc936d0f8b0416c4ead922797b066d871938d8b\","
            + "\"map_width_px\":2048,\"map_height_px\":2048,"
            + "\"observed_marker_x_px\":100.25,\"observed_marker_y_px\":199.75,"
            + "\"nonce\":\"000102030405060708090a0b0c0d0e0f\"}";
        Assert.Equal(expected, Encoding.UTF8.GetString(bytes));
        Assert.Equal((byte)'{', bytes[0]);
        Assert.DoesNotContain((byte)'\r', bytes);
        Assert.DoesNotContain((byte)'\n', bytes);

        using var json = JsonDocument.Parse(bytes);
        Assert.Equal(9, json.RootElement.EnumerateObject().Count());
        Assert.Matches(
            "^[0-9a-f]{32}$",
            json.RootElement.GetProperty("nonce").GetString());
    }

    [Fact]
    public void CrossLanguageFixtureIsTheExactExporterByteSequence()
    {
        var generated = LocalAlignmentObservationExporter.BuildCanonicalBytes(
            ExactBuild,
            ExactMapSha,
            new Size(2_048, 2_048),
            new PointF(100.25f, 199.75f),
            Enumerable.Range(0, 16).Select(value => (byte)value).ToArray());
        var fixture = File.ReadAllBytes(WorkspacePath(
            "tools",
            "pal-map-pack",
            "tests",
            "PalMapPack.Tests",
            "fixtures",
            "local-alignment-observation-canonical.json"));

        Assert.Equal(365, fixture.Length);
        Assert.Equal((byte)'}', fixture[^1]);
        Assert.DoesNotContain((byte)'\r', fixture);
        Assert.DoesNotContain((byte)'\n', fixture);
        Assert.Equal(
            "c9f6bad85dd4138933a6d84c5af263b8a8676676543ac9354b490ad60fd8e231",
            Convert.ToHexString(SHA256.HashData(fixture)).ToLowerInvariant());
        Assert.Equal(generated, fixture);
    }

    [Fact]
    public void CrossLanguageNumericMatrixMatchesExporterBytesExactly()
    {
        var fixture = File.ReadAllBytes(WorkspacePath(
            "tools",
            "pal-map-pack",
            "tests",
            "PalMapPack.Tests",
            "fixtures",
            "local-alignment-observation-canonical-matrix.jsonl"));
        var generated = BuildNumericMatrixFixture();

        Assert.NotEmpty(fixture);
        Assert.Equal(1_821, fixture.Length);
        Assert.NotEqual((byte)'\n', fixture[^1]);
        Assert.DoesNotContain((byte)'\r', fixture);
        Assert.Equal(
            "48b3b748e6b009645b9ace68802771ea54fbd3bbdc8665af782f9e9df641d8ee",
            Convert.ToHexString(SHA256.HashData(fixture)).ToLowerInvariant());
        Assert.Equal(generated, fixture);
    }

    [Fact]
    public void CanonicalNumbersAreCultureIndependentAndNormalizeNegativeZero()
    {
        var previous = CultureInfo.CurrentCulture;
        try
        {
            CultureInfo.CurrentCulture = CultureInfo.GetCultureInfo("fr-FR");
            var bytes = LocalAlignmentObservationExporter.BuildCanonicalBytes(
                ExactBuild,
                ExactMapSha,
                new Size(2_048, 2_048),
                new PointF(-0f, -0f),
                new byte[16]);
            var json = Encoding.UTF8.GetString(bytes);

            Assert.Contains("\"observed_marker_x_px\":0", json, StringComparison.Ordinal);
            Assert.Contains("\"observed_marker_y_px\":0", json, StringComparison.Ordinal);
            Assert.DoesNotContain("-0", json, StringComparison.Ordinal);
        }
        finally
        {
            CultureInfo.CurrentCulture = previous;
        }
    }

    [Fact]
    public void CanonicalDocumentContainsNoIdentityLiveSourceOrPredictionData()
    {
        var bytes = LocalAlignmentObservationExporter.BuildCanonicalBytes(
            ExactBuild,
            ExactMapSha,
            new Size(2_048, 2_048),
            new PointF(100.25f, 199.75f),
            new byte[16]);
        var json = Encoding.UTF8.GetString(bytes);

        foreach (var forbidden in new[]
        {
            "\"world_",
            "\"yaw",
            "\"pid",
            "\"hwnd",
            "\"path",
            "\"timestamp",
            "\"account",
            "\"transform",
            "\"predicted",
            "\"residual",
        })
        {
            Assert.DoesNotContain(forbidden, json, StringComparison.OrdinalIgnoreCase);
        }
    }

    [Fact]
    public void BuildCanonicalBytesRejectsWrongIdentityDimensionsCoordinatesAndNonce()
    {
        var validPoint = new PointF(100.25f, 199.75f);
        var validSize = new Size(2_048, 2_048);
        var validNonce = new byte[16];

        Assert.Throws<ArgumentException>(() =>
            LocalAlignmentObservationExporter.BuildCanonicalBytes(
                "24181526", ExactMapSha, validSize, validPoint, validNonce));
        Assert.Throws<ArgumentException>(() =>
            LocalAlignmentObservationExporter.BuildCanonicalBytes(
                "not-numeric", ExactMapSha, validSize, validPoint, validNonce));
        Assert.Throws<ArgumentException>(() =>
            LocalAlignmentObservationExporter.BuildCanonicalBytes(
                ExactBuild, ExactMapSha.ToUpperInvariant(), validSize, validPoint, validNonce));
        Assert.Throws<ArgumentException>(() =>
            LocalAlignmentObservationExporter.BuildCanonicalBytes(
                ExactBuild, ExactMapSha, new Size(2_047, 2_048), validPoint, validNonce));
        Assert.Throws<ArgumentException>(() =>
            LocalAlignmentObservationExporter.BuildCanonicalBytes(
                ExactBuild, ExactMapSha, validSize, validPoint, new byte[15]));
        Assert.Throws<ArgumentException>(() =>
            LocalAlignmentObservationExporter.BuildCanonicalBytes(
                ExactBuild, ExactMapSha, validSize, validPoint, new byte[17]));

        foreach (var invalidPoint in new[]
        {
            new PointF(float.NaN, 0),
            new PointF(0, float.PositiveInfinity),
            new PointF(-0.01f, 0),
            new PointF(0, -0.01f),
            new PointF(2_048, 0),
            new PointF(0, 2_048),
        })
        {
            Assert.Throws<ArgumentOutOfRangeException>(() =>
                LocalAlignmentObservationExporter.BuildCanonicalBytes(
                    ExactBuild, ExactMapSha, validSize, invalidPoint, validNonce));
        }
    }

    [Fact]
    public void WriteNewCreatesOwnerOnlyOutputAndAdvancesOnlyAfterPublication()
    {
        if (!OperatingSystem.IsWindows())
        {
            return;
        }

        using var directory = OwnerOnlyTemporaryDirectory.Create();
        var output = Path.Combine(directory.Path, "observation.json");
        var session = ConfirmedSession();

        LocalAlignmentObservationExporter.WriteNew(
            output,
            session,
            new FixedNonceSource(new byte[16]));

        Assert.Equal(LocalAlignmentCapturePhase.Exported, session.Phase);
        AssertOwnerOnly(output);
        Assert.Throws<InvalidOperationException>(() =>
            LocalAlignmentObservationExporter.WriteNew(
                output,
                session,
                new FixedNonceSource(new byte[16])));
    }

    [Fact]
    public void WriteNewNeverOverwritesAndDoesNotAdvanceOnPublicationFailure()
    {
        if (!OperatingSystem.IsWindows())
        {
            return;
        }

        using var directory = OwnerOnlyTemporaryDirectory.Create();
        var output = Path.Combine(directory.Path, "observation.json");
        File.WriteAllText(output, "keep");
        var session = ConfirmedSession();

        Assert.Throws<IOException>(() =>
            LocalAlignmentObservationExporter.WriteNew(
                output,
                session,
                new FixedNonceSource(new byte[16])));

        Assert.Equal("keep", File.ReadAllText(output));
        Assert.Equal(LocalAlignmentCapturePhase.ReadyToExport, session.Phase);
    }

    [Fact]
    public void NonceFailureReleasesTheExportReservation()
    {
        var session = ConfirmedSession();

        Assert.Throws<InvalidOperationException>(() =>
            LocalAlignmentObservationExporter.WriteNew(
                Path.Combine(Path.GetTempPath(), $"unused-{Guid.NewGuid():N}.json"),
                session,
                new ThrowingNonceSource()));

        Assert.Equal(LocalAlignmentCapturePhase.ReadyToExport, session.Phase);
    }

    [Fact]
    public async Task ConcurrentExportsAllowOnlyOnePublication()
    {
        if (!OperatingSystem.IsWindows())
        {
            return;
        }

        using var directory = OwnerOnlyTemporaryDirectory.Create();
        using var nonceSource = new BlockingNonceSource();
        var first = Path.Combine(directory.Path, "first.json");
        var second = Path.Combine(directory.Path, "second.json");
        var session = ConfirmedSession();

        var publication = Task.Run(() =>
            LocalAlignmentObservationExporter.WriteNew(first, session, nonceSource));
        Assert.True(nonceSource.WaitUntilEntered(TimeSpan.FromSeconds(5)));
        Assert.Throws<InvalidOperationException>(() =>
            LocalAlignmentObservationExporter.WriteNew(
                second,
                session,
                new FixedNonceSource(new byte[16])));
        nonceSource.Release();
        await publication;

        Assert.Equal(LocalAlignmentCapturePhase.Exported, session.Phase);
        Assert.True(File.Exists(first));
        Assert.False(File.Exists(second));
    }

    [Fact]
    public async Task FailedExportCannotBeForeignCompletedAndCanBeRetried()
    {
        if (!OperatingSystem.IsWindows())
        {
            return;
        }

        using var directory = OwnerOnlyTemporaryDirectory.Create();
        using var nonceSource = new BlockingThrowingNonceSource();
        var output = Path.Combine(directory.Path, "observation.json");
        var session = ConfirmedSession();

        var publication = Task.Run(() =>
            Record.Exception(() =>
                LocalAlignmentObservationExporter.WriteNew(
                    output,
                    session,
                    nonceSource)));
        Assert.True(nonceSource.WaitUntilEntered(TimeSpan.FromSeconds(5)));

        var foreignSession = ConfirmedSession();
        var foreignLease = foreignSession.ReserveConfirmedPointForExport();
        Assert.Throws<InvalidOperationException>(() =>
            session.CompleteExport(foreignLease));
        var publicMark = typeof(LocalAlignmentCaptureSession).GetMethod(
            "MarkExported",
            BindingFlags.Instance | BindingFlags.Public);
        publicMark?.Invoke(session, parameters: null);
        nonceSource.Release();
        var publicationError = await publication;

        Assert.IsType<InvalidOperationException>(publicationError);
        Assert.Null(publicMark);
        Assert.Null(typeof(LocalAlignmentCaptureSession).GetMethod(
            "TakeConfirmedPointForExport",
            BindingFlags.Instance | BindingFlags.Public));
        Assert.Equal(LocalAlignmentCapturePhase.ReadyToExport, session.Phase);
        Assert.False(File.Exists(output));

        LocalAlignmentObservationExporter.WriteNew(
            output,
            session,
            new FixedNonceSource(new byte[16]));
        Assert.Equal(LocalAlignmentCapturePhase.Exported, session.Phase);
        Assert.True(File.Exists(output));
        foreignSession.ReleaseExport(foreignLease);
    }

    [Fact]
    public void CryptographicNonceSourceProducesExactlyOneHundredTwentyEightBits()
    {
        Assert.Equal(
            16,
            new CryptographicLocalAlignmentNonceSource().CreateNonce().Length);
    }

    private static LocalAlignmentCaptureSession ConfirmedSession()
    {
        var session = new LocalAlignmentCaptureSession();
        session.CaptureOverview(new PointF(100, 200));
        session.ConfirmPrecision(new PointF(100.25f, 199.75f));
        return session;
    }

    private static string WorkspacePath(params string[] parts)
    {
        var root = Path.GetFullPath(Path.Combine(
            AppContext.BaseDirectory,
            "..",
            "..",
            "..",
            "..",
            "..",
            "..",
            ".."));
        return Path.Combine(new[] { root }.Concat(parts).ToArray());
    }

    private static byte[] BuildNumericMatrixFixture()
    {
        var points = new[]
        {
            new PointF(-0f, 0f),
            new PointF(100f, 200f),
            new PointF(100.25f, 199.75f),
            new PointF(100f / 3f, 200f / 3f),
            new PointF(1e-7f, 2047.9999f),
        };
        using var output = new MemoryStream();
        for (var index = 0; index < points.Length; index++)
        {
            if (index != 0)
            {
                output.WriteByte((byte)'\n');
            }
            output.Write(LocalAlignmentObservationExporter.BuildCanonicalBytes(
                ExactBuild,
                ExactMapSha,
                new Size(2_048, 2_048),
                points[index],
                Enumerable.Range(0, 16)
                    .Select(value => (byte)value)
                    .ToArray()));
        }
        return output.ToArray();
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
    }

    private sealed class FixedNonceSource(byte[] nonce) : ILocalAlignmentNonceSource
    {
        public byte[] CreateNonce() => nonce.ToArray();
    }

    private sealed class ThrowingNonceSource : ILocalAlignmentNonceSource
    {
        public byte[] CreateNonce() =>
            throw new InvalidOperationException("simulated nonce failure");
    }

    private sealed class BlockingNonceSource : ILocalAlignmentNonceSource, IDisposable
    {
        private readonly ManualResetEventSlim _entered = new();
        private readonly ManualResetEventSlim _release = new();

        public byte[] CreateNonce()
        {
            _entered.Set();
            _release.Wait(TimeSpan.FromSeconds(5));
            return new byte[16];
        }

        public bool WaitUntilEntered(TimeSpan timeout) => _entered.Wait(timeout);

        public void Release() => _release.Set();

        public void Dispose()
        {
            _entered.Dispose();
            _release.Dispose();
        }
    }

    private sealed class BlockingThrowingNonceSource
        : ILocalAlignmentNonceSource, IDisposable
    {
        private readonly ManualResetEventSlim _entered = new();
        private readonly ManualResetEventSlim _release = new();

        public byte[] CreateNonce()
        {
            _entered.Set();
            _release.Wait(TimeSpan.FromSeconds(5));
            throw new InvalidOperationException("simulated blocked nonce failure");
        }

        public bool WaitUntilEntered(TimeSpan timeout) => _entered.Wait(timeout);

        public void Release() => _release.Set();

        public void Dispose()
        {
            _entered.Dispose();
            _release.Dispose();
        }
    }

    private sealed class OwnerOnlyTemporaryDirectory : IDisposable
    {
        private OwnerOnlyTemporaryDirectory(string path)
        {
            Path = path;
        }

        public string Path { get; }

        public static OwnerOnlyTemporaryDirectory Create()
        {
            var path = System.IO.Path.Combine(
                System.IO.Path.GetTempPath(),
                $"pal-local-alignment-{Guid.NewGuid():N}");
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
            return new OwnerOnlyTemporaryDirectory(path);
        }

        public void Dispose()
        {
            if (Directory.Exists(Path))
            {
                Directory.Delete(Path, recursive: true);
            }
        }
    }
}
