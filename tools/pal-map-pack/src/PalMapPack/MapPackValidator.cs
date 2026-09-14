using System.Text.Json;
using System.Text.Json.Serialization;

namespace PalMapPack;

public static class MapPackValidator
{
    private const long MaximumManifestBytes = 1_048_576;
    private const long MaximumTransformBytes = 1_048_576;
    private const long MaximumPoisBytes = 16_777_216;
    private const long MaximumTileIndexBytes = 16_777_216;
    private const long MaximumTileBytes = 16_777_216;
    private const int ParityPointCount = 25;
    private const double ParityTolerancePx = 0.01;

    private static readonly JsonSerializerOptions JsonOptions = new()
    {
        PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower,
        UnmappedMemberHandling = JsonUnmappedMemberHandling.Disallow,
    };

    public static bool IsValid(
        string root,
        string expectedBuildId,
        string expectedContractSha256,
        string expectedMappingSha256)
    {
        try
        {
            EnsureValid(
                root,
                expectedBuildId,
                expectedContractSha256,
                expectedMappingSha256);
            return true;
        }
        catch (Exception error) when (error is
            ArgumentException or
            IOException or
            JsonException or
            MapPackFailure or
            NotSupportedException or
            OverflowException or
            UnauthorizedAccessException)
        {
            return false;
        }
    }

    private static void EnsureValid(
        string root,
        string expectedBuildId,
        string expectedContractSha256,
        string expectedMappingSha256)
    {
        Hashing.ValidateHash(expectedContractSha256, "expected source contract");
        Hashing.ValidateHash(expectedMappingSha256, "expected mapping");
        var fullRoot = Path.GetFullPath(root);
        using var rootLease = SecureDirectoryLease.OpenExisting(fullRoot);
        if (Path.Exists(Path.Combine(fullRoot, "transform.json"))
            || Path.Exists(Path.Combine(fullRoot, "tile-index.json"))
            || Path.Exists(Path.Combine(fullRoot, "tiles")))
        {
            throw Failure();
        }
        var manifest = ReadJson<MapPackManifestDocument>(
            fullRoot,
            "manifest.json",
            MaximumManifestBytes);
        if (manifest.SchemaVersion != 2
            || manifest.GameBuildId != expectedBuildId
            || manifest.SourceContractSha256 != expectedContractSha256
            || manifest.MappingSha256 != expectedMappingSha256
            || manifest.Cue4ParseVersion != "1.2.2.202607"
            || manifest.CoordinateTransformVersion != 2
            || manifest.PoiSchemaVersion != 2
            || manifest.TileCoreSizePx != TilePyramidBuilder.TileCoreSize
            || manifest.TileGutterPx != TilePyramidBuilder.TileGutter
            || !DateTimeOffset.TryParseExact(
                manifest.GeneratedAt,
                "yyyy-MM-dd'T'HH:mm:ss'Z'",
                System.Globalization.CultureInfo.InvariantCulture,
                System.Globalization.DateTimeStyles.AssumeUniversal
                    | System.Globalization.DateTimeStyles.AdjustToUniversal,
                out _))
        {
            throw Failure();
        }

        ValidateSourceContainers(manifest);
        var regions = ValidateMapRegions(manifest);
        var expectedFiles = new HashSet<string>(StringComparer.Ordinal);
        var expectedDirectories = new HashSet<string>(StringComparer.Ordinal)
        {
            "regions",
        };
        foreach (var region in regions)
        {
            ValidateTransform(fullRoot, manifest, region);
            ValidateTiles(
                fullRoot,
                manifest,
                region,
                expectedFiles,
                expectedDirectories);
            expectedFiles.Add(region.TransformRelativePath);
            expectedFiles.Add(region.TileIndexRelativePath);
            AddAncestorDirectories(region.TransformRelativePath, expectedDirectories);
            AddAncestorDirectories(region.TileIndexRelativePath, expectedDirectories);
        }
        ValidateRegionTree(fullRoot, expectedFiles, expectedDirectories);
        ValidatePois(fullRoot, manifest, regions);
    }

    private static void ValidateSourceContainers(MapPackManifestDocument manifest)
    {
        if (manifest.SourceContainers.Count == 0)
        {
            throw Failure();
        }
        var snapshots = manifest.SourceContainers.Select(container =>
        {
            Hashing.ValidateRelativeName(container.RelativeName);
            Hashing.ValidateHash(container.Sha256, "manifest source container");
            if (container.SizeBytes <= 0)
            {
                throw Failure();
            }
            return new SourceContainerSnapshot(
                container.RelativeName,
                container.SizeBytes,
                0,
                container.Sha256);
        }).ToArray();
        if (snapshots
                .Select(snapshot => snapshot.RelativeName)
                .Distinct(StringComparer.OrdinalIgnoreCase)
                .Count() != snapshots.Length
            || Hashing.SourceInputSetSha256(snapshots) != manifest.SourceInputSetSha256)
        {
            throw Failure();
        }
    }

    private static MapRegionDocument[] ValidateMapRegions(MapPackManifestDocument manifest)
    {
        if (manifest.MapRegions.Count == 0
            || manifest.MapRegions
                .GroupBy(
                    region => $"{region.MapId}\0{region.RegionId}",
                    StringComparer.OrdinalIgnoreCase)
                .Any(group => group.Count() != 1))
        {
            throw Failure();
        }
        var regions = manifest.MapRegions.ToArray();
        foreach (var region in regions)
        {
            var prefix = RegionPackPaths.Prefix(region.MapId, region.RegionId);
            foreach (var hash in new[]
            {
                region.MapAssetSha256,
                region.TransformSha256,
                region.TileSetSha256,
                region.TileIndexSha256,
            })
            {
                Hashing.ValidateHash(hash, "map region artifact");
            }
            if (string.IsNullOrWhiteSpace(region.SourceTexturePath)
                || !Finite(
                    region.WorldMinX,
                    region.WorldMinY,
                    region.WorldMaxX,
                    region.WorldMaxY,
                    region.BlockSizeX,
                    region.BlockSizeY,
                    region.GridPositionX,
                    region.GridPositionY)
                || region.WorldMinX >= region.WorldMaxX
                || region.WorldMinY >= region.WorldMaxY
                || region.BlockSizeX <= 0
                || region.BlockSizeY <= 0
                || region.MapWidthPx is <= 0 or > 8192
                || region.MapHeightPx is <= 0 or > 8192
                || !ValidMatrix(region.WorldToMapMatrix)
                || region.TransformRelativePath != $"{prefix}/transform.json"
                || region.TileIndexRelativePath != $"{prefix}/tile-index.json")
            {
                throw Failure();
            }
        }
        if (regions.Count(region => region.MapId == "MainMap") != 1)
        {
            throw Failure();
        }
        RegionContentPolicy.EnsureManifestNonAliasing(regions);
        return regions;
    }

    private static void ValidateTransform(
        string root,
        MapPackManifestDocument manifest,
        MapRegionDocument region)
    {
        var bytes = ReadComponent(
            root,
            region.TransformRelativePath,
            MaximumTransformBytes);
        if (Hashing.Sha256Hex(bytes) != region.TransformSha256)
        {
            throw Failure();
        }
        var transform = Deserialize<TransformDocument>(bytes);
        if (transform.SchemaVersion != 2
            || transform.GameBuildId != manifest.GameBuildId
            || transform.MapId != region.MapId
            || transform.RegionId != region.RegionId
            || transform.MapWidthPx != region.MapWidthPx
            || transform.MapHeightPx != region.MapHeightPx
            || !ValidMatrix(transform.Matrix)
            || !MatricesEqual(transform.Matrix, region.WorldToMapMatrix))
        {
            throw Failure();
        }

        if (region.MapId == "MainMap")
        {
            var calibration = transform.Calibration;
            var mainBoundsMatrix = DeriveRegionBoundsTransform(region);
            var supportedValidatedTransform =
                transform.TransformKind == "reference_fitted_affine_validated"
                || (transform.TransformKind == "authoritative_world_bounds_validated"
                    && MatricesEqual(transform.Matrix, mainBoundsMatrix));
            if (!supportedValidatedTransform
                || calibration is null
                || calibration.LandmarkCount != 15
                || calibration.ReferenceCount != 10
                || calibration.SealedHoldoutCount != 5
                || calibration.ReferenceCenterCount != 2
                || calibration.ReferenceNorthWestCount != 2
                || calibration.ReferenceNorthEastCount != 2
                || calibration.ReferenceSouthWestCount != 2
                || calibration.ReferenceSouthEastCount != 2
                || calibration.SealedHoldoutCenterCount != 1
                || calibration.SealedHoldoutNorthWestCount != 1
                || calibration.SealedHoldoutNorthEastCount != 1
                || calibration.SealedHoldoutSouthWestCount != 1
                || calibration.SealedHoldoutSouthEastCount != 1
                || !CalibrationCountsMatch(calibration)
                || !CoordinateSolver.WithinThresholds(
                    calibration.ReferenceMedianErrorPx,
                    calibration.ReferenceMaxErrorPx)
                || !CoordinateSolver.WithinThresholds(
                    calibration.SealedHoldoutMedianErrorPx,
                    calibration.SealedHoldoutMaxErrorPx)
                || calibration.ParityPoints.Count != ParityPointCount)
            {
                throw Failure();
            }
            Hashing.ValidateHash(
                calibration.SealedHoldoutSha256,
                "sealed coordinate-validation holdout");
            if (calibration.SealedHoldoutSha256 == new string('0', 64))
            {
                throw Failure();
            }
            var worldPoints = new HashSet<(long X, long Y)>();
            var expectedPoints = new HashSet<(long X, long Y)>();
            var coveredGridSlots = new HashSet<(int X, int Y)>();
            foreach (var parity in calibration.ParityPoints)
            {
                if (!Finite(
                        parity.WorldX,
                        parity.WorldY,
                        parity.ExpectedMapX,
                        parity.ExpectedMapY)
                    || parity.ExpectedMapX < 0
                    || parity.ExpectedMapX > transform.MapWidthPx
                    || parity.ExpectedMapY < 0
                    || parity.ExpectedMapY > transform.MapHeightPx
                    || !worldPoints.Add((
                        CoordinateBits(parity.WorldX),
                        CoordinateBits(parity.WorldY)))
                    || !expectedPoints.Add((
                        CoordinateBits(parity.ExpectedMapX),
                        CoordinateBits(parity.ExpectedMapY))))
                {
                    throw Failure();
                }
                var gridSlot = GridSlot(
                    parity.ExpectedMapX,
                    parity.ExpectedMapY,
                    transform.MapWidthPx,
                    transform.MapHeightPx);
                if (gridSlot is null || !coveredGridSlots.Add(gridSlot.Value))
                {
                    throw Failure();
                }
                var actual = CoordinateSolver.Project(
                    transform.Matrix,
                    parity.WorldX,
                    parity.WorldY);
                var error = Hypot(
                    actual.X - parity.ExpectedMapX,
                    actual.Y - parity.ExpectedMapY);
                if (!double.IsFinite(error) || error > ParityTolerancePx)
                {
                    throw Failure();
                }
            }
            if (coveredGridSlots.Count != ParityPointCount)
            {
                throw Failure();
            }
            return;
        }

        var derived = DeriveRegionBoundsTransform(region);
        if (transform.TransformKind != "authoritative_region_bounds"
            || transform.Calibration is not null
            || !MatricesEqual(transform.Matrix, derived))
        {
            throw Failure();
        }
    }

    private static void ValidatePois(
        string root,
        MapPackManifestDocument manifest,
        IReadOnlyList<MapRegionDocument> regions)
    {
        var bytes = ReadComponent(root, "pois.json", MaximumPoisBytes);
        if (Hashing.Sha256Hex(bytes) != manifest.PoisSha256)
        {
            throw Failure();
        }
        var pois = Deserialize<PoiDocument>(bytes);
        if (pois.SchemaVersion != 2
            || pois.GameBuildId != manifest.GameBuildId
            || pois.PoiCount != pois.Pois.Count
            || pois.Pois.Select(poi => poi.Id).Distinct(StringComparer.Ordinal).Count() != pois.Pois.Count
            || new[] { "fast_travel", "boss", "dungeon" }
                .Any(kind => pois.Pois.All(poi => poi.Kind != kind)))
        {
            throw Failure();
        }
        foreach (var poi in pois.Pois)
        {
            if (string.IsNullOrWhiteSpace(poi.Id)
                || string.IsNullOrWhiteSpace(poi.DisplayName)
                || (poi.EntityId is not null
                    && (poi.EntityId.Length is < 1 or > 128
                        || poi.EntityId.Any(character =>
                            !char.IsAsciiLetterOrDigit(character) && character != '_')))
                || poi.SourceBuildId != manifest.GameBuildId
                || !poi.Verified
                || !Finite(poi.WorldX, poi.WorldY, poi.MapX, poi.MapY))
            {
                throw Failure();
            }
            var selected = SelectRegion(regions, poi.WorldX, poi.WorldY);
            if (selected.MapId != poi.MapId || selected.RegionId != poi.RegionId)
            {
                throw Failure();
            }
            var projected = CoordinateSolver.Project(
                selected.WorldToMapMatrix,
                poi.WorldX,
                poi.WorldY);
            if (!NearlyEqual(projected.X, poi.MapX)
                || !NearlyEqual(projected.Y, poi.MapY)
                || poi.MapX < 0
                || poi.MapX > selected.MapWidthPx
                || poi.MapY < 0
                || poi.MapY > selected.MapHeightPx)
            {
                throw Failure();
            }
        }
    }

    private static void ValidateTiles(
        string root,
        MapPackManifestDocument manifest,
        MapRegionDocument region,
        ISet<string> expectedFiles,
        ISet<string> expectedDirectories)
    {
        var bytes = ReadComponent(
            root,
            region.TileIndexRelativePath,
            MaximumTileIndexBytes);
        if (Hashing.Sha256Hex(bytes) != region.TileIndexSha256)
        {
            throw Failure();
        }
        var index = Deserialize<TileIndexDocument>(bytes);
        if (index.SchemaVersion != 2
            || index.GameBuildId != manifest.GameBuildId
            || index.MapId != region.MapId
            || index.RegionId != region.RegionId
            || index.MapWidthPx != region.MapWidthPx
            || index.MapHeightPx != region.MapHeightPx
            || index.TileCoreSizePx != manifest.TileCoreSizePx
            || index.TileGutterPx != manifest.TileGutterPx
            || index.LevelCount != index.Levels.Count
            || index.TileCount != index.Tiles.Count
            || index.Tiles.Count == 0
            || index.Tiles
                .Select(tile => (tile.Level, tile.Y, tile.X))
                .Distinct()
                .Count() != index.Tiles.Count
            || Hashing.TileSetSha256(index.Tiles) != region.TileSetSha256)
        {
            throw Failure();
        }
        ValidateLevels(index);

        var prefix = RegionPackPaths.Prefix(region.MapId, region.RegionId);
        foreach (var tile in index.Tiles)
        {
            var expectedPath = $"{prefix}/tiles/{tile.Level}/{tile.Y}_{tile.X}.jpg";
            var level = index.Levels.SingleOrDefault(item => item.Level == tile.Level);
            if (level is null
                || tile.RelativePath != expectedPath
                || tile.SizeBytes is <= 0 or > MaximumTileBytes
                || !expectedFiles.Add(tile.RelativePath)
                || tile.X >= level.GridWidth
                || tile.Y >= level.GridHeight)
            {
                throw Failure();
            }
            var scale = 1UL << tile.Level;
            var span = checked((ulong)TilePyramidBuilder.TileCoreSize * scale);
            var minX = checked((ulong)tile.X * span);
            var minY = checked((ulong)tile.Y * span);
            var maxX = Math.Min(minX + span, checked((ulong)region.MapWidthPx));
            var maxY = Math.Min(minY + span, checked((ulong)region.MapHeightPx));
            if (tile.MapRect.MinX != minX
                || tile.MapRect.MinY != minY
                || tile.MapRect.MaxX != maxX
                || tile.MapRect.MaxY != maxY)
            {
                throw Failure();
            }
            Hashing.ValidateHash(tile.Sha256, "tile");
            var tileBytes = ReadComponent(root, tile.RelativePath, MaximumTileBytes);
            if (tileBytes.LongLength != tile.SizeBytes
                || Hashing.Sha256Hex(tileBytes) != tile.Sha256)
            {
                throw Failure();
            }
            AddAncestorDirectories(tile.RelativePath, expectedDirectories);
        }

        var expectedTileCount = index.Levels.Aggregate(
            0UL,
            (total, level) => checked(
                total + (ulong)level.GridWidth * level.GridHeight));
        if (expectedTileCount != checked((ulong)index.Tiles.Count))
        {
            throw Failure();
        }
    }

    private static void ValidateLevels(TileIndexDocument index)
    {
        if (index.Levels.Count == 0
            || index.Levels.Select(level => level.Level).Distinct().Count() != index.Levels.Count)
        {
            throw Failure();
        }
        var levels = index.Levels.OrderBy(level => level.Level).ToArray();
        for (var position = 0; position < levels.Length; position++)
        {
            var level = levels[position];
            if (level.Level != position || level.Level >= 32)
            {
                throw Failure();
            }
            var divisor = 1UL << level.Level;
            var expectedWidth = ((ulong)index.MapWidthPx + divisor - 1) / divisor;
            var expectedHeight = ((ulong)index.MapHeightPx + divisor - 1) / divisor;
            var expectedGridWidth =
                (expectedWidth + TilePyramidBuilder.TileCoreSize - 1)
                / TilePyramidBuilder.TileCoreSize;
            var expectedGridHeight =
                (expectedHeight + TilePyramidBuilder.TileCoreSize - 1)
                / TilePyramidBuilder.TileCoreSize;
            if (level.WidthPx != expectedWidth
                || level.HeightPx != expectedHeight
                || level.GridWidth != expectedGridWidth
                || level.GridHeight != expectedGridHeight)
            {
                throw Failure();
            }
            var isLast = position == levels.Length - 1;
            if (isLast != (level.WidthPx <= TilePyramidBuilder.TileCoreSize
                    && level.HeightPx <= TilePyramidBuilder.TileCoreSize))
            {
                throw Failure();
            }
        }
    }

    private static void ValidateRegionTree(
        string root,
        IReadOnlySet<string> expectedFiles,
        IReadOnlySet<string> expectedDirectories)
    {
        var regionRoot = Path.Combine(root, "regions");
        var actualFiles = new HashSet<string>(StringComparer.Ordinal);
        var actualDirectories = new HashSet<string>(StringComparer.Ordinal)
        {
            "regions",
        };
        var pending = new Stack<string>();
        pending.Push(regionRoot);
        while (pending.Count > 0)
        {
            var directory = pending.Pop();
            using var directoryLease = SecureDirectoryLease.OpenExisting(directory);
            foreach (var entry in new DirectoryInfo(directory).EnumerateFileSystemInfos())
            {
                if ((entry.Attributes & FileAttributes.ReparsePoint) != 0)
                {
                    throw Failure();
                }
                var relative = Path.GetRelativePath(root, entry.FullName).Replace('\\', '/');
                Hashing.ValidateRelativeName(relative);
                if (entry is DirectoryInfo child)
                {
                    if (!expectedDirectories.Contains(relative)
                        || !actualDirectories.Add(relative))
                    {
                        throw Failure();
                    }
                    pending.Push(child.FullName);
                    continue;
                }
                if (entry is not FileInfo file || !actualFiles.Add(relative))
                {
                    throw Failure();
                }
                SecureFiles.EnsureRegularFile(file.FullName);
            }
        }
        if (!actualFiles.SetEquals(expectedFiles)
            || !actualDirectories.SetEquals(expectedDirectories))
        {
            throw Failure();
        }
    }

    private static MapRegionDocument SelectRegion(
        IReadOnlyList<MapRegionDocument> regions,
        double worldX,
        double worldY)
    {
        var candidates = regions.Where(region =>
            worldX >= region.WorldMinX
            && worldX <= region.WorldMaxX
            && worldY >= region.WorldMinY
            && worldY <= region.WorldMaxY).ToArray();
        if (candidates.Length == 0)
        {
            throw Failure();
        }
        var priority = candidates.Max(region => region.Priority);
        var selected = candidates.Where(region => region.Priority == priority).ToArray();
        return selected.Length == 1 ? selected[0] : throw Failure();
    }

    private static double[][] DeriveRegionBoundsTransform(MapRegionDocument region)
    {
        var spanX = region.WorldMaxX - region.WorldMinX;
        var spanY = region.WorldMaxY - region.WorldMinY;
        var xScale = region.MapWidthPx / spanY;
        var yScale = region.MapHeightPx / spanX;
        return
        [
            [0.0, xScale, -region.WorldMinY * xScale],
            [-yScale, 0.0, region.WorldMaxX * yScale],
        ];
    }

    private static void AddAncestorDirectories(string relativePath, ISet<string> directories)
    {
        var slash = relativePath.LastIndexOf('/');
        while (slash > 0)
        {
            var directory = relativePath[..slash];
            directories.Add(directory);
            slash = directory.LastIndexOf('/');
        }
    }

    private static T ReadJson<T>(string root, string relativeName, long maximumBytes) =>
        Deserialize<T>(ReadComponent(root, relativeName, maximumBytes));

    private static T Deserialize<T>(byte[] bytes) =>
        JsonSerializer.Deserialize<T>(bytes, JsonOptions)
        ?? throw new JsonException("empty pack component");

    private static byte[] ReadComponent(string root, string relativeName, long maximumBytes)
    {
        Hashing.ValidateRelativeName(relativeName);
        var path = Path.GetFullPath(Path.Combine(
            root,
            relativeName.Replace('/', Path.DirectorySeparatorChar)));
        if (!SourceContainerSnapshot.IsWithin(root, path))
        {
            throw Failure();
        }
        var before = SecureFiles.SnapshotWithin(root, path);
        if (before.SizeBytes > maximumBytes)
        {
            throw Failure();
        }
        var bytes = SecureFiles.ReadAllBytes(
            before.FinalPath,
            checked((int)maximumBytes));
        var after = SecureFiles.SnapshotWithin(root, path);
        if (before.VolumeSerial != after.VolumeSerial
            || before.FileIndex != after.FileIndex
            || before.SizeBytes != after.SizeBytes
            || before.Sha256 != after.Sha256
            || bytes.LongLength != before.SizeBytes
            || Hashing.Sha256Hex(bytes) != before.Sha256)
        {
            throw Failure();
        }
        return bytes;
    }

    private static bool Finite(params double[] values) => values.All(double.IsFinite);

    private static bool CalibrationCountsMatch(CalibrationDocument calibration) =>
        (ulong)calibration.ReferenceCount + calibration.SealedHoldoutCount
            == calibration.LandmarkCount
        && (ulong)calibration.ReferenceCenterCount
            + calibration.ReferenceNorthWestCount
            + calibration.ReferenceNorthEastCount
            + calibration.ReferenceSouthWestCount
            + calibration.ReferenceSouthEastCount
            == calibration.ReferenceCount
        && (ulong)calibration.SealedHoldoutCenterCount
            + calibration.SealedHoldoutNorthWestCount
            + calibration.SealedHoldoutNorthEastCount
            + calibration.SealedHoldoutSouthWestCount
            + calibration.SealedHoldoutSouthEastCount
            == calibration.SealedHoldoutCount;

    private static long CoordinateBits(double value) =>
        BitConverter.DoubleToInt64Bits(value == 0.0 ? 0.0 : value);

    private static (int X, int Y)? GridSlot(
        double expectedX,
        double expectedY,
        int mapWidthPx,
        int mapHeightPx)
    {
        (int X, int Y)? match = null;
        for (var gridY = 0; gridY <= 4; gridY++)
        {
            for (var gridX = 0; gridX <= 4; gridX++)
            {
                var slotX = mapWidthPx * gridX / 4.0;
                var slotY = mapHeightPx * gridY / 4.0;
                if (Hypot(expectedX - slotX, expectedY - slotY) > ParityTolerancePx)
                {
                    continue;
                }
                if (match is not null)
                {
                    return null;
                }
                match = (gridX, gridY);
            }
        }
        return match;
    }

    private static double Hypot(double x, double y)
    {
        var absoluteX = Math.Abs(x);
        var absoluteY = Math.Abs(y);
        var scale = Math.Max(absoluteX, absoluteY);
        if (scale == 0)
        {
            return 0;
        }
        var scaledX = absoluteX / scale;
        var scaledY = absoluteY / scale;
        return scale * Math.Sqrt(scaledX * scaledX + scaledY * scaledY);
    }

    private static bool ValidMatrix(double[][] matrix)
    {
        if (matrix is not { Length: 2 }
            || matrix.Any(row =>
                row is not { Length: 3 }
                || row.Any(value => !double.IsFinite(value))))
        {
            return false;
        }
        var scale = new[]
        {
            Math.Abs(matrix[0][0]),
            Math.Abs(matrix[0][1]),
            Math.Abs(matrix[1][0]),
            Math.Abs(matrix[1][1]),
        }.Max();
        if (scale == 0)
        {
            return false;
        }
        var determinant =
            matrix[0][0] / scale * (matrix[1][1] / scale)
            - matrix[0][1] / scale * (matrix[1][0] / scale);
        return double.IsFinite(determinant)
            && Math.Abs(determinant) > 1e-12;
    }

    private static bool MatricesEqual(double[][] left, double[][] right) =>
        ValidMatrix(left)
        && ValidMatrix(right)
        && left.SelectMany(row => row)
            .SequenceEqual(right.SelectMany(row => row));

    private static bool NearlyEqual(double left, double right) =>
        Math.Abs(left - right) <= 1e-9;

    private static MapPackFailure Failure() =>
        new(ExitCodes.PackIntegrity, "map pack integrity validation failed");
}
