namespace PalMapPack;

public sealed record MapPackBuildRequest(
    string DatasetRoot,
    string GameBuildId,
    string MappingsPath,
    byte[] MappingBytes,
    byte[] ContractBytes,
    AssetContract Contract,
    IReadOnlyList<SourceContainerSnapshot> SourceContainers,
    IReadOnlyList<SourceContainerPath> SourceContainerPaths,
    IReadOnlyList<Landmark> Landmarks,
    IMapAssetReader AssetReader,
    DateTimeOffset GeneratedAt,
    string ExtractorVersion,
    string ExtractorCommit,
    AssetInventory Inventory,
    IExtractionInputGuard? InputGuard = null);

public sealed record MapPackBuildResult(
    string StagingPath,
    MapPackManifestDocument Manifest,
    IReadOnlyList<string> WriteOrder);

public static class MapPackBuilder
{
    public static MapPackBuildResult BuildStaging(MapPackBuildRequest request)
    {
        ArgumentNullException.ThrowIfNull(request);
        if (request.Contract.GameBuildId != request.GameBuildId
            || !request.Contract.Reviewed
            || string.IsNullOrWhiteSpace(request.Contract.ReviewId))
        {
            throw new MapPackFailure(
                ExitCodes.AssetContractMismatch,
                "Build-specific asset contract has not been reviewed/accepted");
        }
        if (request.MappingBytes.Length == 0 || string.IsNullOrWhiteSpace(request.MappingsPath))
        {
            throw new MapPackFailure(ExitCodes.MissingMapping, "a separately supplied build-matched .usmap is required");
        }
        if (request.SourceContainers.Count == 0)
        {
            throw new MapPackFailure(ExitCodes.MountSerialization, "complete mounted container provenance is required");
        }
        ValidateContainerBindings(request.SourceContainers, request.SourceContainerPaths);
        AssetCatalogProbe.EnsureContractAccepted(
            request.Contract,
            request.GameBuildId,
            request.Inventory);
        if (string.IsNullOrWhiteSpace(request.Contract.ApprovedMappingSha256)
            || !string.Equals(
                request.Contract.ApprovedMappingSha256,
                Hashing.Sha256Hex(request.MappingBytes),
                StringComparison.Ordinal))
        {
            throw new MapPackFailure(
                ExitCodes.AssetContractMismatch,
                "supplied mapping hash does not match the reviewed exact-Build contract");
        }

        request.InputGuard?.EnsureUnchanged();
        var assets = request.AssetReader.Read(new AssetReadRequest(
            request.GameBuildId,
            request.MappingsPath,
            request.Contract,
            request.SourceContainerPaths
                .Select(container => Path.GetFullPath(container.FullPath))
                .ToArray()));
        if (assets.Regions.All.Any(region => region.Map.Width > 8192 || region.Map.Height > 8192))
        {
            throw new MapPackFailure(ExitCodes.PackIntegrity, "decoded RGBA map exceeds the dimension cap");
        }
        RegionContentPolicy.EnsureDecodedNonAliasing(assets.Regions);
        var runtimeRegions = CreateRuntimeRegions(assets.Regions, request.Contract);
        var mainRegion = runtimeRegions.All.Single(region =>
            region.MapId == "MainMap" && region.RegionId == "FirstRegion");
        var coordinateValidation = CoordinateSolver.ValidateAuthoritativeBounds(
            mainRegion,
            request.Landmarks,
            request.GameBuildId);
        if (!coordinateValidation.Accepted)
        {
            throw new MapPackFailure(
                ExitCodes.CalibrationRejected,
                coordinateValidation.RejectionReason);
        }
        if (!string.Equals(
                coordinateValidation.SealedHoldoutSha256,
                request.Contract.ApprovedSealedHoldoutSha256,
                StringComparison.Ordinal))
        {
            throw new MapPackFailure(
                ExitCodes.CalibrationRejected,
                "sealed holdout does not match the commitment in the reviewed exact-Build contract");
        }
        var orderedRegions = runtimeRegions.All
            .OrderBy(region => region.Priority)
            .ThenBy(region => region.MapId, StringComparer.Ordinal)
            .ThenBy(region => region.RegionId, StringComparer.Ordinal)
            .ToArray();
        var transforms = orderedRegions.ToDictionary(
            region => (region.MapId, region.RegionId),
            CoordinateSolver.DeriveAuthoritativeBoundsTransform);
        transforms[(mainRegion.MapId, mainRegion.RegionId)] =
            coordinateValidation.Matrix.Select(row => row.ToArray()).ToArray();
        var poiResult = PoiExtractor.Extract(
            assets.PoiRows,
            assets.Regions,
            transforms,
            request.GameBuildId);

        var staging = AtomicPublisher.CreateStagingDirectory(request.DatasetRoot, request.GameBuildId);
        var writes = new List<string>();
        var regionDocuments = new List<MapRegionDocument>(orderedRegions.Length);
        foreach (var region in orderedRegions)
        {
            var prefix = RegionPackPaths.Prefix(region.MapId, region.RegionId);
            var matrix = transforms[(region.MapId, region.RegionId)];
            var isValidatedMain = ReferenceEquals(region, mainRegion);
            CalibrationDocument? calibrationDocument = null;
            if (isValidatedMain)
            {
                var parity = CoordinateSolver.CreateParityPoints(
                    matrix,
                    region.Map.Width,
                    region.Map.Height);
                var referenceZones = coordinateValidation.ReferenceZoneCounts;
                var holdoutZones = coordinateValidation.SealedHoldoutZoneCounts;
                calibrationDocument = new CalibrationDocument(
                    checked((uint)request.Landmarks.Count),
                    coordinateValidation.ReferenceCount,
                    coordinateValidation.SealedHoldoutCount,
                    checked((uint)referenceZones[LandmarkZone.Center]),
                    checked((uint)referenceZones[LandmarkZone.NorthWest]),
                    checked((uint)referenceZones[LandmarkZone.NorthEast]),
                    checked((uint)referenceZones[LandmarkZone.SouthWest]),
                    checked((uint)referenceZones[LandmarkZone.SouthEast]),
                    checked((uint)holdoutZones[LandmarkZone.Center]),
                    checked((uint)holdoutZones[LandmarkZone.NorthWest]),
                    checked((uint)holdoutZones[LandmarkZone.NorthEast]),
                    checked((uint)holdoutZones[LandmarkZone.SouthWest]),
                    checked((uint)holdoutZones[LandmarkZone.SouthEast]),
                    coordinateValidation.ReferenceMedianErrorPixels,
                    coordinateValidation.ReferenceMaxErrorPixels,
                    coordinateValidation.SealedHoldoutMedianErrorPixels,
                    coordinateValidation.SealedHoldoutMaxErrorPixels,
                    coordinateValidation.SealedHoldoutSha256,
                    parity);
            }

            var pyramid = TilePyramidBuilder.Build(
                region.Map,
                staging,
                request.GameBuildId,
                region.MapId,
                region.RegionId);
            writes.AddRange(pyramid.Tiles.Select(tile => tile.RelativePath));

            var transform = new TransformDocument(
                2,
                request.GameBuildId,
                region.MapId,
                region.RegionId,
                region.Map.Width,
                region.Map.Height,
                isValidatedMain
                    ? "authoritative_world_bounds_validated"
                    : "authoritative_region_bounds",
                matrix,
                calibrationDocument);
            var transformBytes = ManifestWriter.SerializeBytes(transform);
            var transformRelativePath = $"{prefix}/transform.json";
            WriteComponent(staging, transformRelativePath, transformBytes, writes);

            var tileIndex = pyramid.ToDocument(request.GameBuildId);
            var tileIndexBytes = ManifestWriter.SerializeBytes(tileIndex);
            var tileIndexRelativePath = $"{prefix}/tile-index.json";
            WriteComponent(staging, tileIndexRelativePath, tileIndexBytes, writes);

            regionDocuments.Add(new MapRegionDocument(
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
                Hashing.MapAssetSha256(region.Map),
                CloneMatrix(matrix),
                transformRelativePath,
                Hashing.Sha256Hex(transformBytes),
                tileIndexRelativePath,
                Hashing.TileSetSha256(pyramid.Tiles),
                Hashing.Sha256Hex(tileIndexBytes)));
        }

        var runtimePois = poiResult.Pois.Select(poi => new RuntimePoi(
            poi.Id,
            poi.Kind,
            poi.DisplayName,
            poi.EntityId,
            poi.MapId,
            poi.RegionId,
            poi.WorldX,
            poi.WorldY,
            poi.MapX,
            poi.MapY,
            poi.SourceBuildId,
            poi.Verified)).ToArray();
        var poiDocument = new PoiDocument(
            2,
            request.GameBuildId,
            checked((uint)runtimePois.Length),
            runtimePois);
        var poiBytes = ManifestWriter.SerializeBytes(poiDocument);
        WriteComponent(staging, "pois.json", poiBytes, writes);

        var reports = Path.Combine(staging, "reports");
        Directory.CreateDirectory(reports);
        var gateReport = new
        {
            schema_version = 2,
            status = "STAGED_NOT_PUBLISHED",
            game_build_id = request.GameBuildId,
            coordinate_validation_accepted = true,
            reference_count = coordinateValidation.ReferenceCount,
            sealed_holdout_count = coordinateValidation.SealedHoldoutCount,
            sealed_holdout_sha256 = coordinateValidation.SealedHoldoutSha256,
            coordinate_validation_residuals = coordinateValidation.Residuals,
            map_regions = regionDocuments.Select(region => new
            {
                region.MapId,
                region.RegionId,
                region.MapAssetSha256,
                region.TransformSha256,
                region.TileSetSha256,
                region.TileIndexSha256,
            }),
            poi_accounting = poiResult.SourceAccounting,
            poi_provenance = poiResult.Pois.Select(poi => new
            {
                poi.Id,
                poi.MapId,
                poi.RegionId,
                poi.SourceAssetPath,
                poi.SourceRowKey,
                poi.SourceRowKeys,
                poi.ExtractionRule,
                poi.SourceBuildId,
            }),
        };
        WriteComponent(
            staging,
            "reports/gate-b.json",
            ManifestWriter.SerializeBytes(gateReport),
            writes);

        var manifest = ManifestWriter.Create(
            request.GameBuildId,
            request.SourceContainers,
            Hashing.Sha256Hex(request.ContractBytes),
            Hashing.Sha256Hex(request.MappingBytes),
            regionDocuments,
            Hashing.Sha256Hex(poiBytes),
            request.ExtractorVersion,
            request.ExtractorCommit,
            request.GeneratedAt);
        request.InputGuard?.EnsureUnchanged();
        WriteComponent(staging, "manifest.json", ManifestWriter.SerializeBytes(manifest), writes);
        return new MapPackBuildResult(staging, manifest, writes);
    }

    private static double[][] CloneMatrix(double[][] matrix) =>
        matrix.Select(row => row.ToArray()).ToArray();

    internal static MapRegionSet CreateRuntimeRegions(
        MapRegionSet sourceRegions,
        AssetContract contract)
    {
        var contractRegions = contract.MapRegions.ToDictionary(
            region => (region.MapId, region.RegionId));
        var regions = sourceRegions.All.Select(source =>
        {
            if (source.Map.Width <= 4_096 && source.Map.Height <= 4_096)
            {
                return source;
            }
            var bound = contractRegions[(source.MapId, source.RegionId)];
            var dimensions = CalibrationPreviewBuilder.SelectDimensions(
                source.Map,
                bound.CalibrationPreview);
            var sourceHash = Hashing.MapAssetSha256(source.Map);
            var runtimeMap = CalibrationPreviewBuilder.BuildDeterministicRuntimeMap(
                source.Map,
                sourceHash,
                dimensions.Width,
                dimensions.Height);
            return source with { Map = runtimeMap };
        });
        return new MapRegionSet(regions);
    }

    private static void ValidateContainerBindings(
        IReadOnlyList<SourceContainerSnapshot> snapshots,
        IReadOnlyList<SourceContainerPath> paths)
    {
        if (paths.Count != snapshots.Count
            || paths.Count == 0
            || snapshots.Select(snapshot => snapshot.RelativeName)
                .Distinct(StringComparer.OrdinalIgnoreCase)
                .Count() != snapshots.Count
            || paths.Select(path => path.RelativeName)
                .Distinct(StringComparer.OrdinalIgnoreCase)
                .Count() != paths.Count
            || paths.Select(path => Path.GetFullPath(path.FullPath))
                .Distinct(StringComparer.OrdinalIgnoreCase)
                .Count() != paths.Count)
        {
            throw new MapPackFailure(
                ExitCodes.MountSerialization,
                "source container paths do not bind the complete provenance set");
        }
        var expected = snapshots.ToDictionary(
            snapshot => snapshot.RelativeName,
            StringComparer.OrdinalIgnoreCase);
        string? sharedRoot = null;
        foreach (var container in paths)
        {
            Hashing.ValidateRelativeName(container.RelativeName);
            if (!expected.TryGetValue(container.RelativeName, out var snapshot)
                || snapshot.RelativeName != container.RelativeName
                || !Path.IsPathFullyQualified(container.FullPath))
            {
                throw new MapPackFailure(
                    ExitCodes.MountSerialization,
                    "source container path is not an exact absolute provenance binding");
            }
            var fullPath = Path.GetFullPath(container.FullPath);
            var relativeSuffix = container.RelativeName.Replace(
                '/',
                Path.DirectorySeparatorChar);
            var suffix = Path.DirectorySeparatorChar + relativeSuffix;
            if (!fullPath.EndsWith(suffix, StringComparison.OrdinalIgnoreCase))
            {
                throw new MapPackFailure(
                    ExitCodes.MountSerialization,
                    "source container path does not match its manifest-relative identity");
            }
            var root = fullPath[..^suffix.Length];
            if (string.IsNullOrEmpty(root)
                || (sharedRoot is not null
                    && !string.Equals(root, sharedRoot, StringComparison.OrdinalIgnoreCase)))
            {
                throw new MapPackFailure(
                    ExitCodes.MountSerialization,
                    "source containers do not share one secured install root");
            }
            sharedRoot ??= root;
        }
    }

    private static void WriteComponent(
        string staging,
        string relativeName,
        byte[] bytes,
        ICollection<string> writeOrder)
    {
        Hashing.ValidateRelativeName(relativeName);
        var path = Path.Combine(staging, relativeName.Replace('/', Path.DirectorySeparatorChar));
        Directory.CreateDirectory(Path.GetDirectoryName(path)!);
        File.WriteAllBytes(path, bytes);
        writeOrder.Add(relativeName);
    }
}

public enum PublishCheckpoint
{
    StagingValidated,
    VersionInstalled,
    BeforeAtomicActivation,
    AfterAtomicActivation,
}

public static class AtomicPublisher
{
    private const int MaximumPointerBytes = 16 * 1024;
    private static readonly System.Text.Json.JsonSerializerOptions PointerJsonOptions = new()
    {
        PropertyNamingPolicy = System.Text.Json.JsonNamingPolicy.SnakeCaseLower,
        UnmappedMemberHandling = System.Text.Json.Serialization.JsonUnmappedMemberHandling.Disallow,
    };

    public static string CreateStagingDirectory(string datasetRoot, string buildId)
    {
        if (string.IsNullOrEmpty(buildId) || buildId.Any(character => !char.IsAsciiDigit(character)))
        {
            throw new MapPackFailure(ExitCodes.PackIntegrity, "staging Build identity is invalid");
        }
        var root = Path.GetFullPath(datasetRoot);
        using var rootLease = Directory.Exists(root)
            ? SecureDirectoryLease.OpenExisting(root)
            : SecureDirectoryLease.Create(root);
        var staging = Path.Combine(root, $".{buildId}.staging-{Guid.NewGuid():N}");
        Directory.CreateDirectory(staging);
        using var stagingLease = SecureDirectoryLease.OpenExisting(staging);
        return staging;
    }

    public static void Publish(string stagingPath, string targetPath, Func<string, bool> validate)
    {
        Publish(stagingPath, targetPath, validate, _ => { });
    }

    public static void Publish(
        string stagingPath,
        string targetPath,
        Func<string, bool> validate,
        Action<PublishCheckpoint> checkpoint)
    {
        ArgumentNullException.ThrowIfNull(validate);
        ArgumentNullException.ThrowIfNull(checkpoint);
        var staging = Path.GetFullPath(stagingPath);
        var target = Path.GetFullPath(targetPath);
        var root = Directory.GetParent(staging)?.FullName
            ?? throw new MapPackFailure(ExitCodes.PackIntegrity, "staging has no dataset root");
        if (!string.Equals(root, Directory.GetParent(target)?.FullName, StringComparison.OrdinalIgnoreCase)
            || !SourceContainerSnapshot.IsWithin(root, staging)
            || !SourceContainerSnapshot.IsWithin(root, target))
        {
            throw new MapPackFailure(ExitCodes.PackIntegrity, "staging and target must share the dataset root");
        }
        var buildId = Path.GetFileName(target);
        ValidateBuildId(buildId);
        using var rootLease = SecureDirectoryLease.OpenExisting(root);
        using var stagingLease = SecureDirectoryLease.OpenMovable(staging);
        var stagingManifest = Path.Combine(staging, "manifest.json");
        if (!File.Exists(stagingManifest))
        {
            throw new MapPackFailure(ExitCodes.PackIntegrity, "staged pack failed integrity validation");
        }
        SecureFiles.EnsureRegularFile(stagingManifest);
        if (!validate(staging))
        {
            throw new MapPackFailure(ExitCodes.PackIntegrity, "staged pack failed integrity validation");
        }
        checkpoint(PublishCheckpoint.StagingValidated);

        var pointerPath = PointerPath(root, buildId);
        if (File.Exists(pointerPath))
        {
            var active = ResolveActivePath(root, buildId);
            if (!validate(active))
            {
                throw new MapPackFailure(
                    ExitCodes.PackIntegrity,
                    "existing active pack failed integrity validation");
            }
        }
        else if (Directory.Exists(target))
        {
            throw new MapPackFailure(
                ExitCodes.PackIntegrity,
                "legacy directory activation cannot be replaced crash-atomically");
        }

        var manifestHash = Hashing.Sha256File(stagingManifest);
        var versionsRoot = Path.Combine(root, ".versions");
        using var versionsLease = Directory.Exists(versionsRoot)
            ? SecureDirectoryLease.OpenExisting(versionsRoot)
            : SecureDirectoryLease.Create(versionsRoot);
        var versionName = $"{buildId}-{Guid.NewGuid():N}";
        var versionPath = Path.Combine(versionsRoot, versionName);
        stagingLease.MoveNoReplace(versionPath);
        using var versionLease = SecureDirectoryLease.OpenExisting(versionPath);
        if (!validate(versionPath))
        {
            throw new MapPackFailure(
                ExitCodes.PackIntegrity,
                "installed immutable version failed integrity validation");
        }
        checkpoint(PublishCheckpoint.VersionInstalled);

        var relativeVersion = Path.GetRelativePath(root, versionPath).Replace('\\', '/');
        Hashing.ValidateRelativeName(relativeVersion);
        var pointer = new ActivePackPointer(
            1,
            buildId,
            relativeVersion,
            manifestHash);
        var pointerBytes = ManifestWriter.SerializeBytes(pointer);
        var temporaryPointer = Path.Combine(
            root,
            $".{buildId}.active-{Guid.NewGuid():N}.tmp");
        SecureFiles.WriteNewFile(root, temporaryPointer, pointerBytes);
        if (!PointerResolvesTo(
                temporaryPointer,
                root,
                buildId,
                versionPath,
                manifestHash))
        {
            throw new MapPackFailure(
                ExitCodes.PackIntegrity,
                "new active-pack pointer failed integrity validation");
        }
        checkpoint(PublishCheckpoint.BeforeAtomicActivation);
        NativeMethods.AtomicReplaceFile(temporaryPointer, pointerPath);
        checkpoint(PublishCheckpoint.AfterAtomicActivation);
        var activated = ResolveActivePath(root, buildId);
        if (!string.Equals(activated, versionPath, StringComparison.OrdinalIgnoreCase)
            || !validate(activated))
        {
            throw new MapPackFailure(
                ExitCodes.PackIntegrity,
                "atomically activated pointer failed post-swap validation");
        }
    }

    public static string ResolveActivePath(string datasetRoot, string buildId)
    {
        ValidateBuildId(buildId);
        var root = Path.GetFullPath(datasetRoot);
        using var rootLease = SecureDirectoryLease.OpenExisting(root);
        var pointerPath = PointerPath(root, buildId);
        var pointer = ReadPointer(pointerPath);
        if (pointer.SchemaVersion != 1
            || !string.Equals(pointer.GameBuildId, buildId, StringComparison.Ordinal))
        {
            throw new MapPackFailure(
                ExitCodes.PackIntegrity,
                "active-pack pointer identity is invalid");
        }
        Hashing.ValidateRelativeName(pointer.RelativeVersionPath);
        Hashing.ValidateHash(pointer.ManifestSha256, "active-pack manifest");
        var versionPath = Path.GetFullPath(Path.Combine(
            root,
            pointer.RelativeVersionPath.Replace('/', Path.DirectorySeparatorChar)));
        if (!SourceContainerSnapshot.IsWithin(root, versionPath)
            || !SourceContainerSnapshot.IsWithin(Path.Combine(root, ".versions"), versionPath))
        {
            throw new MapPackFailure(
                ExitCodes.PackIntegrity,
                "active-pack pointer escaped the immutable version root");
        }
        using var versionLease = SecureDirectoryLease.OpenExisting(versionPath);
        var manifestPath = Path.Combine(versionPath, "manifest.json");
        if (!File.Exists(manifestPath)
            || !string.Equals(
                Hashing.Sha256File(manifestPath),
                pointer.ManifestSha256,
                StringComparison.Ordinal))
        {
            throw new MapPackFailure(
                ExitCodes.PackIntegrity,
                "active-pack pointer references an invalid immutable version");
        }
        return versionPath;
    }

    private static bool PointerResolvesTo(
        string pointerPath,
        string root,
        string buildId,
        string expectedVersion,
        string expectedManifestHash)
    {
        var pointer = ReadPointer(pointerPath);
        var resolved = Path.GetFullPath(Path.Combine(
            root,
            pointer.RelativeVersionPath.Replace('/', Path.DirectorySeparatorChar)));
        return pointer.SchemaVersion == 1
            && pointer.GameBuildId == buildId
            && pointer.ManifestSha256 == expectedManifestHash
            && string.Equals(resolved, expectedVersion, StringComparison.OrdinalIgnoreCase);
    }

    private static ActivePackPointer ReadPointer(string path)
    {
        try
        {
            var bytes = SecureFiles.ReadAllBytes(path, MaximumPointerBytes);
            return System.Text.Json.JsonSerializer.Deserialize<ActivePackPointer>(
                bytes,
                PointerJsonOptions)
                ?? throw new System.Text.Json.JsonException("empty pointer");
        }
        catch (Exception error) when (error is
            IOException or
            System.Text.Json.JsonException or
            MapPackFailure)
        {
            throw new MapPackFailure(
                ExitCodes.PackIntegrity,
                "active-pack pointer is missing, unsafe, or invalid");
        }
    }

    private static string PointerPath(string root, string buildId) =>
        Path.Combine(root, $"{buildId}.active.json");

    private static void ValidateBuildId(string buildId)
    {
        if (string.IsNullOrEmpty(buildId)
            || buildId.Any(character => !char.IsAsciiDigit(character)))
        {
            throw new MapPackFailure(
                ExitCodes.PackIntegrity,
                "active-pack Build identity is invalid");
        }
    }

    private sealed record ActivePackPointer(
        uint SchemaVersion,
        string GameBuildId,
        string RelativeVersionPath,
        string ManifestSha256);
}
