using PalMapPack.Calibrator;

namespace PalMapPack.Tests;

public sealed class LocalAlignmentCaptureCommandTests
{
    [Fact]
    public void ParseAcceptsOnlyTheExactBuildAndAbsoluteTypedPaths()
    {
        var map = Path.Combine(Path.GetTempPath(), "팰월드-🧭-mainmap-24181527.bmp");
        var output = Path.Combine(Path.GetTempPath(), "원본-📍-marker.json");

        var command = LocalAlignmentCaptureCommand.Parse(
        [
            "--local-alignment-capture",
            "--build",
            "24181527",
            "--map",
            map,
            "--output",
            output,
        ]);

        Assert.Equal("24181527", command.BuildId);
        Assert.Equal(Path.GetFullPath(map), command.MapPath);
        Assert.Equal(Path.GetFullPath(output), command.OutputPath);
    }

    [Fact]
    public void ParseRejectsMissingDuplicateUnknownReorderedAndExtraArguments()
    {
        var map = Path.Combine(Path.GetTempPath(), "map.bmp");
        var output = Path.Combine(Path.GetTempPath(), "observation.json");
        var invalid = new IReadOnlyList<string>[]
        {
            [],
            ["--local-alignment-capture"],
            [
                "--local-alignment-capture",
                "--build",
                "24181527",
                "--map",
                map,
                "--output",
            ],
            [
                "--local-alignment-capture",
                "--build",
                "24181527",
                "--map",
                map,
                "--map",
                output,
            ],
            [
                "--local-alignment-capture",
                "--build",
                "24181527",
                "--world-x",
                "1",
                "--output",
                output,
            ],
            [
                "--local-alignment-capture",
                "--map",
                map,
                "--build",
                "24181527",
                "--output",
                output,
            ],
            [
                "--local-alignment-capture",
                "--build",
                "24181527",
                "--map",
                map,
                "--output",
                output,
                "--predicted-marker",
            ],
            [
                "--different-mode",
                "--build",
                "24181527",
                "--map",
                map,
                "--output",
                output,
            ],
        };

        foreach (var args in invalid)
        {
            Assert.Throws<ArgumentException>(() =>
                LocalAlignmentCaptureCommand.Parse(args));
        }
    }

    [Fact]
    public void ParseRejectsWrongBuildRelativeOrWrongExtensionPathsAndInvalidUnicode()
    {
        var root = Path.GetTempPath();
        var map = Path.Combine(root, "map.bmp");
        var output = Path.Combine(root, "observation.json");
        var invalid = new IReadOnlyList<string>[]
        {
            ValidArgs(map, output, build: ""),
            ValidArgs(map, output, build: "not-numeric"),
            ValidArgs(map, output, build: "24181526"),
            ValidArgs("relative-map.bmp", output),
            ValidArgs(Path.Combine(root, "map.png"), output),
            ValidArgs(map, "relative-output.json"),
            ValidArgs(map, Path.Combine(root, "observation.txt")),
            ValidArgs("", output),
            ValidArgs(map, ""),
            ValidArgs(Path.Combine(root, "bad\uD800.bmp"), output),
            ValidArgs(map, Path.Combine(root, "bad\uDFFF.json")),
            ValidArgs(Path.Combine(root, "bad\0.bmp"), output),
            ValidArgs(map, Path.Combine(root, "bad\0.json")),
        };

        foreach (var args in invalid)
        {
            Assert.Throws<ArgumentException>(() =>
                LocalAlignmentCaptureCommand.Parse(args));
        }
    }

    private static IReadOnlyList<string> ValidArgs(
        string map,
        string output,
        string build = "24181527") =>
    [
        "--local-alignment-capture",
        "--build",
        build,
        "--map",
        map,
        "--output",
        output,
    ];
}
