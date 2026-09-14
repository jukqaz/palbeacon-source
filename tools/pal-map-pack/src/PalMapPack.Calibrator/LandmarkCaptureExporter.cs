using System.Text.Json;
using System.Text.Json.Serialization;

namespace PalMapPack.Calibrator;

public static class LandmarkCaptureExporter
{
    private const int MaximumCommitmentBytes = 1_048_576;
    private static readonly JsonSerializerOptions JsonOptions = new()
    {
        PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower,
        WriteIndented = true,
        UnmappedMemberHandling = JsonUnmappedMemberHandling.Disallow,
    };

    public static void WriteHoldoutsNew(string path, LandmarkCaptureSession session)
    {
        ArgumentNullException.ThrowIfNull(session);
        if (session.Phase != CapturePhase.HoldoutsReadyForExport)
        {
            throw new InvalidOperationException("all five holdouts must be complete before export");
        }
        WriteNew(path, SelectRows(session.Landmarks, final: false));
        session.MarkHoldoutsExported();
    }

    public static void WriteFinalNew(string path, LandmarkCaptureSession session)
    {
        ArgumentNullException.ThrowIfNull(session);
        if (session.Phase != CapturePhase.Complete)
        {
            throw new InvalidOperationException(
                "final export requires five approved holdouts and ten references");
        }
        _ = session.CreateCommitment();
        WriteNew(path, SelectRows(session.Landmarks, final: true));
    }

    public static SealedHoldoutCommitment ReadCommitment(string path)
    {
        var bytes = ProtectedFileIO.ReadAllBytes(path, MaximumCommitmentBytes);
        var document = JsonSerializer.Deserialize<CommitmentInput>(bytes, JsonOptions)
            ?? throw new JsonException("empty holdout commitment");
        return new SealedHoldoutCommitment(
            document.SchemaVersion,
            document.GameBuildId,
            document.SealedHoldoutCount,
            document.SealedHoldoutSha256,
            new Dictionary<LandmarkZone, uint>
            {
                [LandmarkZone.Center] = document.CenterCount,
                [LandmarkZone.NorthWest] = document.NorthWestCount,
                [LandmarkZone.NorthEast] = document.NorthEastCount,
                [LandmarkZone.SouthWest] = document.SouthWestCount,
                [LandmarkZone.SouthEast] = document.SouthEastCount,
            });
    }

    private static LandmarkOutput[] SelectRows(
        IReadOnlyList<Landmark> landmarks,
        bool final)
    {
        var selected = (final
                ? landmarks
                : landmarks.Where(row =>
                    row.EvidenceSet == LandmarkEvidenceSet.SealedHoldout))
            .OrderBy(row => row.EvidenceSet)
            .ThenBy(row => row.Zone)
            .ThenBy(row => row.Id, StringComparer.Ordinal)
            .ToArray();
        var referenceCount = selected.Count(row =>
            row.EvidenceSet == LandmarkEvidenceSet.Reference);
        var holdoutCount = selected.Count(row =>
            row.EvidenceSet == LandmarkEvidenceSet.SealedHoldout);
        if ((final && (selected.Length != 15 || referenceCount != 10 || holdoutCount != 5))
            || (!final && (selected.Length != 5 || referenceCount != 0 || holdoutCount != 5))
            || selected.Select(row => row.GameBuildId).Distinct(StringComparer.Ordinal).Count() != 1)
        {
            throw new InvalidOperationException("capture output is incomplete or mixed-Build");
        }
        return selected.Select(row => new LandmarkOutput(
            row.Id,
            row.GameBuildId,
            row.EvidenceSet == LandmarkEvidenceSet.Reference
                ? "reference"
                : "sealed_holdout",
            row.WorldX,
            row.WorldY,
            row.MapX,
            row.MapY)).ToArray();
    }

    private static void WriteNew(string path, LandmarkOutput[] rows)
    {
        ArgumentNullException.ThrowIfNull(path);
        var bytes = JsonSerializer.SerializeToUtf8Bytes(rows, JsonOptions);
        ProtectedFileIO.WriteNewAtomic(path, bytes);
    }

    private sealed record LandmarkOutput(
        string Id,
        string GameBuildId,
        string EvidenceSet,
        double WorldX,
        double WorldY,
        double MapX,
        double MapY);

    private sealed record CommitmentInput(
        uint SchemaVersion,
        string GameBuildId,
        uint SealedHoldoutCount,
        string SealedHoldoutSha256,
        uint CenterCount,
        uint NorthWestCount,
        uint NorthEastCount,
        uint SouthWestCount,
        uint SouthEastCount);
}
