using System.Drawing;
using System.Security.Cryptography;
using System.Text.Json;
using System.Text.Json.Serialization;
using PalMapPack;

namespace PalMapPack.Calibrator;

public interface ILocalAlignmentNonceSource
{
    byte[] CreateNonce();
}

public sealed class CryptographicLocalAlignmentNonceSource
    : ILocalAlignmentNonceSource
{
    public byte[] CreateNonce()
    {
        var nonce = new byte[LocalAlignmentObservationExporter.NonceSizeBytes];
        RandomNumberGenerator.Fill(nonce);
        return nonce;
    }
}

public static class LocalAlignmentObservationExporter
{
    public const string Schema =
        "pal_companion.local_alignment_observation.v1";
    public const string Claim =
        "independent_native_marker_observation_not_gate_b";
    public const string ExactBuildId = "24181527";
    public const string ExactMapSha256 =
        "aa81bdddbc525898b0a70b238cc936d0f8b0416c4ead922797b066d871938d8b";
    public const int MapWidthPx = 2_048;
    public const int MapHeightPx = 2_048;
    public const int NonceSizeBytes = 16;

    private static readonly JsonSerializerOptions JsonOptions = new()
    {
        WriteIndented = false,
    };

    public static byte[] BuildCanonicalBytes(
        string buildId,
        string mapSha256,
        Size mapSize,
        PointF observedMarker,
        ReadOnlySpan<byte> nonce)
    {
        if (!string.Equals(buildId, ExactBuildId, StringComparison.Ordinal)
            || buildId.Any(character => !char.IsAsciiDigit(character)))
        {
            throw new ArgumentException(
                "local alignment observation requires exact Build 24181527",
                nameof(buildId));
        }
        if (!string.Equals(mapSha256, ExactMapSha256, StringComparison.Ordinal))
        {
            throw new ArgumentException(
                "local alignment observation requires the exact 2048px map hash",
                nameof(mapSha256));
        }
        if (mapSize != new Size(MapWidthPx, MapHeightPx))
        {
            throw new ArgumentException(
                "local alignment observation requires the exact 2048px map",
                nameof(mapSize));
        }
        if (!float.IsFinite(observedMarker.X)
            || !float.IsFinite(observedMarker.Y)
            || observedMarker.X < 0
            || observedMarker.X >= MapWidthPx
            || observedMarker.Y < 0
            || observedMarker.Y >= MapHeightPx)
        {
            throw new ArgumentOutOfRangeException(
                nameof(observedMarker),
                "observed marker must be finite and inside the 2048px map");
        }
        if (nonce.Length != NonceSizeBytes)
        {
            throw new ArgumentException(
                "local alignment nonce must contain exactly 128 bits",
                nameof(nonce));
        }

        var markerX = observedMarker.X == 0 ? 0 : observedMarker.X;
        var markerY = observedMarker.Y == 0 ? 0 : observedMarker.Y;
        var output = new ObservationOutput(
            Schema,
            Claim,
            24_181_527,
            ExactMapSha256,
            MapWidthPx,
            MapHeightPx,
            markerX,
            markerY,
            Convert.ToHexString(nonce).ToLowerInvariant());
        return JsonSerializer.SerializeToUtf8Bytes(output, JsonOptions);
    }

    public static void WriteNew(
        string path,
        LocalAlignmentCaptureSession session,
        ILocalAlignmentNonceSource nonceSource)
    {
        ArgumentNullException.ThrowIfNull(path);
        ArgumentNullException.ThrowIfNull(session);
        ArgumentNullException.ThrowIfNull(nonceSource);

        var lease = session.ReserveConfirmedPointForExport();
        try
        {
            var nonce = nonceSource.CreateNonce();
            ArgumentNullException.ThrowIfNull(nonce);
            var bytes = BuildCanonicalBytes(
                ExactBuildId,
                ExactMapSha256,
                new Size(MapWidthPx, MapHeightPx),
                lease.ObservedMarker,
                nonce);
            ProtectedFileIO.WriteNewOwnerOnlyAtomic(path, bytes);
            session.CompleteExport(lease);
        }
        catch
        {
            session.ReleaseExport(lease);
            throw;
        }
    }

    private sealed record ObservationOutput(
        [property: JsonPropertyName("schema"), JsonPropertyOrder(0)]
        string Schema,
        [property: JsonPropertyName("claim"), JsonPropertyOrder(1)]
        string Claim,
        [property: JsonPropertyName("game_build_id"), JsonPropertyOrder(2)]
        int GameBuildId,
        [property: JsonPropertyName("map_sha256"), JsonPropertyOrder(3)]
        string MapSha256,
        [property: JsonPropertyName("map_width_px"), JsonPropertyOrder(4)]
        int MapWidthPx,
        [property: JsonPropertyName("map_height_px"), JsonPropertyOrder(5)]
        int MapHeightPx,
        [property: JsonPropertyName("observed_marker_x_px"), JsonPropertyOrder(6)]
        float ObservedMarkerXPx,
        [property: JsonPropertyName("observed_marker_y_px"), JsonPropertyOrder(7)]
        float ObservedMarkerYPx,
        [property: JsonPropertyName("nonce"), JsonPropertyOrder(8)]
        string Nonce);
}
