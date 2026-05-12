param(
    [string]$ProfilePath = ".\assets\profiles\higher-ground.json",
    [string]$AssetRoot,
    [string]$RequiredRoot,
    [string]$RepositoryRoot,
    [string]$NwsyncTool = ".\third_party\nwsync.windows.i386\nwsync_write.exe",
    [string]$SevenZip = "C:\Program Files\7-Zip\7z.exe",
    [switch]$Apply,
    [switch]$Force,
    [switch]$HashStagedFiles,
    [switch]$SkipNwsync
)

$ErrorActionPreference = "Stop"

function Resolve-ProjectPath {
    param([string]$PathValue, [string]$BasePath = (Get-Location).Path)
    if ([string]::IsNullOrWhiteSpace($PathValue)) {
        return $null
    }
    if ([System.IO.Path]::IsPathRooted($PathValue)) {
        return [System.IO.Path]::GetFullPath($PathValue)
    }
    return [System.IO.Path]::GetFullPath((Join-Path $BasePath $PathValue))
}

function Ensure-Directory {
    param([string]$PathValue)
    if (-not (Test-Path -LiteralPath $PathValue)) {
        New-Item -ItemType Directory -Force -Path $PathValue | Out-Null
    }
}

function Read-JsonProfile {
    param([string]$PathValue)
    if (-not (Test-Path -LiteralPath $PathValue)) {
        throw "Asset profile not found: $PathValue"
    }
    return Get-Content -LiteralPath $PathValue -Raw | ConvertFrom-Json
}

function Get-FileSha256 {
    param([string]$PathValue)
    return (Get-FileHash -LiteralPath $PathValue -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Get-RelativePathCompat {
    param([string]$BasePath, [string]$PathValue)
    $baseFull = [System.IO.Path]::GetFullPath($BasePath)
    if (-not $baseFull.EndsWith([System.IO.Path]::DirectorySeparatorChar)) {
        $baseFull += [System.IO.Path]::DirectorySeparatorChar
    }
    $pathFull = [System.IO.Path]::GetFullPath($PathValue)
    $baseUri = New-Object System.Uri($baseFull)
    $pathUri = New-Object System.Uri($pathFull)
    return [System.Uri]::UnescapeDataString($baseUri.MakeRelativeUri($pathUri).ToString()).Replace("/", "\")
}

function Link-Or-Copy {
    param([string]$Source, [string]$Destination, [switch]$Force)

    Ensure-Directory (Split-Path -Parent $Destination)
    if ([System.IO.Path]::GetFullPath($Source) -ieq [System.IO.Path]::GetFullPath($Destination)) {
        return
    }
    if (Test-Path -LiteralPath $Destination) {
        if (-not $Force) {
            return
        }
        Remove-Item -LiteralPath $Destination -Force
    }

    try {
        New-Item -ItemType HardLink -Path $Destination -Target $Source -Force | Out-Null
    } catch {
        Copy-Item -LiteralPath $Source -Destination $Destination -Force
    }
}

function Invoke-ArchiveExtract {
    param([string]$Archive, [string]$Destination)

    if (-not (Test-Path -LiteralPath $SevenZip)) {
        throw "7-Zip was not found at '$SevenZip'; pass -SevenZip with a valid extractor."
    }
    Ensure-Directory $Destination
    & $SevenZip x "-o$Destination" -y $Archive | Out-Null
    if ($LASTEXITCODE -ne 0) {
        throw "7-Zip failed extracting $Archive"
    }
}

function Remove-StaleProfileFiles {
    param(
        [string]$StageRoot,
        [string]$Subdir,
        [string]$Extension,
        [string[]]$KeepNames
    )

    $dir = Join-Path $StageRoot $Subdir
    if (-not (Test-Path -LiteralPath $dir -PathType Container)) {
        return
    }

    $stageFull = [System.IO.Path]::GetFullPath($StageRoot)
    $dirFull = [System.IO.Path]::GetFullPath($dir)
    if (-not $dirFull.StartsWith($stageFull, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "Refusing to clean staged profile files outside stage root: $dirFull"
    }

    $keep = @{}
    foreach ($name in $KeepNames) {
        $keep[$name.ToLowerInvariant()] = $true
    }

    Get-ChildItem -LiteralPath $dir -File -Filter "*$Extension" | ForEach-Object {
        if (-not $keep.ContainsKey($_.Name.ToLowerInvariant())) {
            Remove-Item -LiteralPath $_.FullName -Force
        }
    }
}

function Copy-ExtractedAssetsToStage {
    param([string]$ExtractRoot, [string]$StageRoot, [switch]$Force)

    $categoryByExtension = @{
        ".hak" = "hak"
        ".tlk" = "tlk"
        ".bmu" = "music"
        ".mp3" = "music"
        ".dds" = "texturepacks"
        ".tga" = "texturepacks"
    }

    Get-ChildItem -LiteralPath $ExtractRoot -Recurse -File | ForEach-Object {
        $ext = $_.Extension.ToLowerInvariant()
        if ($categoryByExtension.ContainsKey($ext)) {
            $category = $categoryByExtension[$ext]
            Link-Or-Copy -Source $_.FullName -Destination (Join-Path $StageRoot "$category\$($_.Name)") -Force:$Force
        }
    }
}

function Find-StagedFile {
    param(
        [string]$FileName,
        [string[]]$Roots
    )

    foreach ($root in $Roots) {
        if ([string]::IsNullOrWhiteSpace($root) -or -not (Test-Path -LiteralPath $root)) {
            continue
        }
        $direct = Join-Path $root $FileName
        if (Test-Path -LiteralPath $direct) {
            return [System.IO.Path]::GetFullPath($direct)
        }
        $found = Get-ChildItem -LiteralPath $root -Recurse -File -Filter $FileName -ErrorAction SilentlyContinue | Select-Object -First 1
        if ($found) {
            return $found.FullName
        }
    }
    return $null
}

function Read-ErfResource {
    param(
        [string]$ContainerPath,
        [string]$ResourceName
    )

    $targetResref = [System.IO.Path]::GetFileNameWithoutExtension($ResourceName).ToLowerInvariant()
    $fs = [System.IO.File]::OpenRead($ContainerPath)
    try {
        $br = New-Object System.IO.BinaryReader($fs)
        $fileType = [System.Text.Encoding]::ASCII.GetString($br.ReadBytes(4))
        $version = [System.Text.Encoding]::ASCII.GetString($br.ReadBytes(4))
        if ($fileType -notin @("HAK ", "ERF ", "MOD ") -or $version -ne "V1.0") {
            throw "Unsupported ERF container '$ContainerPath' type='$fileType' version='$version'"
        }

        $languageCount = $br.ReadUInt32()
        $localizedStringSize = $br.ReadUInt32()
        $entryCount = $br.ReadUInt32()
        $localizedStringOffset = $br.ReadUInt32()
        $keyListOffset = $br.ReadUInt32()
        $resourceListOffset = $br.ReadUInt32()
        [void]$languageCount
        [void]$localizedStringSize
        [void]$localizedStringOffset
        $fs.Seek($keyListOffset, [System.IO.SeekOrigin]::Begin) | Out-Null

        $matches = @()
        for ($index = 0; $index -lt $entryCount; $index++) {
            $resrefBytes = $br.ReadBytes(16)
            $resref = ([System.Text.Encoding]::ASCII.GetString($resrefBytes)).TrimEnd([char]0).ToLowerInvariant()
            $resourceId = $br.ReadUInt32()
            $resourceType = $br.ReadUInt16()
            [void]$br.ReadUInt16()
            if ($resref -eq $targetResref) {
                $matches += [pscustomobject]@{
                    ResourceId = [int]$resourceId
                    ResourceType = [int]$resourceType
                }
            }
        }

        if ($matches.Count -eq 0) {
            throw "Resource '$ResourceName' was not found in $ContainerPath"
        }
        if ($matches.Count -gt 1) {
            throw "Resource '$ResourceName' matched more than once in $ContainerPath"
        }

        $entry = $matches[0]
        $fs.Seek($resourceListOffset + ($entry.ResourceId * 8), [System.IO.SeekOrigin]::Begin) | Out-Null
        $resourceOffset = $br.ReadUInt32()
        $resourceSize = $br.ReadUInt32()
        $fs.Seek($resourceOffset, [System.IO.SeekOrigin]::Begin) | Out-Null
        return $br.ReadBytes($resourceSize)
    } finally {
        $fs.Dispose()
    }
}

function Repair-ErfDropResourceTypes {
    param(
        [string]$Source,
        [string]$Destination,
        [int[]]$DropResourceTypes
    )

    $drop = @{}
    foreach ($resourceType in $DropResourceTypes) {
        $drop[[int]$resourceType] = $true
    }

    $fs = [System.IO.File]::OpenRead($Source)
    try {
        $br = New-Object System.IO.BinaryReader($fs)
        $fileTypeBytes = $br.ReadBytes(4)
        $versionBytes = $br.ReadBytes(4)
        $fileType = [System.Text.Encoding]::ASCII.GetString($fileTypeBytes)
        $version = [System.Text.Encoding]::ASCII.GetString($versionBytes)
        if ($fileType -notin @("HAK ", "ERF ", "MOD ") -or $version -ne "V1.0") {
            throw "Unsupported ERF container '$Source' type='$fileType' version='$version'"
        }

        $languageCount = $br.ReadUInt32()
        $localizedStringSize = $br.ReadUInt32()
        $entryCount = $br.ReadUInt32()
        $localizedStringOffset = $br.ReadUInt32()
        $keyListOffset = $br.ReadUInt32()
        $resourceListOffset = $br.ReadUInt32()
        $buildYear = $br.ReadUInt32()
        $buildDay = $br.ReadUInt32()
        $descriptionStrRef = $br.ReadUInt32()
        $reserved = $br.ReadBytes(116)

        $localizedBytes = @()
        if ($localizedStringSize -gt 0) {
            $fs.Seek($localizedStringOffset, [System.IO.SeekOrigin]::Begin) | Out-Null
            $localizedBytes = $br.ReadBytes($localizedStringSize)
        }

        $keys = @()
        $fs.Seek($keyListOffset, [System.IO.SeekOrigin]::Begin) | Out-Null
        for ($index = 0; $index -lt $entryCount; $index++) {
            $keys += [pscustomobject]@{
                ResrefBytes = $br.ReadBytes(16)
                ResourceId = [int]$br.ReadUInt32()
                ResourceType = [int]$br.ReadUInt16()
                Unused = [int]$br.ReadUInt16()
            }
        }

        $resources = @()
        $fs.Seek($resourceListOffset, [System.IO.SeekOrigin]::Begin) | Out-Null
        for ($index = 0; $index -lt $entryCount; $index++) {
            $resources += [pscustomobject]@{
                Offset = [uint32]$br.ReadUInt32()
                Size = [uint32]$br.ReadUInt32()
            }
        }

        $included = @()
        $dropped = @()
        foreach ($key in $keys) {
            $resref = ([System.Text.Encoding]::ASCII.GetString($key.ResrefBytes)).TrimEnd([char]0)
            if ($drop.ContainsKey($key.ResourceType)) {
                $dropped += [pscustomobject]@{
                    resref = $resref
                    resourceType = $key.ResourceType
                }
                continue
            }

            $resource = $resources[$key.ResourceId]
            $fs.Seek($resource.Offset, [System.IO.SeekOrigin]::Begin) | Out-Null
            $included += [pscustomobject]@{
                ResrefBytes = $key.ResrefBytes
                ResourceType = $key.ResourceType
                Unused = $key.Unused
                Data = $br.ReadBytes($resource.Size)
            }
        }
    } finally {
        $fs.Dispose()
    }

    Ensure-Directory (Split-Path -Parent $Destination)
    $out = [System.IO.File]::Create($Destination)
    try {
        $bw = New-Object System.IO.BinaryWriter($out)
        $headerSize = 160
        $newEntryCount = [uint32]$included.Count
        $newLocalizedStringOffset = [uint32]$headerSize
        $newKeyListOffset = [uint32]($headerSize + $localizedBytes.Length)
        $newResourceListOffset = [uint32]($newKeyListOffset + ($included.Count * 24))
        $dataOffset = [uint32]($newResourceListOffset + ($included.Count * 8))

        $bw.Write($fileTypeBytes)
        $bw.Write($versionBytes)
        $bw.Write([uint32]$languageCount)
        $bw.Write([uint32]$localizedBytes.Length)
        $bw.Write($newEntryCount)
        $bw.Write($newLocalizedStringOffset)
        $bw.Write($newKeyListOffset)
        $bw.Write($newResourceListOffset)
        $bw.Write([uint32]$buildYear)
        $bw.Write([uint32]$buildDay)
        $bw.Write([uint32]$descriptionStrRef)
        $bw.Write($reserved)
        if ($localizedBytes.Length -gt 0) {
            $bw.Write([byte[]]$localizedBytes)
        }

        for ($index = 0; $index -lt $included.Count; $index++) {
            $entry = $included[$index]
            $bw.Write([byte[]]$entry.ResrefBytes)
            $bw.Write([uint32]$index)
            $bw.Write([uint16]$entry.ResourceType)
            $bw.Write([uint16]$entry.Unused)
        }

        $cursor = $dataOffset
        foreach ($entry in $included) {
            $bw.Write([uint32]$cursor)
            $bw.Write([uint32]$entry.Data.Length)
            $cursor = [uint32]($cursor + $entry.Data.Length)
        }

        foreach ($entry in $included) {
            $bw.Write([byte[]]$entry.Data)
        }
    } finally {
        $out.Dispose()
    }

    return [pscustomobject]@{
        source = $Source
        destination = $Destination
        kept = $included.Count
        dropped = $dropped
        sha256 = Get-FileSha256 $Destination
    }
}

function Apply-NwsyncSanitizers {
    param(
        [object]$Profile,
        [string]$StageRoot,
        [string]$FixRoot,
        [switch]$Force
    )

    $outputs = @()
    if (-not $Profile.nwsyncSanitizers) {
        return $outputs
    }

    foreach ($sanitizer in $Profile.nwsyncSanitizers) {
        if ($sanitizer.kind -ne "dropErfResourceTypes") {
            throw "Unsupported NWSync sanitizer kind '$($sanitizer.kind)' in profile '$($Profile.id)'"
        }
        $stagedHak = Join-Path $StageRoot "hak\$($sanitizer.hak)"
        if (-not (Test-Path -LiteralPath $stagedHak)) {
            throw "Cannot sanitize missing staged HAK: $stagedHak"
        }
        $safeHak = Join-Path $FixRoot "hak\$($sanitizer.hak)"
        $result = Repair-ErfDropResourceTypes -Source $stagedHak -Destination $safeHak -DropResourceTypes ([int[]]$sanitizer.dropResourceTypes)
        Link-Or-Copy -Source $safeHak -Destination $stagedHak -Force:$Force
        $outputs += [pscustomobject]@{
            id = $sanitizer.id
            hak = $sanitizer.hak
            reason = $sanitizer.reason
            kept = $result.kept
            dropped = $result.dropped
            stagedPath = $stagedHak
            sha256 = Get-FileSha256 $stagedHak
        }
    }
    return $outputs
}

function Patch-TileBlockLine {
    param(
        [string]$Text,
        [string]$Tile,
        [string]$InsertAfterPrefix,
        [string]$Line
    )

    $lines = $Text -split "`r?`n"
    $tileHeader = "[$Tile]"
    $start = -1
    for ($i = 0; $i -lt $lines.Count; $i++) {
        if ($lines[$i].Trim() -ieq $tileHeader) {
            $start = $i
            break
        }
    }
    if ($start -lt 0) {
        throw "Tile block $tileHeader was not found."
    }

    $end = $lines.Count
    for ($i = $start + 1; $i -lt $lines.Count; $i++) {
        if ($lines[$i].StartsWith("[") -and $lines[$i].EndsWith("]")) {
            $end = $i
            break
        }
    }

    for ($i = $start + 1; $i -lt $end; $i++) {
        if ($lines[$i].Trim().StartsWith(($Line.Split("=")[0] + "="), [System.StringComparison]::OrdinalIgnoreCase)) {
            return [pscustomobject]@{ Text = $Text; Changed = $false }
        }
    }

    $insertAt = $start + 1
    for ($i = $start + 1; $i -lt $end; $i++) {
        if ($lines[$i].StartsWith($InsertAfterPrefix, [System.StringComparison]::OrdinalIgnoreCase)) {
            $insertAt = $i + 1
            break
        }
    }

    $newLines = New-Object System.Collections.Generic.List[string]
    for ($i = 0; $i -lt $lines.Count; $i++) {
        if ($i -eq $insertAt) {
            $newLines.Add($Line)
        }
        $newLines.Add($lines[$i])
    }
    if ($insertAt -ge $lines.Count) {
        $newLines.Add($Line)
    }

    return [pscustomobject]@{
        Text = ($newLines -join "`r`n")
        Changed = $true
    }
}

function Get-2daClassPackageReferences {
    param(
        [string]$Text,
        [string[]]$Columns
    )

    $lines = $Text -split "\r?\n"
    $headerIndex = -1
    for ($i = 0; $i -lt $lines.Count; $i++) {
        $trimmed = $lines[$i].Trim()
        if ([string]::IsNullOrWhiteSpace($trimmed) -or $trimmed -ieq "2DA V2.0") {
            continue
        }
        $headerIndex = $i
        break
    }
    if ($headerIndex -lt 0) {
        throw "classes.2da has no header row"
    }

    $headers = @($lines[$headerIndex].Trim() -split "\s+")
    $columnIndexes = @{}
    foreach ($column in $Columns) {
        for ($i = 0; $i -lt $headers.Count; $i++) {
            if ($headers[$i] -ieq $column) {
                $columnIndexes[$column] = $i
                break
            }
        }
        if (-not $columnIndexes.ContainsKey($column)) {
            throw "classes.2da does not contain requested package column '$column'"
        }
    }

    $references = New-Object 'System.Collections.Generic.HashSet[string]' ([System.StringComparer]::OrdinalIgnoreCase)
    for ($lineIndex = $headerIndex + 1; $lineIndex -lt $lines.Count; $lineIndex++) {
        $line = $lines[$lineIndex].Trim()
        if ([string]::IsNullOrWhiteSpace($line) -or $line.StartsWith("//")) {
            continue
        }
        $cells = @($line -split "\s+")
        if ($cells.Count -lt 2 -or $cells[0] -notmatch '^\d+$') {
            continue
        }

        foreach ($column in $Columns) {
            # Data rows include the numeric 2DA row id before the first header
            # column, so header index N maps to cell N+1.
            $cellIndex = [int]$columnIndexes[$column] + 1
            if ($cellIndex -ge $cells.Count) {
                continue
            }
            $value = $cells[$cellIndex]
            if ([string]::IsNullOrWhiteSpace($value) -or $value -eq "****") {
                continue
            }
            if ($value -like "CLS_*") {
                [void]$references.Add($value.ToUpperInvariant())
            }
        }
    }
    return $references
}

function New-EmptyClassPackage2daText {
    param([string]$ResourceName)

    $upper = ([System.IO.Path]::GetFileNameWithoutExtension($ResourceName)).ToUpperInvariant()
    $columns = switch -Regex ($upper) {
        '^CLS_SKILL_' { "SkillIndex"; break }
        '^CLS_FEAT_' { "FeatIndex"; break }
        '^CLS_BFEAT_' { "FeatIndex"; break }
        '^CLS_PRES_' { "ReqType ReqParam1 ReqParam2"; break }
        default { "Value"; break }
    }

    return "2DA V2.0`r`n`r`n        $columns`r`n"
}

function Apply-CompatibilityFixes {
    param(
        [object]$Profile,
        [string]$StageRoot,
        [string]$FixRoot,
        [string[]]$LookupRoots,
        [switch]$Force
    )

    $outputs = @()
    if (-not $Profile.compatibilityFixes) {
        return $outputs
    }

    foreach ($fix in $Profile.compatibilityFixes) {
        if ($fix.kind -eq "generatedMissingClassPackage2das") {
            $sourceHak = Find-StagedFile -FileName $fix.sourceHak -Roots $LookupRoots
            if (-not $sourceHak) {
                throw "Could not find source HAK '$($fix.sourceHak)' for compatibility fix '$($fix.id)'"
            }

            $sourceResource = if ($fix.sourceResource) { [string]$fix.sourceResource } else { "classes.2da" }
            $bytes = Read-ErfResource -ContainerPath $sourceHak -ResourceName $sourceResource
            $encoding = [System.Text.Encoding]::GetEncoding(1252)
            $references = Get-2daClassPackageReferences -Text ($encoding.GetString($bytes)) -Columns ([string[]]$fix.resourceColumns)

            $generated = @()
            foreach ($resourceName in ([string[]]$fix.resourceNames)) {
                $upper = ([System.IO.Path]::GetFileNameWithoutExtension($resourceName)).ToUpperInvariant()
                if (-not $references.Contains($upper)) {
                    throw "Compatibility fix '$($fix.id)' asked to generate '$upper', but '$sourceResource' in '$sourceHak' does not reference it."
                }

                $fileName = "$($upper.ToLowerInvariant()).2da"
                $text = New-EmptyClassPackage2daText -ResourceName $fileName
                $sourceOut = Join-Path $FixRoot "sources\generated-class-package-2das\$fileName"
                $overrideOut = Join-Path $StageRoot "override\$fileName"
                Ensure-Directory (Split-Path -Parent $sourceOut)
                Ensure-Directory (Split-Path -Parent $overrideOut)
                if ($Force -or -not (Test-Path -LiteralPath $sourceOut)) {
                    [System.IO.File]::WriteAllBytes($sourceOut, ([System.Text.Encoding]::ASCII.GetBytes($text)))
                }
                Link-Or-Copy -Source $sourceOut -Destination $overrideOut -Force:$Force
                $generated += [pscustomobject]@{
                    resource = $fileName
                    stagedPath = $overrideOut
                    sha256 = Get-FileSha256 $overrideOut
                }
            }

            $outputs += [pscustomobject]@{
                id = $fix.id
                sourceHak = $sourceHak
                sourceResource = $sourceResource
                generated = $generated
                evidence = $fix.evidence
            }
            continue
        }

        if ($fix.kind -ne "erfTextResourcePatch") {
            throw "Unsupported compatibility fix kind '$($fix.kind)' in profile '$($Profile.id)'"
        }

        $sourceHak = Find-StagedFile -FileName $fix.sourceHak -Roots $LookupRoots
        if (-not $sourceHak) {
            throw "Could not find source HAK '$($fix.sourceHak)' for compatibility fix '$($fix.id)'"
        }

        $bytes = Read-ErfResource -ContainerPath $sourceHak -ResourceName $fix.resource
        $encoding = [System.Text.Encoding]::GetEncoding(1252)
        $text = $encoding.GetString($bytes)
        $patched = Patch-TileBlockLine -Text $text -Tile $fix.tile -InsertAfterPrefix $fix.insertAfterPrefix -Line $fix.line

        $sourceOut = Join-Path $FixRoot "sources\$($fix.resource)"
        $overrideOut = Join-Path $StageRoot "override\$($fix.resource)"
        Ensure-Directory (Split-Path -Parent $sourceOut)
        Ensure-Directory (Split-Path -Parent $overrideOut)
        if ($patched.Changed -or $Force -or -not (Test-Path -LiteralPath $sourceOut)) {
            [System.IO.File]::WriteAllBytes($sourceOut, $encoding.GetBytes($patched.Text))
        }
        Link-Or-Copy -Source $sourceOut -Destination $overrideOut -Force:$Force

        $outputs += [pscustomobject]@{
            id = $fix.id
            sourceHak = $sourceHak
            resource = $fix.resource
            stagedPath = $overrideOut
            changed = [bool]$patched.Changed
            evidence = $fix.evidence
            sha256 = Get-FileSha256 $overrideOut
        }
    }
    return $outputs
}

function Write-JsonFile {
    param([string]$PathValue, [object]$Value)
    Ensure-Directory (Split-Path -Parent $PathValue)
    $Value | ConvertTo-Json -Depth 16 | Set-Content -LiteralPath $PathValue -Encoding UTF8
}

function Get-NwsyncMaxManifestBytes {
    param([object]$Profile)

    # EE parses NWSync manifest JSON total_bytes through a 32-bit path before it
    # downloads data blobs. Keep each advertised manifest comfortably below that
    # reader limit. Profiles may lower this to force smaller shards, but they
    # must not raise it beyond the EE parser's range.
    $defaultMax = [uint64]3900000000
    $max = $defaultMax
    if ($Profile.nwsync -and $null -ne $Profile.nwsync.maxManifestTotalBytes) {
        $max = [uint64]$Profile.nwsync.maxManifestTotalBytes
    }
    if ($max -gt [uint64]4294967295) {
        throw "Profile '$($Profile.id)' sets nwsync.maxManifestTotalBytes above EE's 32-bit manifest metadata limit."
    }
    if ($max -lt [uint64](64 * 1024 * 1024)) {
        throw "Profile '$($Profile.id)' sets nwsync.maxManifestTotalBytes too low for practical manifest sharding."
    }
    return $max
}

function Get-NwsyncSpecEstimatedBytes {
    param([string]$PathValue)

    $item = Get-Item -LiteralPath $PathValue -ErrorAction Stop
    if ($item.PSIsContainer) {
        $sum = (Get-ChildItem -LiteralPath $item.FullName -Recurse -File | Measure-Object Length -Sum).Sum
        if ($null -eq $sum) {
            return [uint64]0
        }
        return [uint64]$sum
    }
    return [uint64]$item.Length
}

function New-NwsyncSpec {
    param(
        [string]$PathValue,
        [string]$Kind,
        [string]$Name
    )

    $item = Get-Item -LiteralPath $PathValue -ErrorAction Stop
    return [pscustomobject]@{
        path = $item.FullName
        kind = $Kind
        name = $Name
        estimatedBytes = Get-NwsyncSpecEstimatedBytes -PathValue $item.FullName
    }
}

function New-NwsyncShard {
    param(
        [int]$Index,
        [object[]]$Specs,
        [uint64]$EstimatedBytes
    )

    return [pscustomobject]@{
        index = $Index
        estimatedBytes = $EstimatedBytes
        specs = @($Specs)
    }
}

function Split-NwsyncSpecsIntoShards {
    param(
        [object[]]$Specs,
        [uint64]$MaxManifestBytes
    )

    $shards = New-Object System.Collections.Generic.List[object]
    $current = New-Object System.Collections.Generic.List[object]
    $currentBytes = [uint64]0
    $index = 1

    foreach ($spec in $Specs) {
        $specBytes = [uint64]$spec.estimatedBytes
        if ($specBytes -gt $MaxManifestBytes) {
            throw "NWSync spec '$($spec.path)' is estimated at $specBytes bytes, which exceeds the per-manifest limit $MaxManifestBytes."
        }
        if ($current.Count -gt 0 -and ($currentBytes + $specBytes) -gt $MaxManifestBytes) {
            $shards.Add((New-NwsyncShard -Index $index -Specs $current.ToArray() -EstimatedBytes $currentBytes))
            $current = New-Object System.Collections.Generic.List[object]
            $currentBytes = [uint64]0
            $index++
        }
        $current.Add($spec)
        $currentBytes += $specBytes
    }

    if ($current.Count -gt 0) {
        $shards.Add((New-NwsyncShard -Index $index -Specs $current.ToArray() -EstimatedBytes $currentBytes))
    }

    return @($shards.ToArray())
}

function Get-NwsyncModuleResourceManifestAdverts {
    param(
        [object]$Profile,
        [object[]]$WrittenManifests
    )

    if ($Profile.nwsync -and $null -ne $Profile.nwsync.moduleResourceManifests) {
        return @($Profile.nwsync.moduleResourceManifests | ForEach-Object {
            $hash = [string]$_.hash
            if ([string]::IsNullOrWhiteSpace($hash)) {
                throw "Profile '$($Profile.id)' contains an empty nwsync.moduleResourceManifests hash."
            }
            [pscustomobject]@{
                hash = $hash.Trim()
                flags = $(if ($null -ne $_.flags) { [int]$_.flags } else { 1 })
                language = $(if ($null -ne $_.language) { [int]$_.language } else { 255 })
                source = "profile-explicit"
            }
        })
    }

    $policy = "none"
    if ($Profile.nwsync -and $null -ne $Profile.nwsync.moduleResourceManifestPolicy) {
        $policy = ([string]$Profile.nwsync.moduleResourceManifestPolicy).Trim().ToLowerInvariant()
    }

    switch ($policy) {
        "" { return @() }
        "none" { return @() }
        "generated-non-root" {
            return @($WrittenManifests | Where-Object { [int]$_.index -ne 1 } | ForEach-Object {
                [pscustomobject]@{
                    hash = [string]$_.hash
                    flags = [int]$_.flags
                    language = [int]$_.language
                    source = "generated-non-root"
                }
            })
        }
        "generatednonroot" {
            return @($WrittenManifests | Where-Object { [int]$_.index -ne 1 } | ForEach-Object {
                [pscustomobject]@{
                    hash = [string]$_.hash
                    flags = [int]$_.flags
                    language = [int]$_.language
                    source = "generated-non-root"
                }
            })
        }
        default {
            throw "Profile '$($Profile.id)' sets unsupported nwsync.moduleResourceManifestPolicy '$policy'. Supported values: none, generated-non-root."
        }
    }
}

function Format-NwsyncManifestAdvertText {
    param([object[]]$Manifests)

    return ($Manifests | ForEach-Object {
        $hash = [string]$_.hash
        if ([string]::IsNullOrWhiteSpace($hash)) {
            throw "NWSync manifest advert contains an empty hash."
        }
        $flags = $(if ($null -ne $_.flags) { [int]$_.flags } else { 1 })
        $language = $(if ($null -ne $_.language) { [int]$_.language } else { 255 })
        "$($hash.Trim()):$flags:0x$('{0:X2}' -f $language)"
    }) -join ","
}

function New-NwsyncWriteArgs {
    param(
        [object]$Profile,
        [string]$Name,
        [string]$Description,
        [string]$RepositoryRoot,
        [string[]]$SpecPaths
    )

    $args = @(
        "--name", $Name,
        "--description", $Description,
        "--group-id", [string]$Profile.nwsync.groupId,
        "--limit-file-size", [string]$Profile.nwsync.limitFileSizeMB,
        "--compression", [string]$Profile.nwsync.compression
    )
    if ($Profile.nwsync.writeOrigins) {
        $args += "--write-origins"
    }
    $args += $RepositoryRoot
    $args += $SpecPaths
    return $args
}

function Read-NwsyncManifestMetadata {
    param(
        [string]$RepositoryRoot,
        [string]$Hash
    )

    $jsonPath = Join-Path $RepositoryRoot "manifests\$Hash.json"
    if (-not (Test-Path -LiteralPath $jsonPath)) {
        return $null
    }
    return Get-Content -LiteralPath $jsonPath -Raw | ConvertFrom-Json
}

$profilePathResolved = Resolve-ProjectPath $ProfilePath
$profile = Read-JsonProfile $profilePathResolved
if (-not $RequiredRoot) {
    $RequiredRoot = $profile.requiredSourcePath
}
if (-not $AssetRoot) {
    if ($profile.localBuildRoot) {
        $AssetRoot = $profile.localBuildRoot
    } else {
        $AssetRoot = ".\assets"
    }
}
$assetRootResolved = Resolve-ProjectPath $AssetRoot
$requiredRootResolved = Resolve-ProjectPath $RequiredRoot
$stageRoot = Join-Path $assetRootResolved "staged\$($profile.stagingSubdir)"
$extractRoot = Join-Path $assetRootResolved "sources\$($profile.stagingSubdir)\_extracted"
$fixRoot = Join-Path $assetRootResolved "fixes\$($profile.stagingSubdir)"
$buildRoot = Join-Path $assetRootResolved "builds\$($profile.stagingSubdir)"
if ($RepositoryRoot) {
    $repoRoot = Resolve-ProjectPath $RepositoryRoot
} else {
    $repoRoot = Resolve-ProjectPath $profile.nwsync.repositoryPath
}

Ensure-Directory $stageRoot
Ensure-Directory $extractRoot
Ensure-Directory $fixRoot
Ensure-Directory $buildRoot

if ($Apply -and $Force) {
    $profileHakNames = @($profile.hakOrderTopFirst | ForEach-Object { "$_.hak" })
    $profileTlkNames = @($profile.tlkFiles | ForEach-Object { "$_" })
    Remove-StaleProfileFiles -StageRoot $stageRoot -Subdir "hak" -Extension ".hak" -KeepNames $profileHakNames
    Remove-StaleProfileFiles -StageRoot $stageRoot -Subdir "tlk" -Extension ".tlk" -KeepNames $profileTlkNames
}

$sourceManifest = @()
foreach ($archiveSpec in $profile.sourceArchives) {
    $matches = @(Get-ChildItem -LiteralPath $requiredRootResolved -File -Filter $archiveSpec.pattern -ErrorAction SilentlyContinue)
    if ($matches.Count -eq 0) {
        if ($archiveSpec.required -and -not $archiveSpec.allowExistingStaged) {
            throw "Required asset archive '$($archiveSpec.pattern)' was not found in $requiredRootResolved"
        }
        Write-Warning "Asset archive '$($archiveSpec.pattern)' was not found; existing staged assets may be used."
        continue
    }

    foreach ($archive in $matches) {
        $entry = [pscustomobject]@{
            pattern = $archiveSpec.pattern
            path = $archive.FullName
            length = $archive.Length
            sha256 = $null
            extracted = $false
            warning = $null
        }
        if ($archive.Length -le 0) {
            $entry.warning = "Archive is zero bytes and was not extracted."
            if (-not $archiveSpec.allowExistingStaged) {
                throw "Required asset archive '$($archive.FullName)' is zero bytes."
            }
            Write-Warning "$($archive.Name) is zero bytes; using existing staged assets if available."
            $sourceManifest += $entry
            continue
        }

        $entry.sha256 = Get-FileSha256 $archive.FullName
        $archiveExtractRoot = Join-Path $extractRoot ([System.IO.Path]::GetFileNameWithoutExtension($archive.Name))
        if ($Apply) {
            Invoke-ArchiveExtract -Archive $archive.FullName -Destination $archiveExtractRoot
            Copy-ExtractedAssetsToStage -ExtractRoot $archiveExtractRoot -StageRoot $stageRoot -Force:$Force
            $entry.extracted = $true
        }
        $sourceManifest += $entry
    }
}

$legacyRoots = @()
foreach ($legacyRoot in $profile.legacyAssetRoots) {
    $legacyRoots += Resolve-ProjectPath $legacyRoot
}
$lookupRoots = @(
    (Join-Path $stageRoot "hak"),
    (Join-Path $stageRoot "tlk"),
    (Join-Path $stageRoot "override")
)
foreach ($root in $legacyRoots) {
    $lookupRoots += @(
        (Join-Path $root "hg-std\hak"),
        (Join-Path $root "hg-std\tlk"),
        (Join-Path $root "hg-gui\hak"),
        (Join-Path $root "cep23\hak"),
        (Join-Path $root "cep23\tlk"),
        (Join-Path $root "diamond\hak"),
        (Join-Path $root "diamond\tlk"),
        (Join-Path $root "hg-override"),
        (Join-Path $root "hg-overlay")
    )
}

foreach ($hak in $profile.hakOrderTopFirst) {
    $file = Find-StagedFile -FileName "$hak.hak" -Roots $lookupRoots
    if (-not $file) {
        throw "Profile '$($profile.id)' requires HAK '$hak.hak', but it was not found."
    }
    if ($Apply) {
        Link-Or-Copy -Source $file -Destination (Join-Path $stageRoot "hak\$hak.hak") -Force:$Force
    }
}

foreach ($tlk in $profile.tlkFiles) {
    $file = Find-StagedFile -FileName $tlk -Roots $lookupRoots
    if (-not $file) {
        Write-Warning "Profile '$($profile.id)' lists TLK '$tlk', but it was not found."
        continue
    }
    if ($Apply) {
        Link-Or-Copy -Source $file -Destination (Join-Path $stageRoot "tlk\$tlk") -Force:$Force
    }
}

foreach ($overrideRoot in $profile.overrideRoots) {
    foreach ($legacyRoot in $legacyRoots) {
        $root = Join-Path $legacyRoot $overrideRoot
        if (-not (Test-Path -LiteralPath $root)) {
            continue
        }
        Get-ChildItem -LiteralPath $root -Recurse -File | ForEach-Object {
            $relative = Get-RelativePathCompat -BasePath $root -PathValue $_.FullName
            if ($Apply) {
                Link-Or-Copy -Source $_.FullName -Destination (Join-Path $stageRoot "override\$relative") -Force:$Force
            }
        }
    }
}

$compatibility = @()
$sanitizers = @()
if ($Apply) {
    $lookupRoots = @(
        (Join-Path $stageRoot "hak"),
        (Join-Path $stageRoot "tlk"),
        (Join-Path $stageRoot "override")
    ) + $lookupRoots
    $compatibility = Apply-CompatibilityFixes -Profile $profile -StageRoot $stageRoot -FixRoot $fixRoot -LookupRoots $lookupRoots -Force:$Force
    $sanitizers = Apply-NwsyncSanitizers -Profile $profile -StageRoot $stageRoot -FixRoot $fixRoot -Force:$Force
}

$stagedFiles = @()
if (Test-Path -LiteralPath $stageRoot) {
    $stagedFiles = @(Get-ChildItem -LiteralPath $stageRoot -Recurse -File | ForEach-Object {
        [pscustomobject]@{
            path = (Get-RelativePathCompat -BasePath $stageRoot -PathValue $_.FullName).Replace("\", "/")
            length = $_.Length
            sha256 = $(if ($HashStagedFiles) { Get-FileSha256 $_.FullName } else { $null })
        }
    })
}

Write-JsonFile -PathValue (Join-Path $buildRoot "source-manifest.json") -Value ([pscustomobject]@{
    profile = $profile.id
    generatedUtc = [DateTime]::UtcNow.ToString("o")
    requiredRoot = $requiredRootResolved
    sources = $sourceManifest
})

Write-JsonFile -PathValue (Join-Path $buildRoot "staged-manifest.json") -Value ([pscustomobject]@{
    profile = $profile.id
    generatedUtc = [DateTime]::UtcNow.ToString("o")
    stageRoot = $stageRoot
    files = $stagedFiles
})

Write-JsonFile -PathValue (Join-Path $buildRoot "compatibility-manifest.json") -Value ([pscustomobject]@{
    profile = $profile.id
    generatedUtc = [DateTime]::UtcNow.ToString("o")
    fixes = $compatibility
    nwsyncSanitizers = $sanitizers
})

if (-not $SkipNwsync -and $Apply) {
    if (-not (Test-Path -LiteralPath $NwsyncTool)) {
        throw "nwsync_write was not found at '$NwsyncTool'"
    }
    Ensure-Directory $repoRoot

    $specs = New-Object System.Collections.Generic.List[object]
# Profiles declare the module/HAK list in the same top-first order Diamond
# stores in module metadata. Diamond activates that list from the end back
# toward the front, while `nwsync_write --help` documents that later specs
# shadow earlier specs. Feed NWSync the profile-derived reverse order so the
# final/highest-precedence HAK remains the same without hard-coding any HG
# server-specific ordering in the builder.
$hakOrderBottomFirst = @($profile.hakOrderTopFirst)
    [array]::Reverse($hakOrderBottomFirst)
    foreach ($hak in $hakOrderBottomFirst) {
        $path = Join-Path $stageRoot "hak\$hak.hak"
        if (-not (Test-Path -LiteralPath $path)) {
            throw "Staged HAK missing before NWSync build: $path"
        }
        $specs.Add((New-NwsyncSpec -PathValue $path -Kind "hak" -Name "$hak.hak"))
    }
    $tlkRoot = Join-Path $stageRoot "tlk"
    if (Test-Path -LiteralPath $tlkRoot) {
        Get-ChildItem -LiteralPath $tlkRoot -File | Sort-Object Name | ForEach-Object {
            $specs.Add((New-NwsyncSpec -PathValue $_.FullName -Kind "tlk" -Name $_.Name))
        }
    }
    $overrideRoot = Join-Path $stageRoot "override"
    if (Test-Path -LiteralPath $overrideRoot) {
        $specs.Add((New-NwsyncSpec -PathValue $overrideRoot -Kind "override" -Name "override"))
    }

    $maxManifestBytes = Get-NwsyncMaxManifestBytes -Profile $profile
    $shards = Split-NwsyncSpecsIntoShards -Specs $specs.ToArray() -MaxManifestBytes $maxManifestBytes
    if ($shards.Count -eq 0) {
        throw "Profile '$($profile.id)' produced no NWSync specs."
    }

    $latestPath = Join-Path $repoRoot "latest"
    $writtenManifests = @()
    foreach ($shard in $shards) {
        $suffix = $(if ($shards.Count -gt 1) { " shard $($shard.index) of $($shards.Count)" } else { "" })
        $manifestName = "$($profile.nwsync.name)$suffix"
        $manifestDescription = "$($profile.nwsync.description)$suffix"
        $specPaths = @($shard.specs | ForEach-Object { [string]$_.path })
        $args = New-NwsyncWriteArgs -Profile $profile -Name $manifestName -Description $manifestDescription -RepositoryRoot $repoRoot -SpecPaths $specPaths

        Write-Host "Writing NWSync manifest $($shard.index)/$($shards.Count): $($shard.estimatedBytes) estimated bytes, $($specPaths.Count) specs"
        & $NwsyncTool @args
        if ($LASTEXITCODE -ne 0) {
            throw "nwsync_write failed with exit code $LASTEXITCODE for shard $($shard.index)"
        }
        if (-not (Test-Path -LiteralPath $latestPath)) {
            throw "NWSync repository did not produce a latest pointer at $latestPath"
        }

        $hash = (Get-Content -LiteralPath $latestPath -Raw).Trim()
        $metadata = Read-NwsyncManifestMetadata -RepositoryRoot $repoRoot -Hash $hash
        if ($metadata -and $null -ne $metadata.total_bytes -and [uint64]$metadata.total_bytes -gt [uint64]4294967295) {
            throw "NWSync manifest $hash reports total_bytes=$($metadata.total_bytes), which exceeds EE's 32-bit manifest metadata reader."
        }

        $writtenManifests += [pscustomobject]@{
            index = $shard.index
            hash = $hash
            role = $(if ($shard.index -eq 1) { "root" } else { "advertised" })
            flags = 1
            language = 255
            estimatedBytes = $shard.estimatedBytes
            totalBytes = $(if ($metadata -and $null -ne $metadata.total_bytes) { [uint64]$metadata.total_bytes } else { $null })
            totalFiles = $(if ($metadata -and $null -ne $metadata.total_files) { [uint64]$metadata.total_files } else { $null })
            specs = @($shard.specs)
        }
    }

    $rootHash = [string]$writtenManifests[0].hash
    $moduleResourceManifestAdverts = Get-NwsyncModuleResourceManifestAdverts -Profile $profile -WrittenManifests $writtenManifests
    $nwsyncManifest = [pscustomobject]@{
        profile = $profile.id
        generatedUtc = [DateTime]::UtcNow.ToString("o")
        repositoryRoot = $repoRoot
        rootHash = $rootHash
        url = $profile.nwsync.url
        maxManifestTotalBytes = $maxManifestBytes
        loadOrder = "root manifest first, then advertised manifests in listed order"
        specs = @($specs.ToArray())
        manifests = $writtenManifests
        moduleResourceManifests = @($moduleResourceManifestAdverts)
    }
    Write-JsonFile -PathValue (Join-Path $buildRoot "nwsync-manifest.json") -Value $nwsyncManifest

    $advertisedManifestText = Format-NwsyncManifestAdvertText -Manifests $writtenManifests
    # Keep native BNXR download adverts separate from module-resource mount
    # adverts. The EE decompile for `CNWCModule::LoadModuleResources` shows it
    # mounts the root manifest first, then calls `CExoResMan::AddManifest` for
    # every explicit manifest entry in the module-resource packet. Those extras
    # are therefore resource-manager key tables, not just download hints.
    # Profiles can opt into generated non-root shard mounts only when their
    # asset delivery path also gives EE a BNXR preflight chance to cache those
    # manifests before module load.
    $moduleManifestText = Format-NwsyncManifestAdvertText -Manifests $moduleResourceManifestAdverts

    $envText = @(
        "HG_BRIDGE_ASSET_PROFILE=$($profile.id)",
        "HG_BRIDGE_NWSYNC_ROOT=$repoRoot",
        "HG_BRIDGE_NWSYNC_HASH=$rootHash",
        "HG_BRIDGE_NWSYNC_URL=$($profile.nwsync.url)",
        "HG_BRIDGE_NWSYNC_MANIFESTS=$advertisedManifestText",
        "HG_BRIDGE_NWSYNC_MODULE_MANIFESTS=$moduleManifestText"
    ) -join "`r`n"
    $envText += "`r`n"
    Set-Content -LiteralPath (Join-Path $buildRoot "nwsync.env") -Value $envText -Encoding ASCII
    Set-Content -LiteralPath ".\hg-bridge-nwsync.env" -Value $envText -Encoding ASCII
}

Write-Host "Asset profile '$($profile.id)' staged at $stageRoot"
if (-not $Apply) {
    Write-Host "Dry run complete. Re-run with -Apply to extract, stage, patch, and build NWSync."
}
