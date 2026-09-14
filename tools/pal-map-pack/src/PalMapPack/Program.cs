using System.Reflection;
using System.Text.Json;
using System.Text.Json.Serialization;

namespace PalMapPack;

internal static class Program
{
    public static int Main(string[] args) =>
        CommandRunner.Run(
            args,
            Environment.GetEnvironmentVariables()
                .Cast<System.Collections.DictionaryEntry>()
                .ToDictionary(entry => (string)entry.Key, entry => entry.Value?.ToString()));
}

public static class CommandRunner
{
    private const string DefaultAppId = "1623730";
    private const string DefaultBuildId = "24467282";
    private const long MaximumContractBytes = 1_048_576;
    private const long MaximumMappingBytes = 16_777_216;
    private const long MaximumLandmarkBytes = 1_048_576;

    private static readonly HashSet<string> Commands =
        new(StringComparer.Ordinal)
        {
            "doctor",
            "probe-assets",
            "discover-poi-candidates",
            "prepare-holdout-commitment",
            "accept-contract",
            "stage",
            "calibrate",
            "validate",
            "publish",
        };

    private static readonly HashSet<string> Options =
        new(StringComparer.Ordinal)
        {
            "--steam-app-id",
            "--steam-root",
            "--build",
            "--mappings",
            "--contract",
            "--dataset-root",
            "--landmarks",
            "--commitment-output",
            "--candidate-output",
            "--extractor-version",
            "--extractor-commit",
        };

    private static readonly JsonSerializerOptions JsonOptions = new()
    {
        PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower,
        WriteIndented = true,
        UnmappedMemberHandling = JsonUnmappedMemberHandling.Disallow,
    };

    public static int Run(IReadOnlyList<string> args, IReadOnlyDictionary<string, string?> environment) =>
        Run(args, environment, CommandServices.Default);

    public static int Run(
        IReadOnlyList<string> args,
        IReadOnlyDictionary<string, string?> environment,
        CommandServices services)
    {
        ArgumentNullException.ThrowIfNull(args);
        ArgumentNullException.ThrowIfNull(environment);
        ArgumentNullException.ThrowIfNull(services);
        try
        {
            if (args.Count == 0 || !Commands.Contains(args[0]))
            {
                return ExitCodes.Usage;
            }
            var options = ParseOptions(args);
            var command = args[0];
            var mappingPath = Value(options, "--mappings")
                ?? EnvironmentValue(environment, "PAL_USMAP_PATH");
            RequireMapping(mappingPath);

            var buildId = Value(options, "--build") ?? DefaultBuildId;
            ValidateNumericIdentity(buildId, "Build");
            var appId = Value(options, "--steam-app-id") ?? DefaultAppId;
            ValidateNumericIdentity(appId, "Steam App");
            var install = LocateInstall(appId, Value(options, "--steam-root"));
            if (!string.Equals(install.BuildId, buildId, StringComparison.Ordinal))
            {
                throw new MapPackFailure(
                    ExitCodes.MountSerialization,
                    "installed Steam Build does not match the requested exact Build");
            }
            var containerPaths = AssetPaths.DiscoverContainers(install.InstallPath);
            if (command == "doctor")
            {
                return ExitCodes.Success;
            }

            var contractPath = RequireOption(options, "--contract", ExitCodes.AssetContractMismatch);
            var contractBytes = ReadProtectedInput(
                contractPath,
                MaximumContractBytes,
                ExitCodes.AssetContractMismatch,
                "asset contract");
            var contract = DeserializeContract(contractBytes);
            if (!string.Equals(contract.GameBuildId, buildId, StringComparison.Ordinal))
            {
                throw new MapPackFailure(
                    ExitCodes.AssetContractMismatch,
                    "Build-specific asset contract does not match the requested exact Build");
            }
            var mappingBytes = ReadProtectedInput(
                mappingPath!,
                MaximumMappingBytes,
                ExitCodes.MissingMapping,
                "mapping");
            var mappingHash = Hashing.Sha256Hex(mappingBytes);
            var contractHash = Hashing.Sha256Hex(contractBytes);
            EnsureMappingIdentity(contract, mappingBytes.LongLength, mappingHash);

            if (command == "prepare-holdout-commitment")
            {
                return PrepareHoldoutCommitment(
                    options,
                    mappingPath!,
                    contractPath,
                    buildId,
                    contract);
            }

            if (command == "discover-poi-candidates")
            {
                var output = RequireOption(
                    options,
                    "--candidate-output",
                    ExitCodes.AssetContractMismatch);
                var candidates = Cue4ParseMapAssetReader.DiscoverPoiCandidatePackages(
                    install.InstallPath,
                    mappingPath!);
                WriteOutput(
                    output,
                    new PoiCandidateDiscoveryDocument(
                        1,
                        buildId,
                        candidates.Count,
                        candidates));
                return ExitCodes.Success;
            }

            if (command == "probe-assets")
            {
                var probe = Probe(services, install, buildId, mappingPath!, contract);
                var probeDatasetRoot = RequireDatasetRoot(options);
                var probePipelineRoot = OpenPipelineRoot(probeDatasetRoot, buildId);
                var probeSnapshots = CaptureContainers(install.InstallPath, containerPaths);
                var regionDocuments = new List<ProbeRegionDocument>();
                foreach (var region in probe.Regions.All
                    .OrderBy(region => region.Priority)
                    .ThenBy(region => region.MapId, StringComparer.Ordinal)
                    .ThenBy(region => region.RegionId, StringComparer.Ordinal))
                {
                    var mapAssetSha256 = Hashing.MapAssetSha256(region.Map);
                    CalibrationPreviewContract? calibrationPreview = null;
                    if (region.MapId == "MainMap" && region.RegionId == "FirstRegion")
                    {
                        var boundPreview = contract.MapRegions.Single(candidate =>
                            candidate.MapId == region.MapId
                            && candidate.RegionId == region.RegionId).CalibrationPreview;
                        var dimensions = CalibrationPreviewBuilder.SelectDimensions(
                            region.Map,
                            boundPreview);
                        var previewArtifact = CalibrationPreviewBuilder.BuildDeterministicBmp(
                            region.Map,
                            mapAssetSha256,
                            dimensions.Width,
                            dimensions.Height);
                        calibrationPreview = previewArtifact.Contract;
                        if (boundPreview is not null && calibrationPreview != boundPreview)
                        {
                            throw new MapPackFailure(
                                ExitCodes.AssetContractMismatch,
                                "generated calibration preview does not match the exact-Build contract");
                        }
                        CalibrationPreviewBuilder.WriteDeterministicBmp(
                            Path.Combine(probePipelineRoot, "calibration-preview-mainmap.bmp"),
                            previewArtifact);
                    }
                    regionDocuments.Add(new ProbeRegionDocument(
                        region.MapId,
                        region.RegionId,
                        region.SourceTexturePath,
                        region.WorldMinX,
                        region.WorldMinY,
                        region.WorldMaxX,
                        region.WorldMaxY,
                        region.BlockSizeX,
                        region.BlockSizeY,
                        region.GridPositionX,
                        region.GridPositionY,
                        region.Priority,
                        region.Map.Width,
                        region.Map.Height,
                        mapAssetSha256,
                        calibrationPreview));
                }
                WriteState(
                    probePipelineRoot,
                    "probe.json",
                    new ProbeReport(
                        2,
                        contract.Reviewed
                            ? "REVIEWED_CONTRACT_PROBED"
                            : "CANDIDATE_PROBED_NOT_ACCEPTED",
                        buildId,
                        mappingHash,
                        contractHash,
                        Hashing.SourceInputSetSha256(probeSnapshots),
                        probeSnapshots,
                        regionDocuments,
                        probe.Inventory.Entries
                            .OrderBy(entry => entry.PackagePath, StringComparer.Ordinal)
                            .ThenBy(entry => entry.ExportClass, StringComparer.Ordinal)
                            .ToArray()));
                return ExitCodes.Success;
            }

            EnsureReviewed(contract);
            if (command == "accept-contract")
            {
                return ExitCodes.Success;
            }

            var datasetRoot = RequireDatasetRoot(options);
            var snapshots = CaptureContainers(install.InstallPath, containerPaths);
            var pipelineRoot = OpenPipelineRoot(datasetRoot, buildId);
            switch (command)
            {
                case "stage":
                    return Stage(
                        services,
                        install,
                        buildId,
                        mappingPath!,
                        mappingHash,
                        contractPath,
                        contractBytes,
                        contractHash,
                        contract,
                        snapshots,
                        pipelineRoot);
                case "calibrate":
                    return Calibrate(
                        options,
                        services,
                        install,
                        buildId,
                        mappingPath!,
                        mappingHash,
                        contractHash,
                        contract,
                        snapshots,
                        pipelineRoot);
                case "validate":
                    return Validate(
                        options,
                        environment,
                        services,
                        install,
                        buildId,
                        mappingPath!,
                        mappingBytes,
                        mappingHash,
                        contractBytes,
                        contractHash,
                        contract,
                        containerPaths,
                        snapshots,
                        datasetRoot,
                        pipelineRoot);
                case "publish":
                    return Publish(
                        buildId,
                        mappingHash,
                        contractHash,
                        snapshots,
                        datasetRoot,
                        pipelineRoot);
                default:
                    return ExitCodes.Usage;
            }
        }
        catch (MapPackFailure failure)
        {
            Console.Error.WriteLine($"NO-GO [{failure.ExitCode}]: {failure.Message}");
            return failure.ExitCode;
        }
        catch (Exception error) when (error is
            ArgumentException or
            IOException or
            JsonException or
            NotSupportedException or
            OverflowException or
            UnauthorizedAccessException)
        {
            Console.Error.WriteLine($"NO-GO [{ExitCodes.PackIntegrity}]: local pipeline input or state is invalid");
            return ExitCodes.PackIntegrity;
        }
    }

    private static int Stage(
        CommandServices services,
        SteamInstall install,
        string buildId,
        string mappingPath,
        string mappingHash,
        string contractPath,
        byte[] contractBytes,
        string contractHash,
        AssetContract contract,
        IReadOnlyList<SourceContainerSnapshot> snapshots,
        string pipelineRoot)
    {
        var probe = Probe(services, install, buildId, mappingPath, contract);
        AssetCatalogProbe.EnsureContractAccepted(contract, buildId, probe.Inventory);
        var state = new StageState(
            1,
            buildId,
            mappingHash,
            contractHash,
            Hashing.SourceInputSetSha256(snapshots),
            snapshots,
            Path.GetFileName(contractPath),
            Hashing.Sha256Hex(contractBytes));
        WriteState(pipelineRoot, "stage.json", state);
        return ExitCodes.Success;
    }

    private static int PrepareHoldoutCommitment(
        IReadOnlyDictionary<string, string> options,
        string mappingPath,
        string contractPath,
        string buildId,
        AssetContract contract)
    {
        var mainRegions = contract.MapRegions
            .Where(region =>
                string.Equals(region.MapId, "MainMap", StringComparison.Ordinal)
                && string.Equals(region.RegionId, "FirstRegion", StringComparison.Ordinal))
            .ToArray();
        if (mainRegions.Length != 1)
        {
            throw new MapPackFailure(
                ExitCodes.AssetContractMismatch,
                "exact-Build contract must define one authoritative MainMap region");
        }
        var holdoutsPath = RequireOption(options, "--landmarks", ExitCodes.CalibrationRejected);
        var holdoutBytes = ReadProtectedInput(
            holdoutsPath,
            MaximumLandmarkBytes,
            ExitCodes.CalibrationRejected,
            "sealed holdout file");
        var holdouts = DeserializeLandmarks(holdoutBytes, buildId);
        var main = mainRegions[0];
        var commitment = CoordinateSolver.CreateSealedHoldoutCommitment(
            main.WorldMinX,
            main.WorldMinY,
            main.WorldMaxX,
            main.WorldMaxY,
            holdouts,
            buildId);
        var output = RequireOption(
            options,
            "--commitment-output",
            ExitCodes.CalibrationRejected);
        var normalizedOutput = Path.GetFullPath(output);
        if (new[] { holdoutsPath, contractPath, mappingPath }
            .Select(Path.GetFullPath)
            .Any(input => string.Equals(
                input,
                normalizedOutput,
                StringComparison.OrdinalIgnoreCase)))
        {
            throw new MapPackFailure(
                ExitCodes.CalibrationRejected,
                "commitment output must not replace a protected input");
        }
        WriteOutput(
            normalizedOutput,
            new HoldoutCommitmentDocument(
                commitment.SchemaVersion,
                commitment.GameBuildId,
                commitment.SealedHoldoutCount,
                commitment.SealedHoldoutSha256,
                commitment.ZoneCounts[LandmarkZone.Center],
                commitment.ZoneCounts[LandmarkZone.NorthWest],
                commitment.ZoneCounts[LandmarkZone.NorthEast],
                commitment.ZoneCounts[LandmarkZone.SouthWest],
                commitment.ZoneCounts[LandmarkZone.SouthEast]));
        return ExitCodes.Success;
    }

    private static int Calibrate(
        IReadOnlyDictionary<string, string> options,
        CommandServices services,
        SteamInstall install,
        string buildId,
        string mappingPath,
        string mappingHash,
        string contractHash,
        AssetContract contract,
        IReadOnlyList<SourceContainerSnapshot> snapshots,
        string pipelineRoot)
    {
        _ = ReadMatchingStage(pipelineRoot, buildId, mappingHash, contractHash, snapshots);
        var landmarksPath = RequireOption(options, "--landmarks", ExitCodes.CalibrationRejected);
        var landmarkBytes = ReadProtectedInput(
            landmarksPath,
            MaximumLandmarkBytes,
            ExitCodes.CalibrationRejected,
            "landmark file");
        var landmarks = DeserializeLandmarks(landmarkBytes, buildId);
        var probedRegions = Probe(services, install, buildId, mappingPath, contract).Regions;
        var mainRegion = MapPackBuilder.CreateRuntimeRegions(probedRegions, contract)
            .All.Single(region =>
                region.MapId == "MainMap" && region.RegionId == "FirstRegion");
        var result = CoordinateSolver.ValidateAuthoritativeBounds(mainRegion, landmarks, buildId);
        if (!result.Accepted)
        {
            throw new MapPackFailure(ExitCodes.CalibrationRejected, result.RejectionReason);
        }
        WriteState(
            pipelineRoot,
            "calibration.json",
            new CalibrationState(
                2,
                buildId,
                mappingHash,
                contractHash,
                Hashing.Sha256Hex(landmarkBytes),
                checked((uint)landmarks.Count),
                result.ReferenceCount,
                result.SealedHoldoutCount,
                result.ReferenceMedianErrorPixels,
                result.ReferenceMaxErrorPixels,
                result.SealedHoldoutMedianErrorPixels,
                result.SealedHoldoutMaxErrorPixels,
                result.SealedHoldoutSha256,
                result.ReferenceZoneCounts.ToDictionary(
                    pair => pair.Key.ToString(),
                    pair => checked((uint)pair.Value),
                    StringComparer.Ordinal),
                result.SealedHoldoutZoneCounts.ToDictionary(
                    pair => pair.Key.ToString(),
                    pair => checked((uint)pair.Value),
                    StringComparer.Ordinal)));
        return ExitCodes.Success;
    }

    private static int Validate(
        IReadOnlyDictionary<string, string> options,
        IReadOnlyDictionary<string, string?> environment,
        CommandServices services,
        SteamInstall install,
        string buildId,
        string mappingPath,
        byte[] mappingBytes,
        string mappingHash,
        byte[] contractBytes,
        string contractHash,
        AssetContract contract,
        IReadOnlyList<SourceContainerPath> containerPaths,
        IReadOnlyList<SourceContainerSnapshot> snapshots,
        string datasetRoot,
        string pipelineRoot)
    {
        _ = ReadMatchingStage(pipelineRoot, buildId, mappingHash, contractHash, snapshots);
        var landmarksPath = RequireOption(options, "--landmarks", ExitCodes.CalibrationRejected);
        var landmarkBytes = ReadProtectedInput(
            landmarksPath,
            MaximumLandmarkBytes,
            ExitCodes.CalibrationRejected,
            "landmark file");
        var landmarks = DeserializeLandmarks(landmarkBytes, buildId);
        var calibration = ReadState<CalibrationState>(pipelineRoot, "calibration.json");
        if (calibration.SchemaVersion != 2
            || calibration.GameBuildId != buildId
            || calibration.MappingSha256 != mappingHash
            || calibration.ContractSha256 != contractHash
            || calibration.LandmarksSha256 != Hashing.Sha256Hex(landmarkBytes))
        {
            throw new MapPackFailure(
                ExitCodes.CalibrationRejected,
                "calibration state does not match the exact staged inputs");
        }

        var probe = Probe(services, install, buildId, mappingPath, contract);
        AssetCatalogProbe.EnsureContractAccepted(contract, buildId, probe.Inventory);
        var fullContainerPaths = containerPaths.Select(container => container.FullPath).ToArray();
        var guard = new SnapshotInputGuard(install.InstallPath, snapshots);
        var build = MapPackBuilder.BuildStaging(new MapPackBuildRequest(
            datasetRoot,
            buildId,
            mappingPath,
            mappingBytes,
            contractBytes,
            contract,
            snapshots,
            containerPaths,
            landmarks,
            services.CreateAssetReader(fullContainerPaths),
            DateTimeOffset.UtcNow,
            Value(options, "--extractor-version")
                ?? Assembly.GetExecutingAssembly().GetName().Version?.ToString()
                ?? "0.0.0",
            Value(options, "--extractor-commit")
                ?? EnvironmentValue(environment, "PAL_EXTRACTOR_COMMIT")
                ?? "local-working-tree",
            probe.Inventory,
            guard));
        if (!MapPackValidator.IsValid(
                build.StagingPath,
                buildId,
                contractHash,
                mappingHash))
        {
            throw new MapPackFailure(
                ExitCodes.PackIntegrity,
                "new staging pack failed complete integrity validation");
        }
        WriteState(
            pipelineRoot,
            "validated.json",
            new ValidatedState(
                1,
                buildId,
                mappingHash,
                contractHash,
                Hashing.SourceInputSetSha256(snapshots),
                Path.GetFullPath(build.StagingPath),
                Hashing.Sha256File(Path.Combine(build.StagingPath, "manifest.json"))));
        return ExitCodes.Success;
    }

    private static int Publish(
        string buildId,
        string mappingHash,
        string contractHash,
        IReadOnlyList<SourceContainerSnapshot> snapshots,
        string datasetRoot,
        string pipelineRoot)
    {
        _ = ReadMatchingStage(pipelineRoot, buildId, mappingHash, contractHash, snapshots);
        var state = ReadState<ValidatedState>(pipelineRoot, "validated.json");
        if (state.SchemaVersion != 1
            || state.GameBuildId != buildId
            || state.MappingSha256 != mappingHash
            || state.ContractSha256 != contractHash
            || state.SourceInputSetSha256 != Hashing.SourceInputSetSha256(snapshots)
            || !SourceContainerSnapshot.IsWithin(datasetRoot, Path.GetFullPath(state.StagingPath))
            || !File.Exists(Path.Combine(state.StagingPath, "manifest.json"))
            || !string.Equals(
                state.ManifestSha256,
                Hashing.Sha256File(Path.Combine(state.StagingPath, "manifest.json")),
                StringComparison.Ordinal))
        {
            throw new MapPackFailure(
                ExitCodes.PackIntegrity,
                "validated staging state does not match the exact current inputs");
        }

        bool ValidatePack(string path) =>
            MapPackValidator.IsValid(path, buildId, contractHash, mappingHash);
        if (!ValidatePack(state.StagingPath))
        {
            throw new MapPackFailure(
                ExitCodes.PackIntegrity,
                "validated staging pack changed before publication");
        }
        var target = Path.Combine(datasetRoot, buildId);
        AtomicPublisher.Publish(state.StagingPath, target, ValidatePack);
        var active = AtomicPublisher.ResolveActivePath(datasetRoot, buildId);
        if (!ValidatePack(active))
        {
            throw new MapPackFailure(
                ExitCodes.PackIntegrity,
                "published pack failed post-activation validation");
        }
        return ExitCodes.Success;
    }

    private static AssetProbeResult Probe(
        CommandServices services,
        SteamInstall install,
        string buildId,
        string mappingPath,
        AssetContract contract) =>
        services.Probe(new AssetProbeRequest(
            install.InstallPath,
            buildId,
            mappingPath,
            contract));

    private static StageState ReadMatchingStage(
        string pipelineRoot,
        string buildId,
        string mappingHash,
        string contractHash,
        IReadOnlyList<SourceContainerSnapshot> snapshots)
    {
        var stage = ReadState<StageState>(pipelineRoot, "stage.json");
        if (stage.SchemaVersion != 1
            || stage.GameBuildId != buildId
            || stage.MappingSha256 != mappingHash
            || stage.ContractSha256 != contractHash
            || stage.SourceInputSetSha256 != Hashing.SourceInputSetSha256(snapshots))
        {
            throw new MapPackFailure(
                ExitCodes.PackIntegrity,
                "staged inputs do not match the current exact Build");
        }
        SourceContainerSnapshot.EnsureUnchanged(stage.SourceContainers, snapshots);
        return stage;
    }

    private static IReadOnlyList<SourceContainerSnapshot> CaptureContainers(
        string installPath,
        IReadOnlyList<SourceContainerPath> containers) =>
        containers
            .Select(container => SourceContainerSnapshot.Capture(installPath, container.FullPath))
            .ToArray();

    private static SteamInstall LocateInstall(string appId, string? steamRoot) =>
        string.IsNullOrWhiteSpace(steamRoot)
            ? SteamInstallLocator.LocateDefault(appId)
            : SteamInstallLocator.Locate(
                appId,
                SteamInstallLocator.DiscoverLibraryRoots(steamRoot));

    private static AssetContract DeserializeContract(byte[] bytes)
        => AssetContract.Parse(bytes);

    private static IReadOnlyList<Landmark> DeserializeLandmarks(byte[] bytes, string buildId)
    {
        try
        {
            var rows = JsonSerializer.Deserialize<LandmarkInput[]>(bytes, JsonOptions)
                ?? throw new JsonException("empty landmark file");
            return rows.Select(row => new Landmark(
                    row.Id,
                    row.WorldX,
                    row.WorldY,
                    row.MapX,
                    row.MapY,
                    CoordinateSolver.DeriveZone(row.WorldX, row.WorldY),
                    row.EvidenceSet switch
                    {
                        "reference" => LandmarkEvidenceSet.Reference,
                        "sealed_holdout" => LandmarkEvidenceSet.SealedHoldout,
                        _ => throw new JsonException("unknown landmark evidence set"),
                    },
                    row.GameBuildId == buildId
                        ? row.GameBuildId
                        : throw new JsonException("landmark Build identity mismatch")))
                .ToArray();
        }
        catch (JsonException)
        {
            throw new MapPackFailure(
                ExitCodes.CalibrationRejected,
                "landmark file is unreadable or invalid");
        }
    }

    private static void EnsureMappingIdentity(
        AssetContract contract,
        long mappingSize,
        string actualHash)
    {
        var expectedHash = contract.Reviewed
            ? contract.ApprovedMappingSha256
            : contract.MappingCandidate?.Sha256;
        if (string.IsNullOrWhiteSpace(expectedHash)
            || !string.Equals(actualHash, expectedHash, StringComparison.Ordinal))
        {
            throw new MapPackFailure(
                ExitCodes.AssetContractMismatch,
                "mapping hash does not match the exact-Build contract");
        }
        if (!contract.Reviewed
            && contract.MappingCandidate is { } candidate
            && (candidate.FileSizeBytes != mappingSize
                || !string.Equals(candidate.Sha256, actualHash, StringComparison.Ordinal)))
        {
            throw new MapPackFailure(
                ExitCodes.AssetContractMismatch,
                "mapping size or hash does not match the documented candidate");
        }
    }

    private static void EnsureReviewed(AssetContract contract)
    {
        if (!contract.Reviewed
            || string.IsNullOrWhiteSpace(contract.ReviewId)
            || !CanonicalNonzeroSha256(contract.ApprovedMappingSha256)
            || !CanonicalNonzeroSha256(contract.ApprovedSealedHoldoutSha256))
        {
            throw new MapPackFailure(
                ExitCodes.AssetContractMismatch,
                "Build-specific asset contract is candidate-only or not reviewed");
        }
    }

    private static bool CanonicalNonzeroSha256(string? value) =>
        value is { Length: 64 }
        && value != new string('0', 64)
        && value.All(character =>
            char.IsAsciiDigit(character)
            || character is >= 'a' and <= 'f');

    private static void RequireMapping(string? mapping)
    {
        if (string.IsNullOrWhiteSpace(mapping)
            || !string.Equals(Path.GetExtension(mapping), ".usmap", StringComparison.OrdinalIgnoreCase)
            || !File.Exists(mapping))
        {
            throw new MapPackFailure(
                ExitCodes.MissingMapping,
                "PAL_USMAP_PATH must reference a safely obtained build-matched .usmap; no runtime injection or network retrieval is performed");
        }
        try
        {
            SecureFiles.EnsureRegularFile(mapping);
        }
        catch (MapPackFailure)
        {
            throw new MapPackFailure(
                ExitCodes.MissingMapping,
                "mapping path is not a stable, regular, no-follow file");
        }
    }

    private static string RequireDatasetRoot(IReadOnlyDictionary<string, string> options)
    {
        var path = RequireOption(options, "--dataset-root", ExitCodes.PackIntegrity);
        var full = Path.GetFullPath(path);
        using var lease = Directory.Exists(full)
            ? SecureDirectoryLease.OpenExisting(full)
            : SecureDirectoryLease.Create(full);
        return full;
    }

    private static string OpenPipelineRoot(string datasetRoot, string buildId)
    {
        var root = Path.Combine(datasetRoot, $".{buildId}.pipeline");
        using var lease = Directory.Exists(root)
            ? SecureDirectoryLease.OpenExisting(root)
            : SecureDirectoryLease.Create(root);
        return root;
    }

    private static byte[] ReadProtectedInput(
        string path,
        long maximumBytes,
        int exitCode,
        string label)
    {
        try
        {
            var full = Path.GetFullPath(path);
            var parent = Directory.GetParent(full)?.FullName
                ?? throw new IOException("input has no parent");
            var before = SecureFiles.SnapshotWithin(parent, full);
            if (before.SizeBytes > maximumBytes)
            {
                throw new IOException("input exceeds its size cap");
            }
            var bytes = SecureFiles.ReadAllBytes(
                before.FinalPath,
                checked((int)maximumBytes));
            var after = SecureFiles.SnapshotWithin(parent, full);
            if (before.VolumeSerial != after.VolumeSerial
                || before.FileIndex != after.FileIndex
                || before.SizeBytes != after.SizeBytes
                || before.Sha256 != after.Sha256
                || bytes.LongLength != before.SizeBytes
                || Hashing.Sha256Hex(bytes) != before.Sha256)
            {
                throw new IOException("input changed while it was read");
            }
            return bytes;
        }
        catch (Exception error) when (error is IOException or UnauthorizedAccessException or MapPackFailure)
        {
            throw new MapPackFailure(exitCode, $"{label} is missing, unsafe, too large, or changed during read");
        }
    }

    private static IReadOnlyDictionary<string, string> ParseOptions(IReadOnlyList<string> args)
    {
        if ((args.Count - 1) % 2 != 0)
        {
            throw new MapPackFailure(ExitCodes.Usage, "every command option requires exactly one value");
        }
        var parsed = new Dictionary<string, string>(StringComparer.Ordinal);
        for (var index = 1; index < args.Count; index += 2)
        {
            var name = args[index];
            var value = args[index + 1];
            if (!Options.Contains(name)
                || string.IsNullOrWhiteSpace(value)
                || !parsed.TryAdd(name, value))
            {
                throw new MapPackFailure(
                    ExitCodes.Usage,
                    "command contains an unknown, empty, or duplicate option");
            }
        }
        return parsed;
    }

    private static string RequireOption(
        IReadOnlyDictionary<string, string> options,
        string name,
        int exitCode) =>
        Value(options, name)
        ?? throw new MapPackFailure(exitCode, $"{name} is required");

    private static string? Value(IReadOnlyDictionary<string, string> options, string name) =>
        options.TryGetValue(name, out var value) ? value : null;

    private static string? EnvironmentValue(
        IReadOnlyDictionary<string, string?> environment,
        string name) =>
        environment.TryGetValue(name, out var value) ? value : null;

    private static void ValidateNumericIdentity(string value, string label)
    {
        if (string.IsNullOrEmpty(value) || value.Any(character => !char.IsAsciiDigit(character)))
        {
            throw new MapPackFailure(
                ExitCodes.Usage,
                $"{label} identity must contain only ASCII digits");
        }
    }

    private static void WriteState<T>(string root, string name, T value)
    {
        var destination = Path.Combine(root, name);
        var temporary = Path.Combine(root, $".{name}.{Guid.NewGuid():N}.tmp");
        using var rootLease = SecureDirectoryLease.OpenExisting(root);
        if (File.Exists(destination))
        {
            SecureFiles.EnsureRegularFile(destination);
        }
        File.WriteAllBytes(temporary, JsonSerializer.SerializeToUtf8Bytes(value, JsonOptions));
        SecureFiles.EnsureRegularFile(temporary);
        File.Move(temporary, destination, overwrite: true);
        SecureFiles.EnsureRegularFile(destination);
    }

    private static void WriteOutput<T>(string path, T value)
    {
        var destination = Path.GetFullPath(path);
        var parent = Directory.GetParent(destination)?.FullName
            ?? throw new MapPackFailure(
                ExitCodes.CalibrationRejected,
                "commitment output must have an existing parent directory");
        using var parentLease = SecureDirectoryLease.OpenExisting(parent);
        if (File.Exists(destination))
        {
            throw new MapPackFailure(
                ExitCodes.CalibrationRejected,
                "commitment output already exists and will not be replaced");
        }
        var temporary = Path.Combine(
            parent,
            $".{Path.GetFileName(destination)}.{Guid.NewGuid():N}.tmp");
        try
        {
            File.WriteAllBytes(temporary, JsonSerializer.SerializeToUtf8Bytes(value, JsonOptions));
            SecureFiles.EnsureRegularFile(temporary);
            File.Move(temporary, destination);
            SecureFiles.EnsureRegularFile(destination);
        }
        finally
        {
            if (File.Exists(temporary))
            {
                File.Delete(temporary);
            }
        }
    }

    private static T ReadState<T>(string root, string name)
    {
        var path = Path.Combine(root, name);
        var bytes = ReadProtectedInput(path, MaximumContractBytes, ExitCodes.PackIntegrity, name);
        try
        {
            return JsonSerializer.Deserialize<T>(bytes, JsonOptions)
                ?? throw new JsonException("empty pipeline state");
        }
        catch (JsonException)
        {
            throw new MapPackFailure(
                ExitCodes.PackIntegrity,
                $"{name} is invalid or incompatible");
        }
    }

    private sealed record LandmarkInput(
        string Id,
        string GameBuildId,
        string EvidenceSet,
        double WorldX,
        double WorldY,
        double MapX,
        double MapY);

    private sealed record ProbeRegionDocument(
        string MapId,
        string RegionId,
        string SourceTexturePath,
        double WorldMinX,
        double WorldMinY,
        double WorldMaxX,
        double WorldMaxY,
        double BlockSizeX,
        double BlockSizeY,
        double GridPositionX,
        double GridPositionY,
        int Priority,
        int MapWidthPx,
        int MapHeightPx,
        string MapAssetSha256,
        CalibrationPreviewContract? CalibrationPreview);

    private sealed record ProbeReport(
        uint SchemaVersion,
        string Status,
        string GameBuildId,
        string MappingSha256,
        string ContractSha256,
        string SourceInputSetSha256,
        IReadOnlyList<SourceContainerSnapshot> SourceContainers,
        IReadOnlyList<ProbeRegionDocument> MapRegions,
        IReadOnlyList<AssetInventoryEntry> AssetInventory);

    private sealed record StageState(
        uint SchemaVersion,
        string GameBuildId,
        string MappingSha256,
        string ContractSha256,
        string SourceInputSetSha256,
        IReadOnlyList<SourceContainerSnapshot> SourceContainers,
        string ContractFileName,
        string ContractFileSha256);

    private sealed record HoldoutCommitmentDocument(
        uint SchemaVersion,
        string GameBuildId,
        uint SealedHoldoutCount,
        string SealedHoldoutSha256,
        uint CenterCount,
        uint NorthWestCount,
        uint NorthEastCount,
        uint SouthWestCount,
        uint SouthEastCount);

    private sealed record CalibrationState(
        uint SchemaVersion,
        string GameBuildId,
        string MappingSha256,
        string ContractSha256,
        string LandmarksSha256,
        uint LandmarkCount,
        uint ReferenceCount,
        uint SealedHoldoutCount,
        double ReferenceMedianErrorPx,
        double ReferenceMaxErrorPx,
        double SealedHoldoutMedianErrorPx,
        double SealedHoldoutMaxErrorPx,
        string SealedHoldoutSha256,
        IReadOnlyDictionary<string, uint> ReferenceZoneCounts,
        IReadOnlyDictionary<string, uint> SealedHoldoutZoneCounts);

    private sealed record ValidatedState(
        uint SchemaVersion,
        string GameBuildId,
        string MappingSha256,
        string ContractSha256,
        string SourceInputSetSha256,
        string StagingPath,
        string ManifestSha256);

    private sealed class SnapshotInputGuard(
        string installRoot,
        IReadOnlyList<SourceContainerSnapshot> baseline) : IExtractionInputGuard
    {
        public void EnsureUnchanged()
        {
            var current = AssetPaths.DiscoverContainers(installRoot);
            var expected = baseline.ToDictionary(
                snapshot => snapshot.RelativeName,
                StringComparer.Ordinal);
            if (current.Count != expected.Count)
            {
                throw Changed();
            }
            foreach (var container in current)
            {
                if (!expected.TryGetValue(container.RelativeName, out var before))
                {
                    throw Changed();
                }
                var identity = SecureFiles.IdentityWithin(installRoot, container.FullPath);
                if (identity.SizeBytes != before.SizeBytes
                    || identity.LastWriteUtcTicks != before.LastWriteUtcTicks
                    || identity.VolumeSerial != before.VolumeSerial
                    || identity.FileIndex != before.FileIndex)
                {
                    throw Changed();
                }
            }
        }

        private static MapPackFailure Changed() =>
            new(
                ExitCodes.PackIntegrity,
                "Steam build or mounted container set changed during extraction");
    }
}

public sealed record CommandServices(
    Func<AssetProbeRequest, AssetProbeResult> Probe,
    Func<IReadOnlyList<string>, IMapAssetReader> CreateAssetReader)
{
    public static CommandServices Default { get; } = new(
        request => new Cue4ParseMapAssetReader().Probe(request),
        _ => new Cue4ParseMapAssetReader());
}
