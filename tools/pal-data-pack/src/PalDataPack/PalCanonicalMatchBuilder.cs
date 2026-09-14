using System.Text.Json;
using System.Text.Json.Serialization;

namespace PalDataPack;

public sealed record PalCanonicalMatchBuildResult(
    string OutputPath,
    string OutputSha256,
    int CanonicalPalCount,
    int FullyMatchedPalCount,
    int UnmatchedRecordCount);

public sealed record PalCanonicalMatchSource(
    string SourceKind,
    string ReviewId,
    string Sha256);

public sealed record PalCanonicalMatchCounts(
    int SourcePalRowCount,
    int CanonicalPalCount,
    int FullyMatchedPalCount,
    int BreedingSpeciesCount,
    int SpecialBreedingRuleCount,
    int PaldexSpecialBreedingRuleCount,
    int CanonicalPalWithoutIconCount,
    int SourceRowOutsidePaldexCount,
    int IconOutsidePaldexCount,
    int ReportedLocalizationWithoutIconCount,
    int ReportedIconWithoutLocalizationCount,
    int UnmatchedRecordCount);

public sealed record PalCanonicalLocalizationMatch(
    string LocalizationKey,
    string NameKo,
    string? NameEn,
    string? DescriptionKo,
    string? DescriptionEn,
    string MatchKind);

public sealed record PalCanonicalIconMember(
    string IconLogicalId,
    string PackagePath,
    string ThumbnailRelativePath,
    string ThumbnailPngSha256);

public sealed record PalCanonicalIconMatch(
    string Status,
    string? MatchKind,
    string DistributionScope,
    IReadOnlyList<PalCanonicalIconMember> Members,
    string? Reason);

public sealed record PalCanonicalBreedingMatch(
    string Status,
    int? CombiRank,
    int? CombiDuplicatePriority,
    bool GeneralChildEligible,
    IReadOnlyList<string> ParentRuleIds,
    IReadOnlyList<string> ChildRuleIds);

public sealed record PalCanonicalSourceRowMatch(
    string SourceRowId,
    string LinkKind,
    string LocalizationKey,
    bool IsPal,
    bool IsBoss,
    bool IsRaidBoss,
    bool IsTowerBoss,
    bool IsPredator,
    string? Tribe,
    string IconSourceResolution,
    IReadOnlyList<string> IconTableRowKeys,
    IReadOnlyList<string> IconPackagePaths);

public sealed record PalCanonicalMatch(
    string InternalId,
    int PaldexNumber,
    string PaldexSuffix,
    string MatchStatus,
    string CanonicalSourceRowId,
    IReadOnlyList<string> Aliases,
    PalCanonicalLocalizationMatch Localization,
    PalCanonicalIconMatch Icon,
    PalCanonicalBreedingMatch Breeding,
    IReadOnlyList<PalCanonicalSourceRowMatch> SourceRows);

public sealed record PalCanonicalUnmatched(
    string Kind,
    string SubjectId,
    string? InternalId,
    string Reason,
    IReadOnlyList<string> RelatedIds);

public sealed record PalCanonicalMatchDocument(
    int SchemaVersion,
    string DatasetId,
    string GameBuildId,
    string MappingSha256,
    bool Verified,
    string DistributionScope,
    IReadOnlyList<string> LocalePriority,
    IReadOnlyList<PalCanonicalMatchSource> Sources,
    PalCanonicalMatchCounts Counts,
    IReadOnlyList<PalCanonicalMatch> Matches,
    IReadOnlyList<PalCanonicalUnmatched> Unmatched);

public static class PalCanonicalMatchBuilder
{
    private const int MaximumPalCatalogBytes = 32 * 1024 * 1024;
    private const int MaximumIconReportBytes = 8 * 1024 * 1024;
    private const int MaximumIconLinksBytes = 8 * 1024 * 1024;
    private const int MaximumOutputBytes = 32 * 1024 * 1024;

    private static readonly JsonSerializerOptions JsonOptions = new()
    {
        PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower,
        WriteIndented = true,
        UnmappedMemberHandling = JsonUnmappedMemberHandling.Disallow,
        MaxDepth = 64,
    };

    public static PalCanonicalMatchBuildResult BuildToFile(
        string palCatalogPath,
        string iconMatchReportPath,
        string palIconSourceLinksPath,
        string outputPath)
    {
        var palBytes = SecureInput.ReadBoundedRegularFile(
            palCatalogPath,
            MaximumPalCatalogBytes);
        var iconBytes = SecureInput.ReadBoundedRegularFile(
            iconMatchReportPath,
            MaximumIconReportBytes);
        var linkBytes = SecureInput.ReadBoundedRegularFile(
            palIconSourceLinksPath,
            MaximumIconLinksBytes);
        var pals = JsonSerializer.Deserialize<VerifiedPalBreedingDocument>(
                palBytes,
                JsonOptions)
            ?? throw Failure("verified Pal catalog is empty");
        var icons = JsonSerializer.Deserialize<GameIconLocalizationMatchReport>(
                iconBytes,
                JsonOptions)
            ?? throw Failure("game icon match report is empty");
        var links = JsonSerializer.Deserialize<PalIconSourceLinkReport>(
                linkBytes,
                JsonOptions)
            ?? throw Failure("Pal icon source-link report is empty");
        ValidateInputs(pals, icons, links, linkBytes);

        var sourceRowsById = UniqueBy(
            pals.SourcePalRows,
            row => row.SourceRowId,
            "Pal source row");
        var sourceIconLinksById = UniqueBy(
            links.ParameterRowLinks,
            link => link.RowKey,
            "Pal icon source row");
        var iconsById = UniqueBy(
            icons.EligiblePalIcons,
            icon => icon.InternalId,
            "eligible Pal icon",
            StringComparer.OrdinalIgnoreCase);
        var iconGroupsById = UniqueBy(
            icons.MultiIconGroups,
            group => group.InternalId,
            "multi-icon Pal group",
            StringComparer.OrdinalIgnoreCase);
        var breedingById = UniqueBy(
            pals.BreedingSpecies,
            species => species.InternalId,
            "breeding species",
            StringComparer.OrdinalIgnoreCase);
        var specialParentRules = pals.PaldexSpecialBreeding
            .SelectMany(rule => new[]
            {
                (InternalId: rule.ParentAInternalId, RuleId: rule.RuleId),
                (InternalId: rule.ParentBInternalId, RuleId: rule.RuleId),
            })
            .GroupBy(value => value.InternalId, StringComparer.OrdinalIgnoreCase)
            .ToDictionary(
                group => group.Key,
                group => (IReadOnlyList<string>)group
                    .Select(value => value.RuleId)
                    .Distinct(StringComparer.Ordinal)
                    .Order(StringComparer.Ordinal)
                    .ToArray(),
                StringComparer.OrdinalIgnoreCase);
        var specialChildRules = pals.PaldexSpecialBreeding
            .GroupBy(rule => rule.ChildInternalId, StringComparer.OrdinalIgnoreCase)
            .ToDictionary(
                group => group.Key,
                group => (IReadOnlyList<string>)group
                    .Select(rule => rule.RuleId)
                    .Distinct(StringComparer.Ordinal)
                    .Order(StringComparer.Ordinal)
                    .ToArray(),
                StringComparer.OrdinalIgnoreCase);

        var canonicalMatches = new List<PalCanonicalMatch>(pals.Pals.Count);
        var associatedSourceRows = new HashSet<string>(StringComparer.Ordinal);
        foreach (var pal in pals.Pals.OrderBy(
                     pal => pal.InternalId,
                     StringComparer.Ordinal))
        {
            if (pal.PaldexNumber <= 0 || string.IsNullOrWhiteSpace(pal.NameKo))
            {
                throw Failure(
                    $"canonical Pal is not a Paldex record: {pal.InternalId}");
            }
            var rawLinks = new List<PalCanonicalSourceRowMatch>(
                pal.SourceRowIds.Count);
            foreach (var sourceRowId in pal.SourceRowIds)
            {
                if (!sourceRowsById.TryGetValue(sourceRowId, out var sourceRow))
                {
                    throw Failure(
                        $"canonical Pal references an unknown source row: "
                        + $"{pal.InternalId} -> {sourceRowId}");
                }
                associatedSourceRows.Add(sourceRowId);
                sourceIconLinksById.TryGetValue(sourceRowId, out var iconLink);
                rawLinks.Add(new PalCanonicalSourceRowMatch(
                    sourceRowId,
                    string.Equals(
                        sourceRowId,
                        pal.InternalId,
                        StringComparison.Ordinal)
                        ? "canonical_exact"
                        : "same_internal_id_variant",
                    sourceRow.LocalizationKey,
                    sourceRow.IsPal,
                    sourceRow.IsBoss,
                    sourceRow.IsRaidBoss,
                    sourceRow.IsTowerBoss,
                    sourceRow.IsPredator,
                    iconLink?.Tribe,
                    iconLink?.Resolution ?? "not_in_is_pal_icon_projection",
                    iconLink?.IconTableRowKeys ?? [],
                    iconLink?.PackagePaths ?? []));
            }

            var icon = BuildIconMatch(
                pal.InternalId,
                icons.DistributionScope,
                iconsById,
                iconGroupsById);
            breedingById.TryGetValue(pal.InternalId, out var breeding);
            specialParentRules.TryGetValue(
                pal.InternalId,
                out var parentRuleIds);
            specialChildRules.TryGetValue(
                pal.InternalId,
                out var childRuleIds);
            var breedingMatch = new PalCanonicalBreedingMatch(
                breeding is null
                    ? "not_breedable"
                    : breeding.IgnoreCombi
                        ? "eligible_parent_special_child_only"
                        : "eligible_parent_and_general_child",
                breeding?.CombiRank,
                breeding?.CombiDuplicatePriority,
                breeding is { IgnoreCombi: false },
                parentRuleIds ?? [],
                childRuleIds ?? []);
            canonicalMatches.Add(new PalCanonicalMatch(
                pal.InternalId,
                pal.PaldexNumber,
                pal.PaldexSuffix,
                icon.Status == "matched" ? "fully_matched" : "partial",
                pal.SourceRowId,
                pal.SourceRowIds
                    .Where(id => !string.Equals(
                        id,
                        pal.SourceRowId,
                        StringComparison.Ordinal))
                    .Order(StringComparer.Ordinal)
                    .ToArray(),
                new PalCanonicalLocalizationMatch(
                    pal.LocalizationKey,
                    pal.NameKo,
                    pal.NameEn,
                    pal.DescriptionKo,
                    pal.DescriptionEn,
                    "installed_game_localization_key_case_insensitive"),
                icon,
                breedingMatch,
                rawLinks
                    .OrderBy(link => link.SourceRowId, StringComparer.Ordinal)
                    .ToArray()));
        }

        var canonicalIds = canonicalMatches
            .Select(match => match.InternalId)
            .ToHashSet(StringComparer.OrdinalIgnoreCase);
        var unmatched = BuildUnmatched(
            pals,
            icons,
            associatedSourceRows,
            canonicalIds,
            sourceRowsById);
        var fullyMatchedCount = canonicalMatches.Count(match =>
            match.MatchStatus == "fully_matched");
        var counts = new PalCanonicalMatchCounts(
            pals.SourcePalRows.Count,
            canonicalMatches.Count,
            fullyMatchedCount,
            pals.BreedingSpecies.Count,
            pals.SpecialBreeding.Count,
            pals.PaldexSpecialBreeding.Count,
            canonicalMatches.Count(match => match.Icon.Status != "matched"),
            unmatched.Count(match =>
                match.Kind == "source_row_outside_paldex"),
            unmatched.Count(match =>
                match.Kind == "matched_icon_outside_paldex"),
            icons.KoreanLocalizationWithoutIconCount,
            icons.IconWithoutKoreanLocalizationCount,
            unmatched.Count);
        var build = NormalizeBuildId(pals.GameBuildId);
        var document = new PalCanonicalMatchDocument(
            SchemaVersion: 1,
            DatasetId: $"palbeacon:pal-canonical-matches:steam:{build}",
            GameBuildId: $"steam:{build}",
            pals.MappingSha256,
            Verified: true,
            DistributionScope:
                "metadata_committable_original_icon_files_local_windows_only",
            LocalePriority: ["ko", "en"],
            Sources:
            [
                new PalCanonicalMatchSource(
                    "verified_pal_breeding",
                    pals.ContractReviewId,
                    Hashing.Sha256Hex(palBytes)),
                new PalCanonicalMatchSource(
                    "game_icon_match_report",
                    "derived:game-icon-match",
                    Hashing.Sha256Hex(iconBytes)),
                new PalCanonicalMatchSource(
                    "pal_icon_source_links",
                    links.ReviewId,
                    Hashing.Sha256Hex(linkBytes)),
            ],
            counts,
            canonicalMatches,
            unmatched);
        var outputBytes = JsonSerializer.SerializeToUtf8Bytes(document, JsonOptions);
        if (outputBytes.Length > MaximumOutputBytes)
        {
            throw Failure("canonical Pal matching table exceeds its size bound");
        }
        var fullOutputPath = AtomicWrite(outputPath, outputBytes);
        return new PalCanonicalMatchBuildResult(
            fullOutputPath,
            Hashing.Sha256Hex(outputBytes),
            canonicalMatches.Count,
            fullyMatchedCount,
            unmatched.Count);
    }

    private static PalCanonicalIconMatch BuildIconMatch(
        string internalId,
        string distributionScope,
        IReadOnlyDictionary<string, LocalizedPalIconMatch> iconsById,
        IReadOnlyDictionary<string, LocalizedPalIconGroupMatch> groupsById)
    {
        if (iconsById.TryGetValue(internalId, out var icon))
        {
            return new PalCanonicalIconMatch(
                "matched",
                icon.MatchKind,
                distributionScope,
                [
                    new PalCanonicalIconMember(
                        icon.IconLogicalId,
                        icon.PackagePath,
                        icon.ThumbnailRelativePath,
                        icon.ThumbnailPngSha256),
                ],
                null);
        }
        if (groupsById.TryGetValue(internalId, out var group))
        {
            return new PalCanonicalIconMatch(
                "matched",
                group.MatchKind,
                distributionScope,
                group.Members
                    .Select(member => new PalCanonicalIconMember(
                        member.IconLogicalId,
                        member.PackagePath,
                        member.ThumbnailRelativePath,
                        member.ThumbnailPngSha256))
                    .OrderBy(member => member.IconLogicalId, StringComparer.Ordinal)
                    .ToArray(),
                null);
        }
        return new PalCanonicalIconMatch(
            "unmatched",
            null,
            distributionScope,
            [],
            "no_reviewed_exact_build_icon_link");
    }

    private static IReadOnlyList<PalCanonicalUnmatched> BuildUnmatched(
        VerifiedPalBreedingDocument pals,
        GameIconLocalizationMatchReport icons,
        IReadOnlySet<string> associatedSourceRows,
        IReadOnlySet<string> canonicalIds,
        IReadOnlyDictionary<string, VerifiedPalSourceRecord> sourceRowsById)
    {
        var output = new List<PalCanonicalUnmatched>();
        foreach (var sourceRow in pals.SourcePalRows.Where(row =>
                     !associatedSourceRows.Contains(row.SourceRowId)))
        {
            var reason = !sourceRow.IsPal
                ? "is_pal_false"
                : sourceRow.PaldexNumber <= 0
                    ? "paldex_number_not_positive"
                    : sourceRow.NameKo is null
                        ? "korean_localization_missing"
                        : "not_selected_by_canonical_projection";
            output.Add(new PalCanonicalUnmatched(
                "source_row_outside_paldex",
                sourceRow.SourceRowId,
                sourceRow.InternalId,
                reason,
                [sourceRow.LocalizationKey]));
        }
        foreach (var icon in icons.EligiblePalIcons.Where(icon =>
                     !canonicalIds.Contains(icon.InternalId)))
        {
            output.Add(new PalCanonicalUnmatched(
                "matched_icon_outside_paldex",
                icon.IconLogicalId,
                icon.InternalId,
                "localized_icon_has_no_canonical_paldex_record",
                [icon.PackagePath]));
        }
        foreach (var group in icons.MultiIconGroups.Where(group =>
                     !canonicalIds.Contains(group.InternalId)))
        {
            output.Add(new PalCanonicalUnmatched(
                "matched_icon_outside_paldex",
                group.InternalId,
                group.InternalId,
                "localized_icon_group_has_no_canonical_paldex_record",
                group.Members
                    .Select(member => member.IconLogicalId)
                    .Order(StringComparer.Ordinal)
                    .ToArray()));
        }
        foreach (var missing in icons.KoreanLocalizationsWithoutIcon)
        {
            output.Add(new PalCanonicalUnmatched(
                "localized_name_without_icon",
                missing.InternalId,
                missing.InternalId,
                missing.Reason,
                missing.SourceRowKeys
                    .Concat(missing.Tribes)
                    .Distinct(StringComparer.Ordinal)
                    .Order(StringComparer.Ordinal)
                    .ToArray()));
        }
        foreach (var icon in icons.IconsWithoutKoreanLocalization)
        {
            output.Add(new PalCanonicalUnmatched(
                "icon_without_korean_localization",
                icon.IconLogicalId,
                null,
                "reviewed_portrait_has_no_verified_korean_localization_link",
                [icon.PackagePath]));
        }
        var paldexRuleIds = pals.PaldexSpecialBreeding
            .Select(rule => rule.RuleId)
            .ToHashSet(StringComparer.Ordinal);
        foreach (var rule in pals.SpecialBreeding.Where(rule =>
                     !paldexRuleIds.Contains(rule.RuleId)))
        {
            output.Add(new PalCanonicalUnmatched(
                "special_rule_outside_paldex",
                rule.RuleId,
                rule.ChildInternalId,
                "one_or_more_rule_species_are_not_canonical_paldex_records",
                [
                    rule.ParentAInternalId,
                    rule.ParentBInternalId,
                    rule.ChildInternalId,
                    .. rule.SourceRowIds,
                ]));
        }
        foreach (var pal in pals.Pals.Where(pal =>
                     !icons.EligiblePalIcons.Any(icon => string.Equals(
                         icon.InternalId,
                         pal.InternalId,
                         StringComparison.OrdinalIgnoreCase))
                     && !icons.MultiIconGroups.Any(group => string.Equals(
                         group.InternalId,
                         pal.InternalId,
                         StringComparison.OrdinalIgnoreCase))))
        {
            output.Add(new PalCanonicalUnmatched(
                "canonical_paldex_without_icon",
                pal.InternalId,
                pal.InternalId,
                "no_reviewed_exact_build_icon_link",
                pal.SourceRowIds));
        }
        return output
            .OrderBy(match => match.Kind, StringComparer.Ordinal)
            .ThenBy(match => match.SubjectId, StringComparer.Ordinal)
            .ToArray();
    }

    private static void ValidateInputs(
        VerifiedPalBreedingDocument pals,
        GameIconLocalizationMatchReport icons,
        PalIconSourceLinkReport links,
        ReadOnlySpan<byte> linkBytes)
    {
        var palBuild = NormalizeBuildId(pals.GameBuildId);
        var iconBuild = NormalizeBuildId(icons.GameBuildId);
        var linkBuild = NormalizeBuildId(links.GameBuildId);
        if (pals.SchemaVersion != 1
            || !pals.Verified
            || icons.SchemaMajor != CatalogContract.SchemaMajor
            || !icons.Verified
            || links.SchemaMajor != CatalogContract.SchemaMajor
            || !links.Verified
            || !string.Equals(palBuild, iconBuild, StringComparison.Ordinal)
            || !string.Equals(palBuild, linkBuild, StringComparison.Ordinal)
            || !string.Equals(
                pals.MappingSha256,
                icons.MappingSha256,
                StringComparison.Ordinal)
            || !string.Equals(
                pals.MappingSha256,
                links.MappingSha256,
                StringComparison.Ordinal)
            || !string.Equals(
                icons.PalIconSourceLinksSha256,
                Hashing.Sha256Hex(linkBytes),
                StringComparison.Ordinal)
            || links.ParameterTableRowCount != pals.SourcePalRows.Count
            || !string.Equals(
                links.SourceKind,
                "installed_game_data_table",
                StringComparison.Ordinal)
            || !string.Equals(
                icons.DistributionScope,
                "local_windows_only",
                StringComparison.Ordinal))
        {
            throw Failure(
                "canonical Pal matching requires compatible reviewed exact-Build inputs");
        }
        var canonicalIds = pals.Pals
            .Select(pal => pal.InternalId)
            .ToHashSet(StringComparer.OrdinalIgnoreCase);
        var sourceSpeciesIds = pals.SourcePalRows
            .Where(row => row.IsPal)
            .Select(row => row.InternalId)
            .ToHashSet(StringComparer.OrdinalIgnoreCase);
        if (pals.SpecialBreeding.Any(rule =>
                !sourceSpeciesIds.Contains(rule.ParentAInternalId)
                || !sourceSpeciesIds.Contains(rule.ParentBInternalId)
                || !sourceSpeciesIds.Contains(rule.ChildInternalId))
            || pals.PaldexSpecialBreeding.Any(rule =>
                !canonicalIds.Contains(rule.ParentAInternalId)
                || !canonicalIds.Contains(rule.ParentBInternalId)
                || !canonicalIds.Contains(rule.ChildInternalId)))
        {
            throw Failure(
                "special breeding projection contains an unknown Pal");
        }
    }

    private static Dictionary<string, T> UniqueBy<T>(
        IEnumerable<T> values,
        Func<T, string> keySelector,
        string subject,
        IEqualityComparer<string>? comparer = null)
    {
        var output = new Dictionary<string, T>(
            comparer ?? StringComparer.Ordinal);
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
            ?? throw Failure("canonical Pal output path has no parent");
        Directory.CreateDirectory(directory);
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
}
