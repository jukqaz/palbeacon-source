using System.Text;

namespace PalMapPack.Calibrator;

public sealed record LocalAlignmentCaptureCommand(
    string BuildId,
    string MapPath,
    string OutputPath)
{
    public const string ExactBuildId = "24181527";

    private static readonly UTF8Encoding StrictUtf8 = new(
        encoderShouldEmitUTF8Identifier: false,
        throwOnInvalidBytes: true);

    public static LocalAlignmentCaptureCommand Parse(IReadOnlyList<string> args)
    {
        ArgumentNullException.ThrowIfNull(args);
        if (args.Count != 7
            || args.Any(argument => !IsStrictUnicode(argument))
            || !string.Equals(
                args[0],
                "--local-alignment-capture",
                StringComparison.Ordinal)
            || !string.Equals(args[1], "--build", StringComparison.Ordinal)
            || !string.Equals(args[2], ExactBuildId, StringComparison.Ordinal)
            || args[2].Any(character => !char.IsAsciiDigit(character))
            || !string.Equals(args[3], "--map", StringComparison.Ordinal)
            || !string.Equals(args[5], "--output", StringComparison.Ordinal))
        {
            throw UsageError();
        }

        var mapPath = RequireAbsoluteTypedPath(args[4], ".bmp", "map");
        var outputPath = RequireAbsoluteTypedPath(args[6], ".json", "output");
        return new LocalAlignmentCaptureCommand(ExactBuildId, mapPath, outputPath);
    }

    private static string RequireAbsoluteTypedPath(
        string path,
        string extension,
        string argumentName)
    {
        if (string.IsNullOrEmpty(path)
            || !IsStrictUnicode(path)
            || !Path.IsPathFullyQualified(path)
            || !string.Equals(
                Path.GetExtension(path),
                extension,
                StringComparison.OrdinalIgnoreCase))
        {
            throw new ArgumentException(
                $"{argumentName} must be an absolute {extension} path",
                argumentName);
        }

        try
        {
            return Path.GetFullPath(path);
        }
        catch (Exception error)
            when (error is ArgumentException
                or NotSupportedException
                or PathTooLongException)
        {
            throw new ArgumentException(
                $"{argumentName} must be an absolute {extension} path",
                argumentName,
                error);
        }
    }

    private static bool IsStrictUnicode(string? value)
    {
        if (value is null || value.Contains('\0', StringComparison.Ordinal))
        {
            return false;
        }

        try
        {
            _ = StrictUtf8.GetByteCount(value);
            return true;
        }
        catch (EncoderFallbackException)
        {
            return false;
        }
    }

    private static ArgumentException UsageError() =>
        new(
            "usage: PalMapPack.Calibrator.exe --local-alignment-capture "
            + "--build 24181527 --map <absolute .bmp path> "
            + "--output <absolute .json path>");
}
