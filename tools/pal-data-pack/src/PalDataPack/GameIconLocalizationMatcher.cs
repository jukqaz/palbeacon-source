using System.Text.Json;
using System.Text.Json.Serialization;

namespace PalDataPack;

public sealed record KoreanPalLocalization(
    [property: JsonPropertyName("localized_name")] string LocalizedName,
    [property: JsonPropertyName("description")] string Description);

public sealed record LocalizedPalIconMatch(
    string InternalId,
    string KoreanName,
    string IconLogicalId,
    string MatchKind,
    string PackagePath,
    string ThumbnailRelativePath,
    string ThumbnailPngSha256);

public sealed record KoreanPalWithoutIcon(
    string InternalId,
    string KoreanName,
    string Reason,
    IReadOnlyList<string> SourceRowKeys,
    IReadOnlyList<string> Tribes);

public sealed record LocalizedPalIconGroupMember(
    string SourceRowKey,
    string Tribe,
    string IconLogicalId,
    string PackagePath,
    string ThumbnailRelativePath,
    string ThumbnailPngSha256);

public sealed record LocalizedPalIconGroupMatch(
    string InternalId,
    string KoreanName,
    string LocalizationKey,
    string MatchKind,
    IReadOnlyList<LocalizedPalIconGroupMember> Members);

public sealed record UnlocalizedPalIcon(
    string IconLogicalId,
    string PackagePath,
    string ThumbnailRelativePath,
    string ThumbnailPngSha256);

public sealed record DuplicateGameIconAssetGroup(
    string SourceDecodedRgbaSha256,
    IReadOnlyList<string> GroupAndLogicalIds);

public sealed record GameIconLocalizationMatchReport(
    int SchemaMajor,
    string GameBuildId,
    string MappingSha256,
    string IconManifestSha256,
    string KoreanPalCatalogSha256,
    string PalIconSourceLinksSha256,
    string PrimaryLocale,
    bool Verified,
    string DistributionScope,
    int KoreanLocalizationCount,
    int PalPortraitCandidateCount,
    int EligiblePalIconCount,
    int ExactMatchCount,
    int CaseOnlyMatchCount,
    int ParameterRowMatchCount,
    int LocalizedNameBridgeMatchCount,
    int MultiIconGroupMatchCount,
    int MultiIconGroupMemberCount,
    int KoreanLocalizationWithoutIconCount,
    int IconWithoutKoreanLocalizationCount,
    IReadOnlyList<LocalizedPalIconMatch> EligiblePalIcons,
    IReadOnlyList<LocalizedPalIconGroupMatch> MultiIconGroups,
    IReadOnlyList<KoreanPalWithoutIcon> KoreanLocalizationsWithoutIcon,
    IReadOnlyList<UnlocalizedPalIcon> IconsWithoutKoreanLocalization,
    IReadOnlyList<DuplicateGameIconAssetGroup> DuplicateAssetGroups);

public sealed record GameIconLocalizationMatchResult(
    string OutputPath,
    int EligiblePalIconCount,
    int KoreanLocalizationWithoutIconCount,
    int IconWithoutKoreanLocalizationCount);

public static class GameIconLocalizationMatcher
{
    private const int MaximumManifestBytes = 8 * 1024 * 1024;
    private const int MaximumLocalizationBytes = 4 * 1024 * 1024;
    private const int MaximumSourceLinksBytes = 8 * 1024 * 1024;

    private static readonly JsonSerializerOptions JsonOptions = new()
    {
        PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower,
        WriteIndented = true,
        UnmappedMemberHandling = JsonUnmappedMemberHandling.Disallow,
        MaxDepth = 64,
    };

    public static GameIconLocalizationMatchResult BuildToFile(
        string iconManifestPath,
        string koreanPalCatalogPath,
        string palIconSourceLinksPath,
        GameIconContract contract,
        string outputPath)
    {
        var manifestBytes = SecureInput.ReadBoundedRegularFile(
            iconManifestPath,
            MaximumManifestBytes);
        var localizationBytes = SecureInput.ReadBoundedRegularFile(
            koreanPalCatalogPath,
            MaximumLocalizationBytes);
        var sourceLinksBytes = SecureInput.ReadBoundedRegularFile(
            palIconSourceLinksPath,
            MaximumSourceLinksBytes);
        var manifest = JsonSerializer.Deserialize<GameIconPackageManifest>(
                manifestBytes,
                JsonOptions)
            ?? throw Failure("game icon manifest is empty");
        var localizations = JsonSerializer.Deserialize<
                Dictionary<string, KoreanPalLocalization>>(
                localizationBytes,
                JsonOptions)
            ?? throw Failure("Korean pal localization catalog is empty");
        var sourceLinks = JsonSerializer.Deserialize<PalIconSourceLinkReport>(
                sourceLinksBytes,
                JsonOptions)
            ?? throw Failure("Pal icon source-link report is empty");
        var manifestSha256 = Hashing.Sha256Hex(manifestBytes);
        var localizationSha256 = Hashing.Sha256Hex(localizationBytes);
        var sourceLinksSha256 = Hashing.Sha256Hex(sourceLinksBytes);
        if (manifest.SchemaMajor != CatalogContract.SchemaMajor
            || !string.Equals(
                manifest.GameBuildId,
                contract.GameBuildId,
                StringComparison.Ordinal)
            || !string.Equals(
                manifest.MappingSha256,
                contract.ApprovedMappingSha256,
                StringComparison.Ordinal)
            || !string.Equals(
                manifest.ReviewId,
                contract.ReviewId,
                StringComparison.Ordinal)
            || !string.Equals(
                manifest.DistributionScope,
                contract.DistributionScope,
                StringComparison.Ordinal)
            || !manifest.OriginalGameAssetsOnly
            || !string.Equals(
                manifest.SourceKind,
                "installed_game_asset",
                StringComparison.Ordinal)
            || !string.Equals(
                localizationSha256,
                contract.KoreanPalCatalogSha256,
                StringComparison.Ordinal)
            || sourceLinks.SchemaMajor != CatalogContract.SchemaMajor
            || !sourceLinks.Verified
            || !string.Equals(
                sourceLinks.GameBuildId,
                contract.GameBuildId,
                StringComparison.Ordinal)
            || !string.Equals(
                sourceLinks.MappingSha256,
                contract.ApprovedMappingSha256,
                StringComparison.Ordinal)
            || !string.Equals(
                sourceLinks.ReviewId,
                contract.PalIconLinkReviewId,
                StringComparison.Ordinal)
            || !string.Equals(
                sourceLinks.SourceKind,
                "installed_game_data_table",
                StringComparison.Ordinal))
        {
            throw Failure(
                "game icon localization matching requires reviewed exact-Build inputs");
        }

        var candidates = manifest.Icons
            .Where(icon => string.Equals(
                icon.GroupId,
                "pal_portrait",
                StringComparison.Ordinal))
            .OrderBy(icon => icon.LogicalId, StringComparer.Ordinal)
            .ToArray();
        var iconsByPackagePath = candidates
            .GroupBy(icon => icon.PackagePath, StringComparer.OrdinalIgnoreCase)
            .ToDictionary(
                group => group.Key,
                group => group.ToArray(),
                StringComparer.OrdinalIgnoreCase);
        var sourceEntries = sourceLinks.IconTableEntries
            .ToDictionary(entry => entry.RowKey, StringComparer.Ordinal);
        var sourceEntriesInsensitive = sourceLinks.IconTableEntries
            .GroupBy(entry => entry.RowKey, StringComparer.OrdinalIgnoreCase)
            .ToDictionary(
                group => group.Key,
                group => group.ToArray(),
                StringComparer.OrdinalIgnoreCase);
        var parameterRows = sourceLinks.ParameterRowLinks
            .ToDictionary(link => link.RowKey, StringComparer.Ordinal);
        var parameterRowsInsensitive = sourceLinks.ParameterRowLinks
            .GroupBy(link => link.RowKey, StringComparer.OrdinalIgnoreCase)
            .ToDictionary(
                group => group.Key,
                group => group.ToArray(),
                StringComparer.OrdinalIgnoreCase);
        var nameLinks = sourceLinks.LocalizedNameLinks
            .ToDictionary(link => link.LocalizationKey, StringComparer.Ordinal);

        var matchedIconIds = new HashSet<string>(StringComparer.Ordinal);
        var matches = new List<LocalizedPalIconMatch>();
        var groupMatches = new List<LocalizedPalIconGroupMatch>();
        var missing = new List<KoreanPalWithoutIcon>();
        foreach (var localization in localizations
                     .OrderBy(pair => pair.Key, StringComparer.Ordinal))
        {
            ExtractedGameIcon? icon = null;
            var matchKind = "";
            IReadOnlyList<string> sourceRowKeys = [];
            IReadOnlyList<string> tribes = [];
            string? linkedPackagePath = null;
            var missingReason = "no_exact_game_table_link";
            if (sourceEntries.TryGetValue(
                    localization.Key,
                    out var exactSource))
            {
                linkedPackagePath = exactSource.PackagePath;
                matchKind = "direct_table_exact";
            }
            else if (sourceEntriesInsensitive.TryGetValue(
                         localization.Key,
                         out var insensitiveSources)
                     && insensitiveSources.Length == 1)
            {
                linkedPackagePath = insensitiveSources[0].PackagePath;
                matchKind = "direct_table_case_only";
            }
            else
            {
                PalParameterIconSourceLink? parameterLink = null;
                if (parameterRows.TryGetValue(
                        localization.Key,
                        out var exactParameter))
                {
                    parameterLink = exactParameter;
                    matchKind = "parameter_row_to_tribe";
                }
                else if (parameterRowsInsensitive.TryGetValue(
                             localization.Key,
                             out var insensitiveParameters)
                         && insensitiveParameters.Length == 1)
                {
                    parameterLink = insensitiveParameters[0];
                    matchKind = "parameter_row_case_only_to_tribe";
                }
                if (parameterLink is not null)
                {
                    sourceRowKeys = [parameterLink.RowKey];
                    tribes = IsUsable(parameterLink.Tribe)
                        ? [parameterLink.Tribe]
                        : [];
                    if (IsResolved(parameterLink.Resolution)
                        && parameterLink.PackagePaths.Count == 1)
                    {
                        linkedPackagePath = parameterLink.PackagePaths[0];
                    }
                    else
                    {
                        missingReason = parameterLink.Resolution switch
                        {
                            "missing_icon_table_row" =>
                                "parameter_tribe_without_icon_table_entry",
                            _ => "ambiguous_game_table_link",
                        };
                    }
                }
                else if (nameLinks.TryGetValue(
                             $"PAL_NAME_{localization.Key}",
                             out var nameLink))
                {
                    sourceRowKeys = nameLink.ParameterRowKeys;
                    tribes = nameLink.Tribes;
                    matchKind = "localized_name_to_tribe";
                    if (IsResolved(nameLink.Resolution)
                        && nameLink.PackagePaths.Count == 1)
                    {
                        linkedPackagePath = nameLink.PackagePaths[0];
                    }
                    else if (TryBuildMultiIconGroup(
                                 localization.Key,
                                 localization.Value.LocalizedName,
                                 nameLink,
                                 parameterRows,
                                 iconsByPackagePath,
                                 out var groupMatch))
                    {
                        groupMatches.Add(groupMatch);
                        foreach (var member in groupMatch.Members)
                        {
                            matchedIconIds.Add(member.IconLogicalId);
                        }
                        continue;
                    }
                    else
                    {
                        missingReason = nameLink.Resolution switch
                        {
                            "missing_icon_table_row" =>
                                "localized_name_tribe_without_icon_table_entry",
                            _ => "ambiguous_game_table_link",
                        };
                    }
                }
            }

            if (linkedPackagePath is not null)
            {
                if (iconsByPackagePath.TryGetValue(
                        linkedPackagePath,
                        out var linkedIcons)
                    && linkedIcons.Length == 1)
                {
                    icon = linkedIcons[0];
                }
                else
                {
                    missingReason =
                        "linked_icon_not_in_reviewed_portrait_inventory";
                }
            }

            if (icon is null)
            {
                missing.Add(new KoreanPalWithoutIcon(
                    localization.Key,
                    localization.Value.LocalizedName,
                    missingReason,
                    sourceRowKeys,
                    tribes));
                continue;
            }
            matchedIconIds.Add(icon.LogicalId);
            matches.Add(new LocalizedPalIconMatch(
                localization.Key,
                localization.Value.LocalizedName,
                icon.LogicalId,
                matchKind,
                icon.PackagePath,
                icon.ThumbnailRelativePath,
                icon.ThumbnailPngSha256));
        }

        var unlocalized = candidates
            .Where(icon => !matchedIconIds.Contains(icon.LogicalId))
            .Select(icon => new UnlocalizedPalIcon(
                icon.LogicalId,
                icon.PackagePath,
                icon.ThumbnailRelativePath,
                icon.ThumbnailPngSha256))
            .ToArray();
        var duplicateAssets = manifest.Icons
            .GroupBy(icon => icon.SourceDecodedRgbaSha256, StringComparer.Ordinal)
            .Where(group => group.Count() > 1)
            .OrderBy(group => group.Key, StringComparer.Ordinal)
            .Select(group => new DuplicateGameIconAssetGroup(
                group.Key,
                group
                    .Select(icon => $"{icon.GroupId}:{icon.LogicalId}")
                    .Order(StringComparer.Ordinal)
                    .ToArray()))
            .ToArray();
        var report = new GameIconLocalizationMatchReport(
            CatalogContract.SchemaMajor,
            manifest.GameBuildId,
            manifest.MappingSha256,
            manifestSha256,
            localizationSha256,
            sourceLinksSha256,
            "ko",
            Verified: true,
            manifest.DistributionScope,
            localizations.Count,
            candidates.Length,
            matches.Count,
            matches.Count(match =>
                match.MatchKind == "direct_table_exact"),
            matches.Count(match =>
                match.MatchKind.Contains(
                    "case_only",
                    StringComparison.Ordinal)),
            matches.Count(match =>
                match.MatchKind.StartsWith(
                    "parameter_row",
                    StringComparison.Ordinal)),
            matches.Count(match =>
                match.MatchKind == "localized_name_to_tribe"),
            groupMatches.Count,
            groupMatches.Sum(group => group.Members.Count),
            missing.Count,
            unlocalized.Length,
            matches,
            groupMatches,
            missing,
            unlocalized,
            duplicateAssets);

        var fullOutput = Path.GetFullPath(outputPath);
        if (File.Exists(fullOutput) || Directory.Exists(fullOutput))
        {
            throw Failure("game icon localization report output must not exist");
        }
        var parent = Path.GetDirectoryName(fullOutput)
            ?? throw Failure("game icon localization report parent is invalid");
        Directory.CreateDirectory(parent);
        SecureInput.EnsurePlainDirectory(parent);
        var staging = Path.Combine(
            parent,
            $".{Path.GetFileName(fullOutput)}.{Guid.NewGuid():N}.staging");
        try
        {
            File.WriteAllBytes(
                staging,
                JsonSerializer.SerializeToUtf8Bytes(report, JsonOptions));
            using (var stream = new FileStream(
                       staging,
                       FileMode.Open,
                       FileAccess.Read,
                       FileShare.Read,
                       64 * 1024,
                       FileOptions.SequentialScan))
            {
                _ = JsonSerializer.Deserialize<GameIconLocalizationMatchReport>(
                        stream,
                        JsonOptions)
                    ?? throw Failure(
                        "game icon localization report could not be reopened");
            }
            File.Move(staging, fullOutput);
            return new GameIconLocalizationMatchResult(
                fullOutput,
                matches.Count,
                missing.Count,
                unlocalized.Length);
        }
        finally
        {
            if (File.Exists(staging))
            {
                File.Delete(staging);
            }
        }
    }

    private static bool TryBuildMultiIconGroup(
        string internalId,
        string koreanName,
        PalNameIconSourceLink nameLink,
        IReadOnlyDictionary<string, PalParameterIconSourceLink> parameterRows,
        IReadOnlyDictionary<string, ExtractedGameIcon[]> iconsByPackagePath,
        out LocalizedPalIconGroupMatch group)
    {
        group = null!;
        if (!string.Equals(
                nameLink.Resolution,
                "ambiguous_multiple_icons",
                StringComparison.Ordinal)
            || nameLink.ParameterRowKeys.Count < 2)
        {
            return false;
        }

        var members = new List<LocalizedPalIconGroupMember>();
        foreach (var rowKey in nameLink.ParameterRowKeys)
        {
            if (!parameterRows.TryGetValue(rowKey, out var parameter)
                || !IsResolved(parameter.Resolution)
                || parameter.PackagePaths.Count != 1
                || !iconsByPackagePath.TryGetValue(
                    parameter.PackagePaths[0],
                    out var icons)
                || icons.Length != 1)
            {
                return false;
            }
            var icon = icons[0];
            members.Add(new LocalizedPalIconGroupMember(
                rowKey,
                parameter.Tribe,
                icon.LogicalId,
                icon.PackagePath,
                icon.ThumbnailRelativePath,
                icon.ThumbnailPngSha256));
        }

        var ordered = members
            .OrderBy(member => member.SourceRowKey, StringComparer.Ordinal)
            .ToArray();
        if (ordered.Select(member => member.IconLogicalId)
            .Distinct(StringComparer.Ordinal)
            .Count() != ordered.Length)
        {
            return false;
        }
        group = new LocalizedPalIconGroupMatch(
            internalId,
            koreanName,
            nameLink.LocalizationKey,
            "shared_localization_multi_entity_group",
            ordered);
        return true;
    }

    private static DataPackFailure Failure(string message) =>
        new(DataPackExitCode.SourceContractMismatch, message);

    private static bool IsResolved(string resolution) =>
        string.Equals(resolution, "resolved", StringComparison.Ordinal);

    private static bool IsUsable(string value) =>
        !string.IsNullOrWhiteSpace(value)
        && !string.Equals(value, "None", StringComparison.Ordinal);
}
