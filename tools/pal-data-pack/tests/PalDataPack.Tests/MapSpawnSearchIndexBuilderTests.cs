using System.Text.Json;
using PalDataPack;

namespace PalDataPack.Tests;

public sealed class MapSpawnSearchIndexBuilderTests
{
    [Fact]
    public void Builds_compact_species_to_placement_index()
    {
        var root = Path.Combine(Path.GetTempPath(), $"pal-spawn-index-{Guid.NewGuid():N}");
        Directory.CreateDirectory(root);
        try
        {
            var sourcePath = Path.Combine(root, "source.json");
            var outputPath = Path.Combine(root, "index.json");
            var source = new VerifiedMapSpawnDocument(
                1,
                "steam:24181527",
                new string('a', 64),
                "review:test",
                true,
                "placement row",
                "weighted group row",
                0,
                0,
                Array.Empty<MapSpawnSource>(),
                [
                    new(
                        "placement:1",
                        "1",
                        null,
                        "sky_a",
                        "PL_MainWorld5",
                        "Field",
                        "Common",
                        "S",
                        10,
                        20,
                        30,
                        15000,
                        0,
                        true),
                    new(
                        "placement:2",
                        "2",
                        null,
                        "missing",
                        "PL_MainWorld5",
                        "Field",
                        "Common",
                        "S",
                        40,
                        50,
                        60,
                        15000,
                        0,
                        false),
                ],
                [
                    new(
                        "group:1",
                        "1",
                        "sky_a",
                        "Common",
                        100,
                        "Night",
                        "Undefined",
                        false,
                        true,
                        [
                            new(1, "SkyDragon", null, 23, 25, 1, 2, false, false),
                            new(2, null, "Hunter", 20, 20, 1, 1, false, false),
                        ]),
                ]);
            File.WriteAllBytes(
                sourcePath,
                JsonSerializer.SerializeToUtf8Bytes(
                    source,
                    new JsonSerializerOptions
                    {
                        PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower,
                    }));

            var result = MapSpawnSearchIndexBuilder.BuildToFile(sourcePath, outputPath);

            Assert.Equal("steam:24181527", result.GameBuildId);
            Assert.Equal(1, result.PlacementCount);
            Assert.Equal(1, result.SpawnRuleCount);
            Assert.Equal(1, result.SpeciesCount);
            using var index = JsonDocument.Parse(File.ReadAllBytes(outputPath));
            var rootElement = index.RootElement;
            Assert.Equal(1, rootElement.GetProperty("schema_version").GetInt32());
            Assert.Equal(1, rootElement.GetProperty("placements").GetArrayLength());
            var rules = rootElement.GetProperty("species").GetProperty("SkyDragon");
            Assert.Equal(1, rules.GetArrayLength());
            Assert.Equal("sky_a", rules[0][0].GetString());
            Assert.Equal("Night", rules[0][3].GetString());
            Assert.False(rootElement.GetProperty("species").TryGetProperty("Hunter", out _));
        }
        finally
        {
            Directory.Delete(root, recursive: true);
        }
    }
}
