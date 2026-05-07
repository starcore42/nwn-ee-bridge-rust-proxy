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

    $specs = New-Object System.Collections.Generic.List[string]
    $hakOrderBottomFirst = @($profile.hakOrderTopFirst)
    [array]::Reverse($hakOrderBottomFirst)
    foreach ($hak in $hakOrderBottomFirst) {
        $path = Join-Path $stageRoot "hak\$hak.hak"
        if (-not (Test-Path -LiteralPath $path)) {
            throw "Staged HAK missing before NWSync build: $path"
        }
        $specs.Add($path)
    }
    $tlkRoot = Join-Path $stageRoot "tlk"
    if (Test-Path -LiteralPath $tlkRoot) {
        Get-ChildItem -LiteralPath $tlkRoot -File | Sort-Object Name | ForEach-Object { $specs.Add($_.FullName) }
    }
    $overrideRoot = Join-Path $stageRoot "override"
    if (Test-Path -LiteralPath $overrideRoot) {
        $specs.Add($overrideRoot)
    }

    $args = @(
        "--name", $profile.nwsync.name,
        "--description", $profile.nwsync.description,
        "--group-id", [string]$profile.nwsync.groupId,
        "--limit-file-size", [string]$profile.nwsync.limitFileSizeMB,
        "--compression", [string]$profile.nwsync.compression
    )
    if ($profile.nwsync.writeOrigins) {
        $args += "--write-origins"
    }
    $args += $repoRoot
    $args += $specs.ToArray()

    & $NwsyncTool @args
    if ($LASTEXITCODE -ne 0) {
        throw "nwsync_write failed with exit code $LASTEXITCODE"
    }

    $latestPath = Join-Path $repoRoot "latest"
    if (-not (Test-Path -LiteralPath $latestPath)) {
        throw "NWSync repository did not produce a latest pointer at $latestPath"
    }
    $rootHash = (Get-Content -LiteralPath $latestPath -Raw).Trim()
    $nwsyncManifest = [pscustomobject]@{
        profile = $profile.id
        generatedUtc = [DateTime]::UtcNow.ToString("o")
        repositoryRoot = $repoRoot
        rootHash = $rootHash
        url = $profile.nwsync.url
        specs = $specs
    }
    Write-JsonFile -PathValue (Join-Path $buildRoot "nwsync-manifest.json") -Value $nwsyncManifest

    $envText = @(
        "HG_BRIDGE_ASSET_PROFILE=$($profile.id)",
        "HG_BRIDGE_NWSYNC_ROOT=$repoRoot",
        "HG_BRIDGE_NWSYNC_HASH=$rootHash",
        "HG_BRIDGE_NWSYNC_URL=$($profile.nwsync.url)"
    ) -join "`r`n"
    $envText += "`r`n"
    Set-Content -LiteralPath (Join-Path $buildRoot "nwsync.env") -Value $envText -Encoding ASCII
    Set-Content -LiteralPath ".\hg-bridge-nwsync.env" -Value $envText -Encoding ASCII
}

Write-Host "Asset profile '$($profile.id)' staged at $stageRoot"
if (-not $Apply) {
    Write-Host "Dry run complete. Re-run with -Apply to extract, stage, patch, and build NWSync."
}
