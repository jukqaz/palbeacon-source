using System.Text.Json;
using System.Text.Json.Serialization;

namespace PalDataPack;

public sealed record PalRelationMatchBuildResult(
    string OutputPath,
    string OutputSha256,
    int DropSourceIdCount,
    int DropCanonicalMatchCount,
    int SpawnSourceIdCount,
    int SpawnCanonicalMatchCount);

public sealed record PalRelationMatchSource(
    string SourceKind,
    string ReviewId,
    string Sha256);

public sealed record PalRelationMatchCounts(
    int DropRelationCount,
    int DropSourceIdCount,
    int DropCanonicalExactCount,
    int DropCanonicalCaseOnlyCount,
    int DropNonPaldexSourceCount,
    int DropHumanSourceCount,
    int DropHumanBossAliasCount,
    int DropNotInPalParameterCount,
    int DropSentinelCount,
    int SpawnRuleCount,
    int SpawnSourceIdCount,
    int SpawnCanonicalExactCount,
    int SpawnCanonicalCaseOnlyCount,
    int SpawnNonPaldexSourceCount,
    int SpawnHumanSourceCount,
    int SpawnHumanBossAliasCount,
    int SpawnNotInPalParameterCount,
    int SpawnSentinelCount);

public sealed record PalRelationSourceMatch(
    string SourceId,
    string Status,
    string? MatchedSourceRowId,
    string? CanonicalInternalId,
    string? NameKo,
    int RelationCount,
    string Reason,
    string? NameEn = null,
    string? IconPackagePath = null,
    string VerificationStatus = "verified_exact_build");

public sealed record PalRelationMatchDocument(
    int SchemaVersion,
    string DatasetId,
    string GameBuildId,
    string MappingSha256,
    bool Verified,
    string MatchPolicy,
    IReadOnlyList<PalRelationMatchSource> Sources,
    PalRelationMatchCounts Counts,
    IReadOnlyList<PalRelationSourceMatch> DropSources,
    IReadOnlyList<PalRelationSourceMatch> SpawnSources);

public static class PalRelationMatchBuilder
{
    private const int MaximumPalCatalogBytes = 32 * 1024 * 1024;
    private const int MaximumHumanCatalogBytes = 8 * 1024 * 1024;
    private const int MaximumItemCatalogBytes = 32 * 1024 * 1024;
    private const int MaximumSpawnSearchBytes = 16 * 1024 * 1024;
    private const int MaximumOutputBytes = 8 * 1024 * 1024;

    private static readonly JsonSerializerOptions JsonOptions = new()
    {
        PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower,
        WriteIndented = true,
        UnmappedMemberHandling = JsonUnmappedMemberHandling.Disallow,
        MaxDepth = 64,
    };

    public static PalRelationMatchBuildResult BuildToFile(
        string palCatalogPath,
        string humanCatalogPath,
        string itemCatalogPath,
        string spawnSearchPath,
        string outputPath)
    {
        var palBytes = SecureInput.ReadBoundedRegularFile(
            palCatalogPath,
            MaximumPalCatalogBytes);
        var humanBytes = SecureInput.ReadBoundedRegularFile(
            humanCatalogPath,
            MaximumHumanCatalogBytes);
        var itemBytes = SecureInput.ReadBoundedRegularFile(
            itemCatalogPath,
            MaximumItemCatalogBytes);
        var spawnBytes = SecureInput.ReadBoundedRegularFile(
            spawnSearchPath,
            MaximumSpawnSearchBytes);
        var pals = JsonSerializer.Deserialize<VerifiedPalBreedingDocument>(
                palBytes,
                JsonOptions)
            ?? throw Failure("verified Pal catalog is empty");
        var humans = JsonSerializer.Deserialize<VerifiedHumanCatalogDocument>(
                humanBytes,
                JsonOptions)
            ?? throw Failure("verified human catalog is empty");
        var items = JsonSerializer.Deserialize<VerifiedItemCatalogDocument>(
                itemBytes,
                JsonOptions)
            ?? throw Failure("verified item catalog is empty");
        using var spawnDocument = JsonDocument.Parse(
            spawnBytes,
            new JsonDocumentOptions
            {
                AllowTrailingCommas = false,
                CommentHandling = JsonCommentHandling.Disallow,
                MaxDepth = 64,
            });
        ValidateInputs(pals, humans, items, spawnDocument.RootElement);

        var resolver = BuildResolver(pals, humans);
        var dropCounts = items.PalDrops
            .GroupBy(drop => drop.PalId, StringComparer.Ordinal)
            .ToDictionary(
                group => group.Key,
                group => group.Count(),
                StringComparer.Ordinal);
        var spawnCounts = ReadSpawnCounts(spawnDocument.RootElement);
        var dropMatches = BuildMatches(dropCounts, resolver, "drop");
        var spawnMatches = BuildMatches(spawnCounts, resolver, "spawn");
        var counts = new PalRelationMatchCounts(
            items.PalDrops.Count,
            dropMatches.Count,
            Count(dropMatches, "canonical_exact"),
            Count(dropMatches, "canonical_case_only"),
            Count(dropMatches, "non_paldex_source_exact")
                + Count(dropMatches, "non_paldex_source_case_only"),
            Count(dropMatches, "human_source_exact")
                + Count(dropMatches, "human_source_case_only"),
            Count(dropMatches, "human_boss_alias_reviewed"),
            Count(dropMatches, "not_in_pal_parameter"),
            Count(dropMatches, "sentinel"),
            spawnCounts.Values.Sum(),
            spawnMatches.Count,
            Count(spawnMatches, "canonical_exact"),
            Count(spawnMatches, "canonical_case_only"),
            Count(spawnMatches, "non_paldex_source_exact")
                + Count(spawnMatches, "non_paldex_source_case_only"),
            Count(spawnMatches, "human_source_exact")
                + Count(spawnMatches, "human_source_case_only"),
            Count(spawnMatches, "human_boss_alias_reviewed"),
            Count(spawnMatches, "not_in_pal_parameter"),
            Count(spawnMatches, "sentinel"));
        var buildId = NormalizeBuildId(pals.GameBuildId);
        var document = new PalRelationMatchDocument(
            SchemaVersion: 2,
            DatasetId: $"palbeacon:pal-relation-matches:steam:{buildId}",
            GameBuildId: $"steam:{buildId}",
            pals.MappingSha256,
            Verified: true,
            MatchPolicy:
                "exact source-row ID first, then one unique Unreal FName-style "
                + "case-only source-row match; the only non-exact rule is a "
                + "reviewed BOSS_ human variant alias when the exact base human "
                + "row exists; no other prefix stripping or suffix guessing",
            Sources:
            [
                new PalRelationMatchSource(
                    "verified_pal_breeding",
                    pals.ContractReviewId,
                    Hashing.Sha256Hex(palBytes)),
                new PalRelationMatchSource(
                    "verified_human_catalog",
                    humans.ContractReviewId,
                    Hashing.Sha256Hex(humanBytes)),
                new PalRelationMatchSource(
                    "verified_item_catalog",
                    items.ContractReviewId,
                    Hashing.Sha256Hex(itemBytes)),
                new PalRelationMatchSource(
                    "verified_spawn_search_index",
                    $"derived:index-map-spawns:{buildId}",
                    Hashing.Sha256Hex(spawnBytes)),
            ],
            counts,
            dropMatches,
            spawnMatches);
        var outputBytes = JsonSerializer.SerializeToUtf8Bytes(document, JsonOptions);
        if (outputBytes.Length is < 1 or > MaximumOutputBytes)
        {
            throw Failure("Pal relation matching table exceeds its size bound");
        }
        var fullOutputPath = AtomicWrite(outputPath, outputBytes);
        return new PalRelationMatchBuildResult(
            fullOutputPath,
            Hashing.Sha256Hex(outputBytes),
            dropMatches.Count,
            CountCanonical(dropMatches),
            spawnMatches.Count,
            CountCanonical(spawnMatches));
    }

    private static PalRelationResolver BuildResolver(
        VerifiedPalBreedingDocument pals,
        VerifiedHumanCatalogDocument humans)
    {
        var sourceExact = UniqueBy(
            pals.SourcePalRows,
            row => row.SourceRowId,
            "Pal source row");
        var sourceCase = UniqueCaseInsensitiveBy(
            pals.SourcePalRows,
            row => row.SourceRowId,
            "Pal source row");
        var canonicalExact = new Dictionary<string, CanonicalTarget>(
            StringComparer.Ordinal);
        var canonicalCase = new Dictionary<string, CanonicalTarget>(
            StringComparer.OrdinalIgnoreCase);
        foreach (var pal in pals.Pals)
        {
            foreach (var sourceRowId in pal.SourceRowIds)
            {
                if (!sourceExact.ContainsKey(sourceRowId))
                {
                    throw Failure(
                        $"canonical Pal references an unknown source row: "
                        + $"{pal.InternalId} -> {sourceRowId}");
                }
                var target = new CanonicalTarget(
                    sourceRowId,
                    pal.InternalId,
                    pal.NameKo);
                if (!canonicalExact.TryAdd(sourceRowId, target))
                {
                    throw Failure(
                        $"canonical Pal source row is not unique: {sourceRowId}");
                }
                if (canonicalCase.TryGetValue(sourceRowId, out var previous)
                    && !string.Equals(
                        previous.SourceRowId,
                        sourceRowId,
                        StringComparison.Ordinal))
                {
                    throw Failure(
                        $"canonical Pal source rows have a case-only collision: "
                        + $"{previous.SourceRowId} / {sourceRowId}");
                }
                canonicalCase[sourceRowId] = target;
            }
        }
        var humanExact = UniqueBy(
            humans.Humans,
            human => human.SourceRowId,
            "human source row");
        var humanCase = UniqueCaseInsensitiveBy(
            humans.Humans,
            human => human.SourceRowId,
            "human source row");
        return new PalRelationResolver(
            canonicalExact,
            canonicalCase,
            sourceExact,
            sourceCase,
            humanExact,
            humanCase);
    }

    private static IReadOnlyDictionary<string, int> ReadSpawnCounts(
        JsonElement root)
    {
        var species = RequiredProperty(root, "species");
        if (species.ValueKind != JsonValueKind.Object)
        {
            throw Failure("spawn search species is not an object");
        }
        var output = new SortedDictionary<string, int>(StringComparer.Ordinal);
        foreach (var property in species.EnumerateObject())
        {
            if (string.IsNullOrWhiteSpace(property.Name)
                || property.Value.ValueKind != JsonValueKind.Array
                || !output.TryAdd(
                    property.Name,
                    property.Value.GetArrayLength()))
            {
                throw Failure(
                    $"spawn search species entry is invalid: {property.Name}");
            }
            foreach (var rule in property.Value.EnumerateArray())
            {
                if (rule.ValueKind != JsonValueKind.Array
                    || rule.GetArrayLength() != 10)
                {
                    throw Failure(
                        $"spawn search rule is invalid: {property.Name}");
                }
            }
        }
        var declaredCount = RequiredProperty(root, "species_count").GetInt32();
        if (declaredCount != output.Count)
        {
            throw Failure("spawn search species count does not match its payload");
        }
        return output;
    }

    private static IReadOnlyList<PalRelationSourceMatch> BuildMatches(
        IReadOnlyDictionary<string, int> sourceCounts,
        PalRelationResolver resolver,
        string relationKind)
    {
        var output = new List<PalRelationSourceMatch>(sourceCounts.Count);
        foreach (var (sourceId, relationCount) in sourceCounts.OrderBy(
                     pair => pair.Key,
                     StringComparer.Ordinal))
        {
            if (resolver.CanonicalExact.TryGetValue(sourceId, out var exact))
            {
                output.Add(CanonicalMatch(
                    sourceId,
                    relationCount,
                    exact,
                    "canonical_exact",
                    "exact installed-game source-row ID"));
                continue;
            }
            if (resolver.CanonicalCase.TryGetValue(sourceId, out var caseOnly))
            {
                output.Add(CanonicalMatch(
                    sourceId,
                    relationCount,
                    caseOnly,
                    "canonical_case_only",
                    "unique Unreal FName-style case-only source-row match"));
                continue;
            }
            if (resolver.SourceExact.TryGetValue(sourceId, out var sourceExact))
            {
                output.Add(NonPaldexMatch(
                    sourceId,
                    relationCount,
                    sourceExact,
                    "non_paldex_source_exact",
                    "exact installed-game Pal source row is outside the "
                    + "positive-Paldex projection"));
                continue;
            }
            if (resolver.SourceCase.TryGetValue(sourceId, out var sourceCase))
            {
                output.Add(NonPaldexMatch(
                    sourceId,
                    relationCount,
                    sourceCase,
                    "non_paldex_source_case_only",
                    "unique Unreal FName-style case-only Pal source row is "
                    + "outside the positive-Paldex projection"));
                continue;
            }
            if (resolver.HumanExact.TryGetValue(sourceId, out var humanExact))
            {
                output.Add(HumanMatch(
                    sourceId,
                    relationCount,
                    humanExact,
                    "human_source_exact",
                    "exact installed-game human source row"));
                continue;
            }
            if (resolver.HumanCase.TryGetValue(sourceId, out var humanCase))
            {
                output.Add(HumanMatch(
                    sourceId,
                    relationCount,
                    humanCase,
                    "human_source_case_only",
                    "unique Unreal FName-style case-only human source-row match"));
                continue;
            }
            if (sourceId.StartsWith("BOSS_", StringComparison.Ordinal)
                && resolver.HumanExact.TryGetValue(
                    sourceId["BOSS_".Length..],
                    out var humanBossBase))
            {
                output.Add(HumanMatch(
                    sourceId,
                    relationCount,
                    humanBossBase,
                    "human_boss_alias_reviewed",
                    "reviewed human boss-variant alias; exact base human row "
                    + "exists but the variant has no direct parameter row",
                    "reviewed_inference"));
                continue;
            }
            if (relationKind == "spawn"
                && string.Equals(
                    sourceId,
                    "RowName",
                    StringComparison.Ordinal))
            {
                output.Add(new PalRelationSourceMatch(
                    sourceId,
                    "sentinel",
                    null,
                    null,
                    null,
                    relationCount,
                    "source field-name sentinel is not a character ID"));
                continue;
            }
            output.Add(new PalRelationSourceMatch(
                sourceId,
                "not_in_pal_parameter",
                null,
                null,
                null,
                relationCount,
                "source ID is not present in DT_PalMonsterParameter; do not "
                + "guess a Paldex link"));
        }
        return output;
    }

    private static PalRelationSourceMatch CanonicalMatch(
        string sourceId,
        int relationCount,
        CanonicalTarget target,
        string status,
        string reason) =>
        new(
            sourceId,
            status,
            target.SourceRowId,
            target.InternalId,
            target.NameKo,
            relationCount,
            reason);

    private static PalRelationSourceMatch NonPaldexMatch(
        string sourceId,
        int relationCount,
        VerifiedPalSourceRecord source,
        string status,
        string reason) =>
        new(
            sourceId,
            status,
            source.SourceRowId,
            null,
            source.NameKo,
            relationCount,
            reason);

    private static PalRelationSourceMatch HumanMatch(
        string sourceId,
        int relationCount,
        VerifiedHumanCatalogRecord human,
        string status,
        string reason,
        string verificationStatus = "verified_exact_build") =>
        new(
            sourceId,
            status,
            human.SourceRowId,
            null,
            human.NameKo,
            relationCount,
            reason,
            human.NameEn,
            human.IconPackagePath,
            verificationStatus);

    private static void ValidateInputs(
        VerifiedPalBreedingDocument pals,
        VerifiedHumanCatalogDocument humans,
        VerifiedItemCatalogDocument items,
        JsonElement spawnRoot)
    {
        var palBuild = NormalizeBuildId(pals.GameBuildId);
        var humanBuild = NormalizeBuildId(humans.GameBuildId);
        var itemBuild = NormalizeBuildId(items.GameBuildId);
        var spawnBuild = NormalizeBuildId(
            RequiredProperty(spawnRoot, "game_build_id").GetString()
                ?? string.Empty);
        if (pals.SchemaVersion != 1
            || !pals.Verified
            || humans.SchemaVersion != 1
            || !humans.Verified
            || items.SchemaVersion != 1
            || !items.Verified
            || RequiredProperty(spawnRoot, "schema_version").GetInt32() != 1
            || !RequiredProperty(spawnRoot, "verified").GetBoolean()
            || !string.Equals(palBuild, itemBuild, StringComparison.Ordinal)
            || !string.Equals(palBuild, humanBuild, StringComparison.Ordinal)
            || !string.Equals(palBuild, spawnBuild, StringComparison.Ordinal)
            || !string.Equals(
                pals.MappingSha256,
                items.MappingSha256,
                StringComparison.Ordinal)
            || !string.Equals(
                pals.MappingSha256,
                humans.MappingSha256,
                StringComparison.Ordinal)
            || !CatalogContract.IsCanonicalSha256(pals.MappingSha256))
        {
            throw Failure(
                "Pal relation matching requires compatible reviewed exact-Build inputs");
        }
    }

    private static JsonElement RequiredProperty(
        JsonElement element,
        string propertyName)
    {
        if (element.ValueKind != JsonValueKind.Object
            || !element.TryGetProperty(propertyName, out var value))
        {
            throw Failure($"required JSON property is missing: {propertyName}");
        }
        return value;
    }

    private static Dictionary<string, T> UniqueBy<T>(
        IEnumerable<T> values,
        Func<T, string> keySelector,
        string subject)
    {
        var output = new Dictionary<string, T>(StringComparer.Ordinal);
        foreach (var value in values)
        {
            var key = keySelector(value);
            if (!output.TryAdd(key, value))
            {
                throw Failure($"{subject} ID is not unique: {key}");
            }
        }
        return output;
    }

    private static Dictionary<string, T> UniqueCaseInsensitiveBy<T>(
        IEnumerable<T> values,
        Func<T, string> keySelector,
        string subject)
    {
        var output = new Dictionary<string, T>(StringComparer.OrdinalIgnoreCase);
        var originalKeys = new Dictionary<string, string>(
            StringComparer.OrdinalIgnoreCase);
        foreach (var value in values)
        {
            var key = keySelector(value);
            if (originalKeys.TryGetValue(key, out var previous)
                && !string.Equals(previous, key, StringComparison.Ordinal))
            {
                throw Failure(
                    $"{subject} IDs have a case-only collision: "
                    + $"{previous} / {key}");
            }
            originalKeys[key] = key;
            output[key] = value;
        }
        return output;
    }

    private static int Count(
        IReadOnlyList<PalRelationSourceMatch> matches,
        string status) =>
        matches.Count(match => string.Equals(
            match.Status,
            status,
            StringComparison.Ordinal));

    private static int CountCanonical(
        IReadOnlyList<PalRelationSourceMatch> matches) =>
        matches.Count(match =>
            match.Status is "canonical_exact" or "canonical_case_only");

    private static string NormalizeBuildId(string buildId) =>
        buildId.StartsWith("steam:", StringComparison.Ordinal)
            ? buildId["steam:".Length..]
            : buildId;

    private static string AtomicWrite(
        string outputPath,
        ReadOnlySpan<byte> bytes)
    {
        var fullPath = Path.GetFullPath(outputPath);
        var directory = Path.GetDirectoryName(fullPath)
            ?? throw Failure("Pal relation output path has no parent");
        Directory.CreateDirectory(directory);
        SecureInput.EnsurePlainDirectory(directory);
        var temporaryPath = Path.Combine(
            directory,
            $".{Path.GetFileName(fullPath)}.{Guid.NewGuid():N}.tmp");
        try
        {
            using (var stream = new FileStream(
                       temporaryPath,
                       FileMode.CreateNew,
                       FileAccess.Write,
                       FileShare.None,
                       64 * 1024,
                       FileOptions.WriteThrough))
            {
                stream.Write(bytes);
                stream.Flush(flushToDisk: true);
            }
            File.Move(temporaryPath, fullPath, overwrite: true);
            return fullPath;
        }
        finally
        {
            if (File.Exists(temporaryPath))
            {
                File.Delete(temporaryPath);
            }
        }
    }

    private static DataPackFailure Failure(string message) => new(
        DataPackExitCode.ReferenceIntegrity,
        message);

    private sealed record CanonicalTarget(
        string SourceRowId,
        string InternalId,
        string NameKo);

    private sealed record PalRelationResolver(
        IReadOnlyDictionary<string, CanonicalTarget> CanonicalExact,
        IReadOnlyDictionary<string, CanonicalTarget> CanonicalCase,
        IReadOnlyDictionary<string, VerifiedPalSourceRecord> SourceExact,
        IReadOnlyDictionary<string, VerifiedPalSourceRecord> SourceCase,
        IReadOnlyDictionary<string, VerifiedHumanCatalogRecord> HumanExact,
        IReadOnlyDictionary<string, VerifiedHumanCatalogRecord> HumanCase);
}
