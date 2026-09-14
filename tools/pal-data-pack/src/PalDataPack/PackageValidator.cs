using System.Text;
using System.Text.Json;

namespace PalDataPack;

public sealed record SourceInspection(
    string GameBuildId,
    IReadOnlyList<SourceFingerprint> Sources,
    IReadOnlyList<CapabilityStatus> Capabilities,
    IReadOnlyList<PackageFile> Files);

public static class PackageValidator
{
    private static readonly UTF8Encoding StrictUtf8 = new(
        encoderShouldEmitUTF8Identifier: false,
        throwOnInvalidBytes: true);

    public static SourceInspection InspectNormalizedSource(
        string sourceDirectory,
        string expectedGameBuildId)
    {
        CatalogContract.RequireBuildId(expectedGameBuildId);
        SecureInput.EnsurePlainDirectory(sourceDirectory);
        var root = Path.GetFullPath(sourceDirectory);
        var requiredFiles = CatalogContract.CapabilityFiles.Values
            .Distinct(StringComparer.Ordinal)
            .Order(StringComparer.Ordinal)
            .ToArray();
        foreach (var requiredFile in requiredFiles)
        {
            if (!File.Exists(Path.Combine(root, requiredFile)))
            {
                throw new DataPackFailure(
                    DataPackExitCode.SourceContractMismatch,
                    $"normalized source is missing {requiredFile}");
            }
        }

        var parsed = new Dictionary<string, IReadOnlyList<JsonElement>>(
            StringComparer.Ordinal);
        var files = new List<PackageFile>(requiredFiles.Length);
        foreach (var fileName in requiredFiles)
        {
            var path = Path.Combine(root, fileName);
            var rows = ReadRows(path, expectedGameBuildId);
            if (rows.Count == 0)
            {
                throw new DataPackFailure(
                    DataPackExitCode.SourceContractMismatch,
                    $"normalized source is empty: {fileName}");
            }
            parsed.Add(fileName, rows);
            var info = new FileInfo(path);
            files.Add(new PackageFile(
                fileName,
                rows.Count,
                info.Length,
                Hashing.Sha256File(path)));
        }

        EnforceRowBounds(parsed);
        ValidateCapabilityKinds(parsed);
        var sources = ParseSources(parsed["sources.ndjson"]);
        ValidateReferences(parsed, sources);
        var capabilities = CatalogContract.RequiredCapabilities.Select(capability =>
            new CapabilityStatus(
                capability,
                CapabilityState.Complete,
                SourceIdsForCapability(parsed, capability))).ToArray();

        return new SourceInspection(
            expectedGameBuildId,
            sources,
            capabilities,
            files.OrderBy(file => file.Path, StringComparer.Ordinal).ToArray());
    }

    public static DataPackManifestV1 ReopenAndVerify(string versionPath)
    {
        SecureInput.EnsurePlainDirectory(versionPath);
        var root = Path.GetFullPath(versionPath);
        var manifest = ManifestCodec.Load(Path.Combine(root, "manifest.json"));
        ValidateManifestShape(manifest);

        foreach (var file in manifest.Files)
        {
            var path = Path.GetFullPath(Path.Combine(root, file.Path));
            if (!string.Equals(
                    Path.GetDirectoryName(path),
                    root,
                    StringComparison.OrdinalIgnoreCase))
            {
                throw IntegrityFailure("manifest file path escapes the package");
            }
            SecureInput.EnsureRegularFile(path);
            var info = new FileInfo(path);
            if (info.Length != file.FileSizeBytes
                || Hashing.Sha256File(path) != file.Sha256
                || CountLines(path) != file.RowCount)
            {
                throw IntegrityFailure($"package file verification failed: {file.Path}");
            }
        }

        var computed = Hashing.DatasetManifestId(
            manifest.SchemaMajor,
            manifest.GameBuildId,
            manifest.DistributionScope,
            manifest.Sources,
            manifest.Capabilities,
            manifest.Files);
        if (computed != manifest.DatasetManifestId)
        {
            throw IntegrityFailure("dataset manifest identity does not match content");
        }

        SourceInspection inspection;
        try
        {
            inspection = InspectNormalizedSource(root, manifest.GameBuildId);
        }
        catch (DataPackFailure error)
        {
            throw new DataPackFailure(
                DataPackExitCode.PackageIntegrity,
                $"published normalized data is invalid: {error.Message}");
        }
        if (!inspection.Files.SequenceEqual(manifest.Files)
            || !inspection.Sources.SequenceEqual(manifest.Sources)
            || !CapabilitySequenceEqual(inspection.Capabilities, manifest.Capabilities))
        {
            throw IntegrityFailure("manifest facts do not describe the reopened package");
        }
        return manifest;
    }

    private static IReadOnlyList<JsonElement> ReadRows(
        string path,
        string expectedGameBuildId)
    {
        SecureInput.EnsureRegularFile(path);
        var info = new FileInfo(path);
        if (info.Length is <= 0 or > CatalogContract.MaximumFileBytes)
        {
            throw new DataPackFailure(
                DataPackExitCode.SourceContractMismatch,
                "normalized file exceeds its size bound");
        }
        var rows = new List<JsonElement>();
        using var stream = new FileStream(
            path,
            FileMode.Open,
            FileAccess.Read,
            FileShare.Read,
            64 * 1024,
            FileOptions.SequentialScan);
        using var reader = new StreamReader(
            stream,
            StrictUtf8,
            detectEncodingFromByteOrderMarks: false,
            bufferSize: 64 * 1024,
            leaveOpen: false);
        while (reader.ReadLine() is { } line)
        {
            if (StrictUtf8.GetByteCount(line) > CatalogContract.MaximumLineBytes)
            {
                throw new DataPackFailure(
                    DataPackExitCode.SourceContractMismatch,
                    "normalized row exceeds its size bound");
            }
            try
            {
                using var document = JsonDocument.Parse(
                    line,
                    new JsonDocumentOptions
                    {
                        AllowTrailingCommas = false,
                        CommentHandling = JsonCommentHandling.Disallow,
                        MaxDepth = 32,
                    });
                var row = document.RootElement.Clone();
                if (row.ValueKind != JsonValueKind.Object
                    || RequiredInt(row, "schema_major") != CatalogContract.SchemaMajor
                    || RequiredString(row, "game_build_id") != expectedGameBuildId)
                {
                    throw new JsonException("row provenance mismatch");
                }
                CatalogContract.RequireIdentifier(
                    RequiredString(row, "source_id"),
                    "source_id");
                var entityVersion = RequiredString(row, "entity_version");
                CatalogContract.RequireIdentifier(entityVersion, "entity_version");
                if (entityVersion != expectedGameBuildId)
                {
                    throw new JsonException("row entity version does not match Build");
                }
                RejectFloatingPointNumbers(row);
                rows.Add(row);
            }
            catch (Exception error) when (error is JsonException
                or InvalidOperationException
                or DataPackFailure)
            {
                throw new DataPackFailure(
                    DataPackExitCode.SourceContractMismatch,
                    $"normalized row is invalid in {Path.GetFileName(path)}");
            }
        }
        return rows;
    }

    private static void RejectFloatingPointNumbers(JsonElement element)
    {
        switch (element.ValueKind)
        {
            case JsonValueKind.Object:
                foreach (var property in element.EnumerateObject())
                {
                    RejectFloatingPointNumbers(property.Value);
                }
                break;
            case JsonValueKind.Array:
                foreach (var value in element.EnumerateArray())
                {
                    RejectFloatingPointNumbers(value);
                }
                break;
            case JsonValueKind.Number:
                if (!element.TryGetInt64(out _)
                    && !element.TryGetUInt64(out _))
                {
                    throw new JsonException("floating-point values are forbidden");
                }
                break;
        }
    }

    private static void ValidateCapabilityKinds(
        IReadOnlyDictionary<string, IReadOnlyList<JsonElement>> parsed)
    {
        var worldKinds = parsed["world.ndjson"]
            .Select(row => RequiredString(row, "kind"))
            .ToHashSet(StringComparer.Ordinal);
        foreach (var kind in new[] { "technology", "building", "location", "poi", "alias" })
        {
            if (!worldKinds.Contains(kind))
            {
                throw new DataPackFailure(
                    DataPackExitCode.SourceContractMismatch,
                    $"world data is missing required kind {kind}");
            }
        }
        var skillKinds = parsed["skills.ndjson"]
            .Select(row => RequiredString(row, "skill_kind"))
            .ToHashSet(StringComparer.Ordinal);
        if (!new[] { "active", "passive", "partner" }.All(skillKinds.Contains))
        {
            throw new DataPackFailure(
                DataPackExitCode.SourceContractMismatch,
                "skills data is not capability-complete");
        }
    }

    private static void EnforceRowBounds(
        IReadOnlyDictionary<string, IReadOnlyList<JsonElement>> parsed)
    {
        var bounds = new Dictionary<string, int>(StringComparer.Ordinal)
        {
            ["pals.ndjson"] = 4_096,
            ["skills.ndjson"] = 65_536,
            ["items.ndjson"] = 131_072,
            ["recipes.ndjson"] = 131_072,
            ["acquisition.ndjson"] = 1_048_576,
            ["breeding.ndjson"] = 1_048_576,
            ["world.ndjson"] = 1_048_576,
            ["localization.ndjson"] = 1_048_576,
            ["sources.ndjson"] = 1_024,
        };
        foreach (var (file, maximum) in bounds)
        {
            if (parsed[file].Count > maximum)
            {
                throw new DataPackFailure(
                    DataPackExitCode.SourceContractMismatch,
                    $"normalized source exceeds the row bound for {file}");
            }
        }
    }

    private static IReadOnlyList<SourceFingerprint> ParseSources(
        IReadOnlyList<JsonElement> rows)
    {
        var sources = rows.Select(row =>
        {
            var sourceId = RequiredString(row, "source_id");
            var hash = RequiredString(row, "source_sha256");
            if (!CatalogContract.IsCanonicalSha256(hash))
            {
                throw new DataPackFailure(
                    DataPackExitCode.SourceContractMismatch,
                    "source hash is not canonical SHA-256");
            }
            return new SourceFingerprint(
                sourceId,
                RequiredString(row, "source_kind"),
                hash,
                row.GetProperty("verified").GetBoolean());
        }).OrderBy(source => source.SourceId, StringComparer.Ordinal).ToArray();
        if (sources.Select(source => source.SourceId)
            .Distinct(StringComparer.Ordinal).Count() != sources.Length)
        {
            throw new DataPackFailure(
                DataPackExitCode.ReferenceIntegrity,
                "source identifiers are not unique");
        }
        return sources;
    }

    private static IReadOnlyList<string> SourceIdsForCapability(
        IReadOnlyDictionary<string, IReadOnlyList<JsonElement>> parsed,
        string capability)
    {
        var file = CatalogContract.CapabilityFiles[capability];
        IEnumerable<JsonElement> rows = parsed[file];
        if (file == "world.ndjson")
        {
            var kind = capability switch
            {
                "technologies" => "technology",
                "buildings" => "building",
                "locations" => "location",
                "pois" => "poi",
                "aliases" => "alias",
                _ => throw new InvalidOperationException("unknown world capability"),
            };
            rows = rows.Where(row => RequiredString(row, "kind") == kind);
        }
        return rows.Select(row => RequiredString(row, "source_id"))
            .Distinct(StringComparer.Ordinal)
            .Order(StringComparer.Ordinal)
            .ToArray();
    }

    private static void ValidateReferences(
        IReadOnlyDictionary<string, IReadOnlyList<JsonElement>> parsed,
        IReadOnlyList<SourceFingerprint> sources)
    {
        var sourceIds = sources.Select(source => source.SourceId)
            .ToHashSet(StringComparer.Ordinal);
        foreach (var row in parsed.Values.SelectMany(rows => rows))
        {
            RequireReference(sourceIds, RequiredString(row, "source_id"), "source");
        }

        var pals = UniqueIds(parsed["pals.ndjson"], "species_id", "Pal");
        var items = UniqueIds(parsed["items.ndjson"], "item_id", "item");
        var recipes = UniqueIds(parsed["recipes.ndjson"], "recipe_id", "recipe");
        _ = UniqueIds(parsed["skills.ndjson"], "skill_id", "skill");
        _ = UniqueIds(parsed["breeding.ndjson"], "rule_id", "breeding rule");
        _ = UniqueIds(parsed["acquisition.ndjson"], "method_id", "acquisition method");
        var world = parsed["world.ndjson"];
        var technologies = UniqueNestedIds(world, "technology", "technology_id");
        var buildings = UniqueNestedIds(world, "building", "building_id");
        var locations = UniqueNestedIds(world, "location", "location_id");

        foreach (var row in parsed["recipes.ndjson"])
        {
            RequireReference(items, RequiredString(row, "output_item_id"), "recipe output");
            foreach (var ingredient in row.GetProperty("ingredients").EnumerateArray())
            {
                RequireReference(
                    items,
                    RequiredString(ingredient, "item_id"),
                    "recipe ingredient");
                RequirePositive(ingredient, "quantity");
            }
            RequirePositive(row, "output_quantity");
        }

        foreach (var row in parsed["breeding.ndjson"])
        {
            RequireReference(
                pals,
                RequiredString(row, "parent_a_species_id"),
                "breeding parent");
            RequireReference(
                pals,
                RequiredString(row, "parent_b_species_id"),
                "breeding parent");
            RequireReference(
                pals,
                RequiredString(row, "child_species_id"),
                "breeding child");
        }

        foreach (var row in parsed["acquisition.ndjson"])
        {
            var kind = RequiredString(row, "kind");
            if (kind == "captured_cage")
            {
                RequireReference(
                    pals,
                    RequiredString(row, "species_id"),
                    "captured cage Pal");
            }
            else
            {
                RequireReference(
                    items,
                    RequiredString(row, "item_id"),
                    "acquisition target");
            }
            if (row.TryGetProperty("location_id", out var location))
            {
                RequireReference(locations, location.GetString()!, "acquisition location");
            }
            switch (kind)
            {
                case "pal_drop":
                    ValidateQuantityMethod(row.GetProperty("pal_drop"));
                    RequireReference(
                        pals,
                        RequiredString(row.GetProperty("pal_drop"), "pal_id"),
                        "drop Pal");
                    break;
                case "craft":
                    RequireReference(
                        recipes,
                        RequiredString(row.GetProperty("craft"), "recipe_id"),
                        "craft recipe");
                    break;
                case "merchant":
                    var merchant = row.GetProperty("merchant");
                    if (merchant.TryGetProperty("currency_item_id", out var currency))
                    {
                        RequireReference(items, currency.GetString()!, "merchant currency");
                    }
                    break;
                case "ranch":
                    ValidateQuantityMethod(row.GetProperty("ranch"));
                    RequireReference(
                        pals,
                        RequiredString(row.GetProperty("ranch"), "species_id"),
                        "ranch Pal");
                    break;
                case "gather":
                    ValidateQuantityMethod(row.GetProperty("gather"));
                    break;
                case "expedition":
                    ValidateQuantityMethod(row.GetProperty("expedition"));
                    RequirePositive(row.GetProperty("expedition"), "duration_ms");
                    break;
                case "fishing":
                    ValidateQuantityMethod(row.GetProperty("fishing"));
                    break;
                case "salvage":
                    ValidateQuantityMethod(row.GetProperty("salvage"));
                    break;
                case "chest":
                    ValidateQuantityMethod(row.GetProperty("chest"));
                    break;
                case "boss":
                    ValidateQuantityMethod(row.GetProperty("boss"));
                    break;
                case "captured_cage":
                    var cage = row.GetProperty("captured_cage");
                    ValidateRange(cage, "minimum_level", "maximum_level");
                    RequirePositive(cage, "source_weight");
                    if (cage.TryGetProperty("probability_ppm", out var cageProbability))
                    {
                        ValidateProbability(cageProbability);
                    }
                    break;
                default:
                    throw new DataPackFailure(
                        DataPackExitCode.ReferenceIntegrity,
                        $"unknown acquisition kind {kind}");
            }
        }

        foreach (var row in world)
        {
            switch (RequiredString(row, "kind"))
            {
                case "building":
                    var building = row.GetProperty("building");
                    if (building.TryGetProperty("technology_id", out var technology))
                    {
                        RequireReference(
                            technologies,
                            technology.GetString()!,
                            "building technology");
                    }
                    break;
                case "poi":
                    RequireReference(
                        locations,
                        RequiredString(row.GetProperty("poi"), "location_id"),
                        "POI location");
                    break;
                case "technology":
                case "location":
                case "alias":
                    break;
            }
        }

        _ = buildings;
    }

    private static void ValidateQuantityMethod(JsonElement method)
    {
        ValidateRange(method, "minimum_quantity", "maximum_quantity");
        ValidateProbability(method.GetProperty("probability_ppm"));
    }

    private static void ValidateRange(
        JsonElement element,
        string minimumProperty,
        string maximumProperty)
    {
        var minimum = RequiredInt(element, minimumProperty);
        var maximum = RequiredInt(element, maximumProperty);
        if (minimum < 0 || maximum < minimum)
        {
            throw new DataPackFailure(
                DataPackExitCode.ReferenceIntegrity,
                $"{minimumProperty}/{maximumProperty} range is invalid");
        }
    }

    private static void ValidateProbability(JsonElement probability)
    {
        var value = probability.GetInt64();
        if (value is < 0 or > 1_000_000)
        {
            throw new DataPackFailure(
                DataPackExitCode.ReferenceIntegrity,
                "probability_ppm is outside 0..=1,000,000");
        }
    }

    private static HashSet<string> UniqueIds(
        IReadOnlyList<JsonElement> rows,
        string property,
        string label)
    {
        var values = rows.Select(row => RequiredString(row, property)).ToArray();
        if (values.Distinct(StringComparer.Ordinal).Count() != values.Length)
        {
            throw new DataPackFailure(
                DataPackExitCode.ReferenceIntegrity,
                $"{label} identifiers are not unique");
        }
        return values.ToHashSet(StringComparer.Ordinal);
    }

    private static HashSet<string> UniqueNestedIds(
        IReadOnlyList<JsonElement> rows,
        string kind,
        string property)
    {
        var values = rows.Where(row => RequiredString(row, "kind") == kind)
            .Select(row => RequiredString(row.GetProperty(kind), property))
            .ToArray();
        if (values.Distinct(StringComparer.Ordinal).Count() != values.Length)
        {
            throw new DataPackFailure(
                DataPackExitCode.ReferenceIntegrity,
                $"{kind} identifiers are not unique");
        }
        return values.ToHashSet(StringComparer.Ordinal);
    }

    private static void RequireReference(
        HashSet<string> values,
        string value,
        string label)
    {
        if (!values.Contains(value))
        {
            throw new DataPackFailure(
                DataPackExitCode.ReferenceIntegrity,
                $"{label} reference does not resolve");
        }
    }

    private static void RequirePositive(JsonElement element, string property)
    {
        if (RequiredInt(element, property) <= 0)
        {
            throw new DataPackFailure(
                DataPackExitCode.ReferenceIntegrity,
                $"{property} must be positive");
        }
    }

    private static string RequiredString(JsonElement element, string property) =>
        element.GetProperty(property).GetString()
        ?? throw new JsonException($"missing string {property}");

    private static long RequiredInt(JsonElement element, string property) =>
        element.GetProperty(property).GetInt64();

    private static long CountLines(string path)
    {
        long count = 0;
        using var stream = File.OpenRead(path);
        var previous = -1;
        int current;
        while ((current = stream.ReadByte()) >= 0)
        {
            if (current == '\n')
            {
                count++;
            }
            previous = current;
        }
        if (previous >= 0 && previous != '\n')
        {
            count++;
        }
        return count;
    }

    private static void ValidateManifestShape(DataPackManifestV1 manifest)
    {
        if (manifest.SchemaMajor != CatalogContract.SchemaMajor
            || manifest.DistributionScope != "local_only"
            || !CatalogContract.IsCanonicalSha256(manifest.DatasetManifestId)
            || !manifest.Capabilities.Select(value => value.CapabilityId)
                .SequenceEqual(CatalogContract.RequiredCapabilities)
            || manifest.Capabilities.Any(value =>
                value.Status != CapabilityState.Complete
                || value.SourceIds.Count == 0)
            || !manifest.Files.Select(value => value.Path)
                .SequenceEqual(
                    CatalogContract.CapabilityFiles.Values
                        .Distinct(StringComparer.Ordinal)
                        .Order(StringComparer.Ordinal))
            || manifest.Files.Count > CatalogContract.MaximumFiles
            || manifest.Files.Any(file =>
                file.RowCount <= 0
                || file.FileSizeBytes <= 0
                || file.FileSizeBytes > CatalogContract.MaximumFileBytes
                || !CatalogContract.IsCanonicalSha256(file.Sha256)))
        {
            throw IntegrityFailure("manifest shape is invalid");
        }
        CatalogContract.RequireBuildId(manifest.GameBuildId);
    }

    private static bool CapabilitySequenceEqual(
        IReadOnlyList<CapabilityStatus> left,
        IReadOnlyList<CapabilityStatus> right) =>
        left.Count == right.Count
        && left.Zip(right).All(pair =>
            pair.First.CapabilityId == pair.Second.CapabilityId
            && pair.First.Status == pair.Second.Status
            && pair.First.SourceIds.SequenceEqual(pair.Second.SourceIds));

    private static DataPackFailure IntegrityFailure(string message) =>
        new(DataPackExitCode.PackageIntegrity, message);
}
