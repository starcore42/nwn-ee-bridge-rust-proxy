param(
    [Parameter(Mandatory = $true)]
    [string]$PacketDir,
    [string]$RunRoot = '',
    [string]$ProxyExe = '',
    [int]$ListenPort = 55121,
    [int]$ServerPort = 55133,
    [int]$PacketDelayMilliseconds = 25,
    [int]$TimeoutSeconds = 0,
    [int]$FinalDrainRounds = 30,
    [int]$ProxyOutputWaitMilliseconds = 15000,
    [int]$DrainReceiveTimeoutMilliseconds = 100,
    [switch]$SkipBuild,
    [switch]$NoStrictTranslate,
    [switch]$EnableNwsync,
    [switch]$NoGeneratedClientAcks,
    [switch]$NoSeedEeBnxi,
    [ValidateSet('Auto', 'Client', 'Server')]
    [string]$CapturePerspective = 'Auto',
    [int]$SeedEeBnxiUdpPort = 5120,
    [int]$SeedEeBnxiMajor = 8193,
    [int]$SeedEeBnxiMinor = 37,
    [int]$SeedEeBnxiRevision = 17,
    [string]$SeedEeBnxiBuildHash = '26c6e573',
    [switch]$DebugLiveClaim
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Resolve-RequiredDirectory {
    param(
        [string]$Path,
        [string]$Label
    )

    if (-not (Test-Path -LiteralPath $Path -PathType Container)) {
        throw "$Label not found: $Path"
    }
    return (Resolve-Path -LiteralPath $Path).Path
}

function Resolve-Proxy2Executable {
    param(
        [string]$ExplicitPath,
        [string]$RepositoryRoot,
        [switch]$SkipBuild
    )

    if (-not [string]::IsNullOrWhiteSpace($ExplicitPath)) {
        $resolved = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($ExplicitPath)
        if (-not (Test-Path -LiteralPath $resolved -PathType Leaf)) {
            throw "Proxy executable not found: $resolved"
        }
        return $resolved
    }

    $debugExe = Join-Path $RepositoryRoot 'target\debug\hgbridge_proxy2.exe'
    if ((-not $SkipBuild) -or (-not (Test-Path -LiteralPath $debugExe -PathType Leaf))) {
        if ($SkipBuild) {
            throw "Proxy executable not found and -SkipBuild was set: $debugExe"
        }
        & cargo build -q -p hgbridge-proxy2
        if ($LASTEXITCODE -ne 0) {
            throw "cargo build -p hgbridge-proxy2 failed with exit code $LASTEXITCODE"
        }
    }

    if (-not (Test-Path -LiteralPath $debugExe -PathType Leaf)) {
        throw "Proxy build completed but executable was not found: $debugExe"
    }
    return (Resolve-Path -LiteralPath $debugExe).Path
}

function Write-BeU16 {
    param(
        [byte[]]$Bytes,
        [int]$Offset,
        [int]$Value
    )

    $Bytes[$Offset] = [byte](($Value -shr 8) -band 0xff)
    $Bytes[$Offset + 1] = [byte]($Value -band 0xff)
}

function Read-BeU16 {
    param(
        [byte[]]$Bytes,
        [int]$Offset
    )

    if ($Bytes.Length -lt $Offset + 2) {
        return 0
    }
    return (($Bytes[$Offset] -shl 8) -bor $Bytes[$Offset + 1])
}

function New-LegacyMcrcTable {
    $table = New-Object 'uint32[]' 256
    $poly = [Convert]::ToUInt32('EDB88320', 16)
    for ($i = 0; $i -lt 256; $i++) {
        [uint32]$crc = [uint32]$i
        for ($j = 0; $j -lt 8; $j++) {
            if (($crc -band 1) -ne 0) {
                $crc = (($crc -shr 1) -bxor $poly)
            } else {
                $crc = ($crc -shr 1)
            }
        }
        $table[$i] = $crc
    }
    return $table
}

function Get-LegacyMcrc {
    param(
        [byte[]]$Bytes,
        [uint32[]]$Table
    )

    [uint32]$acc = 0
    for ($i = 3; $i -lt $Bytes.Length; $i++) {
        $index = (($acc -bxor [uint32]$Bytes[$i]) -band 0xff)
        $acc = (($acc -shr 8) -bxor $Table[$index])
    }
    return [int]($acc -band 0xffff)
}

function New-MAckControlFrame {
    param(
        [int]$AckSequence,
        [uint32[]]$CrcTable
    )

    [byte[]]$packet = @(0) * 12
    $packet[0] = [byte][char]'M'
    Write-BeU16 -Bytes $packet -Offset 3 -Value 0
    Write-BeU16 -Bytes $packet -Offset 5 -Value $AckSequence
    $packet[7] = 0x10
    Write-BeU16 -Bytes $packet -Offset 8 -Value 0
    Write-BeU16 -Bytes $packet -Offset 10 -Value 0
    $crc = Get-LegacyMcrc -Bytes $packet -Table $CrcTable
    Write-BeU16 -Bytes $packet -Offset 1 -Value $crc
    return $packet
}

function Add-AsciiBytes {
    param(
        [System.Collections.Generic.List[byte]]$Bytes,
        [string]$Text
    )

    foreach ($b in [System.Text.Encoding]::ASCII.GetBytes($Text)) {
        [void]$Bytes.Add([byte]$b)
    }
}

function Add-CountedAscii {
    param(
        [System.Collections.Generic.List[byte]]$Bytes,
        [string]$Text,
        [string]$Label
    )

    [byte[]]$encoded = [System.Text.Encoding]::ASCII.GetBytes($Text)
    if ($encoded.Length -gt 255) {
        throw "$Label exceeds one-byte BNXI length: $($encoded.Length)"
    }
    [void]$Bytes.Add([byte]$encoded.Length)
    foreach ($b in $encoded) {
        [void]$Bytes.Add([byte]$b)
    }
}

function New-SeedEeBnxiPacket {
    param(
        [int]$UdpPort,
        [int]$Major,
        [int]$Minor,
        [int]$Revision,
        [string]$BuildHash
    )

    $majorText = [string]$Major
    $minorText = [string]$Minor
    $revisionText = [string]$Revision
    $bytes = [System.Collections.Generic.List[byte]]::new()
    Add-AsciiBytes -Bytes $bytes -Text 'BNXI'
    [void]$bytes.Add([byte]($UdpPort -band 0xff))
    [void]$bytes.Add([byte](($UdpPort -shr 8) -band 0xff))
    # EE RequestExtendedServerInfo writes three empty counted strings before
    # the build header on the direct-connect/server-list path used by this
    # replay seed. The third header byte mirrors the observed minor length;
    # the fourth byte is the build-number length consumed by proxy2's parser.
    [void]$bytes.Add([byte]0)
    [void]$bytes.Add([byte]0)
    [void]$bytes.Add([byte]0)
    [void]$bytes.Add([byte]0)
    [void]$bytes.Add([byte]0)
    [void]$bytes.Add([byte]$minorText.Length)
    [void]$bytes.Add([byte]$majorText.Length)
    Add-AsciiBytes -Bytes $bytes -Text $majorText
    Add-CountedAscii -Bytes $bytes -Text $minorText -Label 'BNXI minor build'
    Add-CountedAscii -Bytes $bytes -Text $revisionText -Label 'BNXI revision build'
    Add-CountedAscii -Bytes $bytes -Text $BuildHash -Label 'BNXI build hash'
    return $bytes.ToArray()
}

function Test-PacketTag {
    param(
        [byte[]]$Bytes,
        [string]$Tag
    )

    if ($Bytes.Length -lt 4) {
        return $false
    }

    [byte[]]$tagBytes = [System.Text.Encoding]::ASCII.GetBytes($Tag)
    for ($i = 0; $i -lt 4; $i++) {
        if ($Bytes[$i] -ne $tagBytes[$i]) {
            return $false
        }
    }
    return $true
}

function Resolve-CapturePerspective {
    param(
        [string]$PacketDir,
        [object[]]$Files,
        [string]$RequestedPerspective
    )

    if ($RequestedPerspective -ne 'Auto') {
        return $RequestedPerspective
    }

    $leaf = Split-Path -Leaf $PacketDir
    if ($leaf -ieq 'diamond-client-packets') {
        return 'Client'
    }
    if ($leaf -ieq 'diamond-packets') {
        return 'Server'
    }

    foreach ($file in $Files) {
        if ($file.Name -like '*sendto*') {
            return 'Client'
        }
        if ($file.Name -like '*recvfrom*') {
            return 'Server'
        }
    }

    throw "Could not infer capture perspective for packet dump directory: $PacketDir"
}

function Test-CapturedClientToServer {
    param(
        [string]$FileName,
        [string]$Perspective
    )

    if ($Perspective -eq 'Client') {
        return $FileName -like '*sendto*'
    }
    return $FileName -like '*recvfrom*'
}

function Test-CapturedServerToClient {
    param(
        [string]$FileName,
        [string]$Perspective
    )

    if ($Perspective -eq 'Client') {
        return $FileName -like '*recvfrom*'
    }
    return $FileName -like '*sendto*'
}

function Drain-DummyServer {
    param(
        [System.Net.Sockets.UdpClient]$Server,
        [ref]$ProxyServerEndpoint,
        [ref]$ReceivedCount,
        [object]$DeadlineUtc = $null,
        [int]$TimeoutSeconds = 0,
        [string]$Stage = 'drain dummy server'
    )

    while ($true) {
        Assert-ReplayDeadline -DeadlineUtc $DeadlineUtc -TimeoutSeconds $TimeoutSeconds -Stage $Stage
        try {
            $remote = [System.Net.IPEndPoint]::new([System.Net.IPAddress]::Any, 0)
            [void]$Server.Receive([ref]$remote)
            $ProxyServerEndpoint.Value = $remote
            $ReceivedCount.Value++
        } catch [System.Net.Sockets.SocketException] {
            break
        }
    }
}

function Drain-DummyClient {
    param(
        [System.Net.Sockets.UdpClient]$Client,
        [ref]$ReceivedCount,
        [ref]$GeneratedAckCount,
        [System.Collections.Generic.HashSet[int]]$AckedSequences,
        [uint32[]]$CrcTable,
        [bool]$GenerateClientAcks,
        [object]$DeadlineUtc = $null,
        [int]$TimeoutSeconds = 0,
        [string]$Stage = 'drain dummy client'
    )

    $rounds = 0
    while ($rounds -lt 200) {
        Assert-ReplayDeadline -DeadlineUtc $DeadlineUtc -TimeoutSeconds $TimeoutSeconds -Stage $Stage
        $rounds++
        try {
            $remote = [System.Net.IPEndPoint]::new([System.Net.IPAddress]::Any, 0)
            [byte[]]$packet = $Client.Receive([ref]$remote)
            $ReceivedCount.Value++
            if ($GenerateClientAcks -and $packet.Length -ge 12 -and $packet[0] -eq [byte][char]'M') {
                $sequence = Read-BeU16 -Bytes $packet -Offset 3
                if ($sequence -ne 0 -and $AckedSequences.Add($sequence)) {
                    $ack = New-MAckControlFrame -AckSequence $sequence -CrcTable $CrcTable
                    [void]$Client.Send($ack, $ack.Length)
                    $GeneratedAckCount.Value++
                }
            }
        } catch [System.Net.Sockets.SocketException] {
            break
        }
    }
}

function Get-ServerMCompletionOutputWait {
    param(
        [byte[]]$Bytes,
        [ref]$PendingDeflatedFrames,
        [int]$ProxyOutputWaitMilliseconds
    )

    if ($ProxyOutputWaitMilliseconds -le 0 -or $Bytes.Length -lt 12 -or $Bytes[0] -ne [byte][char]'M') {
        return 0
    }

    $flags = $Bytes[7]
    $packetizedSequence = Read-BeU16 -Bytes $Bytes -Offset 8
    if (($flags -band 0x04) -ne 0) {
        if ($packetizedSequence -gt 1) {
            $PendingDeflatedFrames.Value = $packetizedSequence - 1
            return 0
        }
        $PendingDeflatedFrames.Value = 0
        return $ProxyOutputWaitMilliseconds
    }

    if ($PendingDeflatedFrames.Value -gt 0) {
        $PendingDeflatedFrames.Value--
        if ($PendingDeflatedFrames.Value -eq 0) {
            return $ProxyOutputWaitMilliseconds
        }
    }

    return 0
}

function Wait-DummyClientOutput {
    param(
        [System.Net.Sockets.UdpClient]$Server,
        [System.Net.Sockets.UdpClient]$Client,
        [ref]$ProxyServerEndpoint,
        [ref]$ServerReceivedCount,
        [ref]$ClientReceivedCount,
        [ref]$GeneratedAckCount,
        [System.Collections.Generic.HashSet[int]]$AckedSequences,
        [uint32[]]$CrcTable,
        [bool]$GenerateClientAcks,
        [int]$InitialClientReceivedCount,
        [int]$WaitMilliseconds,
        [object]$DeadlineUtc = $null,
        [int]$TimeoutSeconds = 0,
        [string]$Stage = 'wait for proxy client output'
    )

    if ($WaitMilliseconds -le 0 -or $ClientReceivedCount.Value -gt $InitialClientReceivedCount) {
        return $true
    }

    $waitUntil = [DateTime]::UtcNow.AddMilliseconds($WaitMilliseconds)
    while ([DateTime]::UtcNow -lt $waitUntil) {
        Assert-ReplayDeadline -DeadlineUtc $DeadlineUtc -TimeoutSeconds $TimeoutSeconds -Stage $Stage
        Drain-DummyServer `
            -Server $Server `
            -ProxyServerEndpoint $ProxyServerEndpoint `
            -ReceivedCount $ServerReceivedCount `
            -DeadlineUtc $DeadlineUtc `
            -TimeoutSeconds $TimeoutSeconds `
            -Stage "$Stage dummy server"
        Drain-DummyClient `
            -Client $Client `
            -ReceivedCount $ClientReceivedCount `
            -GeneratedAckCount $GeneratedAckCount `
            -AckedSequences $AckedSequences `
            -CrcTable $CrcTable `
            -GenerateClientAcks $GenerateClientAcks `
            -DeadlineUtc $DeadlineUtc `
            -TimeoutSeconds $TimeoutSeconds `
            -Stage "$Stage dummy client"
        Drain-DummyServer `
            -Server $Server `
            -ProxyServerEndpoint $ProxyServerEndpoint `
            -ReceivedCount $ServerReceivedCount `
            -DeadlineUtc $DeadlineUtc `
            -TimeoutSeconds $TimeoutSeconds `
            -Stage "$Stage dummy server after client"

        if ($ClientReceivedCount.Value -gt $InitialClientReceivedCount) {
            return $true
        }
        Start-Sleep -Milliseconds 50
    }

    return ($ClientReceivedCount.Value -gt $InitialClientReceivedCount)
}

function Get-TextMatchCount {
    param(
        [string]$Text,
        [string]$Pattern
    )

    if ([string]::IsNullOrEmpty($Text)) {
        return 0
    }
    return [regex]::Matches($Text, $Pattern).Count
}

function Get-TraceFieldSum {
    param(
        [string]$Text,
        [string]$Message,
        [string]$Field
    )

    if ([string]::IsNullOrEmpty($Text)) {
        return 0
    }

    [Int64]$sum = 0
    $fieldPattern = '\b' + [regex]::Escape($Field) + '=(\d+)'
    foreach ($line in ($Text -split "`r?`n")) {
        if ($line -notmatch [regex]::Escape($Message)) {
            continue
        }
        $match = [regex]::Match($line, $fieldPattern)
        if ($match.Success) {
            $sum += [Int64]$match.Groups[1].Value
        }
    }

    return $sum
}

function Get-TraceFieldMax {
    param(
        [string]$Text,
        [string]$Message,
        [string]$Field
    )

    if ([string]::IsNullOrEmpty($Text)) {
        return 0
    }

    [Int64]$max = 0
    $fieldPattern = '\b' + [regex]::Escape($Field) + '=(\d+)'
    foreach ($line in ($Text -split "`r?`n")) {
        if ($line -notmatch [regex]::Escape($Message)) {
            continue
        }
        $match = [regex]::Match($line, $fieldPattern)
        if ($match.Success) {
            $value = [Int64]$match.Groups[1].Value
            if ($value -gt $max) {
                $max = $value
            }
        }
    }

    return $max
}

function Get-LiveObjectExactClaimTraceFieldSum {
    param(
        [string]$Text,
        [string]$Field
    )

    return Get-TraceFieldSum `
        -Text $Text `
        -Message 'live-object payload accepted exact EE shape with lifecycle proof' `
        -Field $Field
}

function Get-QuickbarRewriteTraceFieldSum {
    param(
        [string]$Text,
        [string]$Field,
        [bool]$Committed = $true
    )

    $traceText = Get-QuickbarRewriteTraceText -Text $Text -Committed $Committed
    return Get-TraceFieldSum `
        -Text $traceText `
        -Message 'server GuiQuickbar_SetAllButtons rewrite summary' `
        -Field $Field
}

function Get-QuickbarRewriteTraceFieldMax {
    param(
        [string]$Text,
        [string]$Field,
        [bool]$Committed = $true
    )

    $traceText = Get-QuickbarRewriteTraceText -Text $Text -Committed $Committed
    return Get-TraceFieldMax `
        -Text $traceText `
        -Message 'server GuiQuickbar_SetAllButtons rewrite summary' `
        -Field $Field
}

function Get-QuickbarRegistryContextTraceText {
    param(
        [string]$Text,
        [bool]$Committed
    )

    if ([string]::IsNullOrEmpty($Text)) {
        return ''
    }

    $message = [regex]::Escape('server GuiQuickbar_SetAllButtons registry materialization context')
    $commitPattern = if ($Committed) { '\bcommitted=true\b' } else { '\bcommitted=false\b' }
    $lines = foreach ($line in ($Text -split "`r?`n")) {
        if ($line -match $message -and $line -match $commitPattern) {
            $line
        }
    }
    return ($lines -join "`n")
}

function Get-QuickbarRegistryContextTraceCount {
    param(
        [string]$Text,
        [bool]$Committed = $true
    )

    $traceText = Get-QuickbarRegistryContextTraceText -Text $Text -Committed $Committed
    return Get-TextMatchCount -Text $traceText -Pattern 'server GuiQuickbar_SetAllButtons registry materialization context'
}

function Get-QuickbarRegistryContextTraceFieldMax {
    param(
        [string]$Text,
        [string]$Field,
        [bool]$Committed = $true
    )

    $traceText = Get-QuickbarRegistryContextTraceText -Text $Text -Committed $Committed
    return Get-TraceFieldMax `
        -Text $traceText `
        -Message 'server GuiQuickbar_SetAllButtons registry materialization context' `
        -Field $Field
}

function Get-SemanticCommittedQuickbarProfileTraceText {
    param(
        [string]$Text
    )

    if ([string]::IsNullOrEmpty($Text)) {
        return ''
    }

    $message = [regex]::Escape('semantic state observed committed GuiQuickbar slot profile')
    $lines = foreach ($line in ($Text -split "`r?`n")) {
        if ($line -match $message) {
            $line
        }
    }
    return ($lines -join "`n")
}

function Get-SemanticCommittedQuickbarProfileTraceCount {
    param(
        [string]$Text
    )

    $traceText = Get-SemanticCommittedQuickbarProfileTraceText -Text $Text
    return Get-TextMatchCount -Text $traceText -Pattern 'semantic state observed committed GuiQuickbar slot profile'
}

function Get-SemanticCommittedQuickbarProfileTraceFieldMax {
    param(
        [string]$Text,
        [string]$Field
    )

    $traceText = Get-SemanticCommittedQuickbarProfileTraceText -Text $Text
    return Get-TraceFieldMax `
        -Text $traceText `
        -Message 'semantic state observed committed GuiQuickbar slot profile' `
        -Field $Field
}

function Get-SemanticCommittedQuickbarProfileFlagCount {
    param(
        [string]$Text,
        [string]$Field,
        [string]$Value
    )

    $traceText = Get-SemanticCommittedQuickbarProfileTraceText -Text $Text
    if ([string]::IsNullOrEmpty($traceText)) {
        return 0
    }

    $pattern = '\b' + [regex]::Escape($Field) + '=' + [regex]::Escape($Value) + '\b'
    return Get-TextMatchCount -Text $traceText -Pattern $pattern
}

function Get-SemanticCommittedQuickbarProfileStringFieldCount {
    param(
        [string]$Text,
        [string]$Field,
        [string]$Value
    )

    $traceText = Get-SemanticCommittedQuickbarProfileTraceText -Text $Text
    if ([string]::IsNullOrEmpty($traceText)) {
        return 0
    }

    $pattern = '\b' + [regex]::Escape($Field) + '="' + [regex]::Escape($Value) + '"'
    return Get-TextMatchCount -Text $traceText -Pattern $pattern
}

function Get-SemanticPostQuickbarItemContextTraceText {
    param(
        [string]$Text
    )

    if ([string]::IsNullOrEmpty($Text)) {
        return ''
    }

    $message = [regex]::Escape('semantic state retained inventory item context after committed GuiQuickbar')
    $lines = foreach ($line in ($Text -split "`r?`n")) {
        if ($line -match $message) {
            $line
        }
    }
    return ($lines -join "`n")
}

function Get-SemanticPostQuickbarItemContextTraceCount {
    param(
        [string]$Text
    )

    $traceText = Get-SemanticPostQuickbarItemContextTraceText -Text $Text
    return Get-TextMatchCount -Text $traceText -Pattern 'semantic state retained inventory item context after committed GuiQuickbar'
}

function Get-SemanticPostQuickbarItemContextTraceFieldMax {
    param(
        [string]$Text,
        [string]$Field
    )

    $traceText = Get-SemanticPostQuickbarItemContextTraceText -Text $Text
    return Get-TraceFieldMax `
        -Text $traceText `
        -Message 'semantic state retained inventory item context after committed GuiQuickbar' `
        -Field $Field
}

function Get-SemanticPostQuickbarItemContextFlagCount {
    param(
        [string]$Text,
        [string]$Field,
        [string]$Value
    )

    $traceText = Get-SemanticPostQuickbarItemContextTraceText -Text $Text
    if ([string]::IsNullOrEmpty($traceText)) {
        return 0
    }

    $pattern = '\b' + [regex]::Escape($Field) + '=' + [regex]::Escape($Value) + '\b'
    return Get-TextMatchCount -Text $traceText -Pattern $pattern
}

function Get-SemanticPostQuickbarItemContextStringFieldCount {
    param(
        [string]$Text,
        [string]$Field,
        [string]$Value
    )

    $traceText = Get-SemanticPostQuickbarItemContextTraceText -Text $Text
    if ([string]::IsNullOrEmpty($traceText)) {
        return 0
    }

    $pattern = '\b' + [regex]::Escape($Field) + '="' + [regex]::Escape($Value) + '"'
    return Get-TextMatchCount -Text $traceText -Pattern $pattern
}

function Get-SemanticUnresolvedQuickbarItemRefreshTraceText {
    param(
        [string]$Text
    )

    if ([string]::IsNullOrEmpty($Text)) {
        return ''
    }

    $message = [regex]::Escape('semantic state ended with unresolved pending GuiQuickbar item refresh')
    $lines = foreach ($line in ($Text -split "`r?`n")) {
        if ($line -match $message) {
            $line
        }
    }
    return ($lines -join "`n")
}

function Get-SemanticUnresolvedQuickbarItemRefreshTraceCount {
    param(
        [string]$Text
    )

    $traceText = Get-SemanticUnresolvedQuickbarItemRefreshTraceText -Text $Text
    return Get-TextMatchCount -Text $traceText -Pattern 'semantic state ended with unresolved pending GuiQuickbar item refresh'
}

function Get-SemanticUnresolvedQuickbarItemRefreshTraceFieldMax {
    param(
        [string]$Text,
        [string]$Field
    )

    $traceText = Get-SemanticUnresolvedQuickbarItemRefreshTraceText -Text $Text
    return Get-TraceFieldMax `
        -Text $traceText `
        -Message 'semantic state ended with unresolved pending GuiQuickbar item refresh' `
        -Field $Field
}

function Get-SemanticUnresolvedQuickbarItemRefreshStringFieldCount {
    param(
        [string]$Text,
        [string]$Field,
        [string]$Value
    )

    $traceText = Get-SemanticUnresolvedQuickbarItemRefreshTraceText -Text $Text
    if ([string]::IsNullOrEmpty($traceText)) {
        return 0
    }

    $pattern = '\b' + [regex]::Escape($Field) + '="' + [regex]::Escape($Value) + '"'
    return Get-TextMatchCount -Text $traceText -Pattern $pattern
}

function Get-SemanticUnresolvedQuickbarItemRefreshFlagCount {
    param(
        [string]$Text,
        [string]$Field,
        [string]$Value
    )

    $traceText = Get-SemanticUnresolvedQuickbarItemRefreshTraceText -Text $Text
    if ([string]::IsNullOrEmpty($traceText)) {
        return 0
    }

    $pattern = '\b' + [regex]::Escape($Field) + '=' + [regex]::Escape($Value) + '\b'
    return Get-TextMatchCount -Text $traceText -Pattern $pattern
}

function Get-QuickbarRewriteTraceText {
    param(
        [string]$Text,
        [bool]$Committed
    )

    if ([string]::IsNullOrEmpty($Text)) {
        return ''
    }

    $message = [regex]::Escape('server GuiQuickbar_SetAllButtons rewrite summary')
    $commitPattern = if ($Committed) { '\bcommitted=true\b' } else { '\bcommitted=false\b' }
    $lines = foreach ($line in ($Text -split "`r?`n")) {
        if ($line -match $message -and $line -match $commitPattern) {
            $line
        }
    }
    return ($lines -join "`n")
}

function Get-QuickbarCommittedRewriteTraceText {
    param(
        [string]$Text
    )

    return Get-QuickbarRewriteTraceText -Text $Text -Committed $true
}

function Get-QuickbarStreamProbeRewriteTraceText {
    param(
        [string]$Text
    )

    return Get-QuickbarRewriteTraceText -Text $Text -Committed $false
}

function Get-QuickbarCommittedRewriteTraceCount {
    param(
        [string]$Text
    )

    $committedText = Get-QuickbarCommittedRewriteTraceText -Text $Text
    return Get-TextMatchCount -Text $committedText -Pattern 'server GuiQuickbar_SetAllButtons rewrite summary'
}

function Get-QuickbarStreamProbeRewriteTraceCount {
    param(
        [string]$Text
    )

    $streamProbeText = Get-QuickbarStreamProbeRewriteTraceText -Text $Text
    return Get-TextMatchCount -Text $streamProbeText -Pattern 'server GuiQuickbar_SetAllButtons rewrite summary'
}

function Get-QuickbarItemDecisionTraceText {
    param(
        [string]$Text,
        [bool]$Committed
    )

    if ([string]::IsNullOrEmpty($Text)) {
        return ''
    }

    $message = [regex]::Escape('server GuiQuickbar_SetAllButtons item materialization decision')
    $commitPattern = if ($Committed) { '\bcommitted=true\b' } else { '\bcommitted=false\b' }
    $lines = foreach ($line in ($Text -split "`r?`n")) {
        if ($line -match $message -and $line -match $commitPattern) {
            $line
        }
    }
    return ($lines -join "`n")
}

function Get-QuickbarCommittedItemDecisionTraceText {
    param(
        [string]$Text
    )

    return Get-QuickbarItemDecisionTraceText -Text $Text -Committed $true
}

function Get-QuickbarStreamProbeItemDecisionTraceText {
    param(
        [string]$Text
    )

    return Get-QuickbarItemDecisionTraceText -Text $Text -Committed $false
}

function Get-QuickbarCommittedItemDecisionTraceCount {
    param(
        [string]$Text
    )

    $committedText = Get-QuickbarCommittedItemDecisionTraceText -Text $Text
    return Get-TextMatchCount -Text $committedText -Pattern 'server GuiQuickbar_SetAllButtons item materialization decision'
}

function Get-QuickbarStreamProbeItemDecisionTraceCount {
    param(
        [string]$Text
    )

    $streamProbeText = Get-QuickbarStreamProbeItemDecisionTraceText -Text $Text
    return Get-TextMatchCount -Text $streamProbeText -Pattern 'server GuiQuickbar_SetAllButtons item materialization decision'
}

function Get-QuickbarItemDecisionFlagCount {
    param(
        [string]$Text,
        [string]$Field,
        [string]$Value,
        [bool]$Committed = $true
    )

    $traceText = Get-QuickbarItemDecisionTraceText -Text $Text -Committed $Committed
    if ([string]::IsNullOrEmpty($traceText)) {
        return 0
    }

    $pattern = '\b' + [regex]::Escape($Field) + '=' + [regex]::Escape($Value) + '\b'
    return Get-TextMatchCount -Text $traceText -Pattern $pattern
}

function Get-QuickbarCommittedItemDecisionFlagCount {
    param(
        [string]$Text,
        [string]$Field,
        [string]$Value
    )

    return Get-QuickbarItemDecisionFlagCount -Text $Text -Field $Field -Value $Value -Committed $true
}

function Get-QuickbarStreamProbeItemDecisionFlagCount {
    param(
        [string]$Text,
        [string]$Field,
        [string]$Value
    )

    return Get-QuickbarItemDecisionFlagCount -Text $Text -Field $Field -Value $Value -Committed $false
}

function Get-TerminalFragmentResidualSummary {
    param(
        [string]$Text
    )

    $empty = [pscustomobject]@{
        Count = 0
        FirstOffset = $null
        FirstRecordEnd = $null
        FirstBitCursor = $null
        LastOffset = $null
        LastRecordEnd = $null
        LastBitCursor = $null
    }
    if ([string]::IsNullOrEmpty($Text)) {
        return $empty
    }

    $pattern = 'live-object update rewrite cursor unreliable: reason=terminal-fragment-bits-unowned-after-rewrite offset=(\d+) record_end=(\d+) bit_cursor=(\d+)'
    $matches = [regex]::Matches($Text, $pattern)
    if ($matches.Count -eq 0) {
        return $empty
    }

    $first = $matches[0]
    $last = $matches[$matches.Count - 1]
    return [pscustomobject]@{
        Count = $matches.Count
        FirstOffset = [int]$first.Groups[1].Value
        FirstRecordEnd = [int]$first.Groups[2].Value
        FirstBitCursor = [int]$first.Groups[3].Value
        LastOffset = [int]$last.Groups[1].Value
        LastRecordEnd = [int]$last.Groups[2].Value
        LastBitCursor = [int]$last.Groups[3].Value
    }
}

function Convert-LiveObjectClaimAcceptedLine {
    param(
        [string]$Line
    )

    $pattern = 'live-object claim accepted: family=([^ ]+) offset=(\d+) record_end=(\d+) bit_cursor=(\d+) opcode=0x([0-9A-Fa-f]+) marker=0x([0-9A-Fa-f]+)'
    $match = [regex]::Match($Line, $pattern)
    if (-not $match.Success) {
        return $null
    }

    return [pscustomobject]@{
        Family = $match.Groups[1].Value
        Offset = [int]$match.Groups[2].Value
        RecordEnd = [int]$match.Groups[3].Value
        BitCursor = [int]$match.Groups[4].Value
        Opcode = "0x$($match.Groups[5].Value)"
        Marker = "0x$($match.Groups[6].Value)"
    }
}

function Export-TerminalFragmentResidualEvidence {
    param(
        [string]$Text,
        [string]$QuarantineDir,
        [string]$OutputPath,
        [string]$PayloadCopyPath
    )

    $empty = [pscustomobject]@{
        Path = $null
        PayloadCopy = $null
        Count = 0
    }
    if ([string]::IsNullOrEmpty($Text)) {
        return $empty
    }

    $lines = $Text -split "`r?`n"
    $failurePattern = 'live-object update rewrite cursor unreliable: reason=terminal-fragment-bits-unowned-after-rewrite offset=(\d+) record_end=(\d+) bit_cursor=(\d+)'
    $gatePattern = 'live-object terminal fragment trim gate: bit_cursor=(\d+) fragment_bits=(\d+) residual_bits=(\d+) residual_preview=\[([^\]]*)\]'
    $events = @()

    for ($i = 0; $i -lt $lines.Count; $i++) {
        $failure = [regex]::Match($lines[$i], $failurePattern)
        if (-not $failure.Success) {
            continue
        }

        $start = [Math]::Max(0, $i - 80)
        for ($j = $i - 1; $j -ge $start; $j--) {
            if ($lines[$j] -match 'live-object claim boundary: offset=0\b') {
                $start = $j
                break
            }
        }

        $gate = $null
        $claims = @()
        for ($j = $start; $j -lt $i; $j++) {
            $gateMatch = [regex]::Match($lines[$j], $gatePattern)
            if ($gateMatch.Success) {
                $gate = $gateMatch
            }

            $claim = Convert-LiveObjectClaimAcceptedLine -Line $lines[$j]
            if ($null -ne $claim) {
                $claims += $claim
            }
        }

        $gateObject = $null
        if ($null -ne $gate) {
            $gateObject = [pscustomobject]@{
                BitCursor = [int]$gate.Groups[1].Value
                FragmentBits = [int]$gate.Groups[2].Value
                ResidualBits = [int]$gate.Groups[3].Value
                ResidualPreview = $gate.Groups[4].Value
            }
        }

        $events += [pscustomobject]@{
            Offset = [int]$failure.Groups[1].Value
            RecordEnd = [int]$failure.Groups[2].Value
            BitCursor = [int]$failure.Groups[3].Value
            Gate = $gateObject
            AcceptedRecords = @($claims)
        }
    }

    if ($events.Count -eq 0) {
        return $empty
    }

    $payloadCopy = $null
    if (Test-Path -LiteralPath $QuarantineDir -PathType Container) {
        $payload = Get-ChildItem -LiteralPath $QuarantineDir -File -Filter 'live-object-unclaimed-strict-family*.bin' -ErrorAction SilentlyContinue |
            Sort-Object LastWriteTime |
            Select-Object -First 1
        if ($null -eq $payload) {
            $payload = Get-ChildItem -LiteralPath $QuarantineDir -File -Filter '*.bin' -ErrorAction SilentlyContinue |
                Sort-Object LastWriteTime |
                Select-Object -First 1
        }
        if ($null -ne $payload) {
            Copy-Item -LiteralPath $payload.FullName -Destination $PayloadCopyPath -Force
            $payloadCopy = $PayloadCopyPath
        }
    }

    $report = [pscustomobject]@{
        Count = $events.Count
        PayloadCopy = $payloadCopy
        Events = @($events)
    }
    $report | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $OutputPath -Encoding UTF8

    return [pscustomobject]@{
        Path = $OutputPath
        PayloadCopy = $payloadCopy
        Count = $events.Count
    }
}

function Get-FileCount {
    param(
        [string]$Path,
        [string]$Filter
    )

    if (-not (Test-Path -LiteralPath $Path -PathType Container)) {
        return 0
    }
    return @(Get-ChildItem -LiteralPath $Path -Recurse -File -Filter $Filter -ErrorAction SilentlyContinue).Count
}

function Assert-ReplayDeadline {
    param(
        [object]$DeadlineUtc,
        [int]$TimeoutSeconds,
        [string]$Stage
    )

    if ($null -ne $DeadlineUtc -and [DateTime]::UtcNow -ge [DateTime]$DeadlineUtc) {
        throw "Diamond replay timed out after $TimeoutSeconds seconds during $Stage"
    }
}

$repositoryRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path
if ($TimeoutSeconds -lt 0) {
    throw "-TimeoutSeconds must be zero or positive"
}
if ($DrainReceiveTimeoutMilliseconds -le 0) {
    throw "-DrainReceiveTimeoutMilliseconds must be positive"
}
if ($ProxyOutputWaitMilliseconds -lt 0) {
    throw "-ProxyOutputWaitMilliseconds must be zero or positive"
}

$packetDirResolved = Resolve-RequiredDirectory -Path $PacketDir -Label 'Diamond packet dump directory'
$proxyResolved = Resolve-Proxy2Executable -ExplicitPath $ProxyExe -RepositoryRoot $repositoryRoot -SkipBuild:$SkipBuild

if ([string]::IsNullOrWhiteSpace($RunRoot)) {
    $stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
    $RunRoot = Join-Path 'C:\nwnbridge' "proxy2-diamond-replay-$stamp"
}
$RunRoot = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($RunRoot)
New-Item -ItemType Directory -Force -Path $RunRoot | Out-Null

$proxyLog = Join-Path $RunRoot 'proxy2.log'
$proxyStdout = Join-Path $RunRoot 'proxy2.stdout.log'
$proxyStderr = Join-Path $RunRoot 'proxy2.stderr.log'
$quickbarItemRefreshHint = Join-Path $RunRoot 'quickbar-item-refresh-hint.json'
$summaryPath = Join-Path $RunRoot 'replay-summary.json'
$deadlineUtc = if ($TimeoutSeconds -gt 0) { [DateTime]::UtcNow.AddSeconds($TimeoutSeconds) } else { $null }

$previousDebugLiveClaim = $env:HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM
if ($DebugLiveClaim) {
    $env:HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM = '1'
}

$proxyArgs = @(
    '--listen', "127.0.0.1:$ListenPort",
    '--server', "127.0.0.1:$ServerPort",
    '--log', $proxyLog,
    '--quickbar-item-refresh-hint', $quickbarItemRefreshHint,
    '--packet-dump'
)
if (-not $NoStrictTranslate) {
    $proxyArgs += '--strict-translate'
}
if (-not $EnableNwsync) {
    $proxyArgs += '--disable-nwsync'
}

$proxy = $null
$client = $null
$server = $null
try {
    $proxy = Start-Process `
        -FilePath $proxyResolved `
        -ArgumentList $proxyArgs `
        -WorkingDirectory $repositoryRoot `
        -RedirectStandardOutput $proxyStdout `
        -RedirectStandardError $proxyStderr `
        -WindowStyle Hidden `
        -PassThru

    $ready = $false
    for ($i = 0; $i -lt 80; $i++) {
        Assert-ReplayDeadline -DeadlineUtc $deadlineUtc -TimeoutSeconds $TimeoutSeconds -Stage 'proxy startup'
        if ($proxy.HasExited) {
            $stderr = Get-Content -LiteralPath $proxyStderr -Raw -ErrorAction SilentlyContinue
            throw "proxy2 exited early with code $($proxy.ExitCode): $stderr"
        }
        if (Test-Path -LiteralPath $proxyLog) {
            $tail = Get-Content -LiteralPath $proxyLog -Tail 20 -ErrorAction SilentlyContinue
            if ($tail -match 'hgbridge_proxy2 starting') {
                $ready = $true
                break
            }
        }
        Start-Sleep -Milliseconds 250
    }
    if (-not $ready) {
        throw "proxy2 did not write startup log at $proxyLog"
    }

    $crcTable = New-LegacyMcrcTable
    $server = [System.Net.Sockets.UdpClient]::new($ServerPort)
    $server.Client.ReceiveTimeout = $DrainReceiveTimeoutMilliseconds
    $client = [System.Net.Sockets.UdpClient]::new(0)
    $client.Client.ReceiveTimeout = $DrainReceiveTimeoutMilliseconds
    $client.Connect('127.0.0.1', $ListenPort)

    $proxyServerEndpoint = $null
    $clientPacketsSent = 0
    $serverPacketsSent = 0
    $serverPacketsSkipped = 0
    $proxyPacketsReceivedByDummyServer = 0
    $proxyPacketsReceivedByDummyClient = 0
    $generatedClientAcks = 0
    $capturedRecvMFrames = 0
    $capturedRecvLiveObjectDirectFrames = 0
    $capturedRecvAreaClientAreaDirectFrames = 0
    $ackedSequences = [System.Collections.Generic.HashSet[int]]::new()
    $serverDeflatedFramesUntilCompletion = 0
    $proxyOutputWaitEvents = 0
    $proxyOutputWaitTimeouts = 0
    $generateClientAcks = -not [bool]$NoGeneratedClientAcks
    $seedEeBnxiEnabled = -not [bool]$NoSeedEeBnxi
    $seededEeBnxiSent = $false
    $seedEeBnxiPlacement = if ($seedEeBnxiEnabled) { 'before-first-BNCS' } else { '<disabled>' }
    [byte[]]$seedBnxi = @()
    if ($seedEeBnxiEnabled) {
        $seedBnxi = New-SeedEeBnxiPacket `
            -UdpPort $SeedEeBnxiUdpPort `
            -Major $SeedEeBnxiMajor `
            -Minor $SeedEeBnxiMinor `
            -Revision $SeedEeBnxiRevision `
            -BuildHash $SeedEeBnxiBuildHash
    }

    $files = Get-ChildItem -LiteralPath $packetDirResolved -Filter '*.bin' | Sort-Object Name
    $capturePerspectiveResolved = Resolve-CapturePerspective `
        -PacketDir $packetDirResolved `
        -Files $files `
        -RequestedPerspective $CapturePerspective
    foreach ($file in $files) {
        Assert-ReplayDeadline -DeadlineUtc $deadlineUtc -TimeoutSeconds $TimeoutSeconds -Stage "packet replay $($file.Name)"
        [byte[]]$bytes = [System.IO.File]::ReadAllBytes($file.FullName)
        $proxyOutputWaitAfterPacket = 0
        if (Test-CapturedClientToServer -FileName $file.Name -Perspective $capturePerspectiveResolved) {
            if ($seedEeBnxiEnabled -and -not $seededEeBnxiSent -and (Test-PacketTag -Bytes $bytes -Tag 'BNCS')) {
                [void]$client.Send($seedBnxi, $seedBnxi.Length)
                $clientPacketsSent++
                $seededEeBnxiSent = $true
                Start-Sleep -Milliseconds $PacketDelayMilliseconds
                Assert-ReplayDeadline -DeadlineUtc $deadlineUtc -TimeoutSeconds $TimeoutSeconds -Stage "seed BNXI delay after $($file.Name)"
                Drain-DummyServer -Server $server -ProxyServerEndpoint ([ref]$proxyServerEndpoint) -ReceivedCount ([ref]$proxyPacketsReceivedByDummyServer) -DeadlineUtc $deadlineUtc -TimeoutSeconds $TimeoutSeconds -Stage "drain dummy server after seeded BNXI $($file.Name)"
                Drain-DummyClient `
                    -Client $client `
                    -ReceivedCount ([ref]$proxyPacketsReceivedByDummyClient) `
                    -GeneratedAckCount ([ref]$generatedClientAcks) `
                    -AckedSequences $ackedSequences `
                    -CrcTable $crcTable `
                    -GenerateClientAcks $generateClientAcks `
                    -DeadlineUtc $deadlineUtc `
                    -TimeoutSeconds $TimeoutSeconds `
                    -Stage "drain dummy client after seeded BNXI $($file.Name)"
                Drain-DummyServer -Server $server -ProxyServerEndpoint ([ref]$proxyServerEndpoint) -ReceivedCount ([ref]$proxyPacketsReceivedByDummyServer) -DeadlineUtc $deadlineUtc -TimeoutSeconds $TimeoutSeconds -Stage "drain dummy server after seeded BNXI client drain $($file.Name)"
            }
            [void]$client.Send($bytes, $bytes.Length)
            $clientPacketsSent++
        } elseif (Test-CapturedServerToClient -FileName $file.Name -Perspective $capturePerspectiveResolved) {
            if ($bytes.Length -gt 0 -and $bytes[0] -eq [byte][char]'M') {
                $capturedRecvMFrames++
                if ($bytes.Length -ge 15) {
                    $envelope = $bytes[12]
                    if (($envelope -eq [byte][char]'P' -or $envelope -eq 0x70) -and $bytes[13] -eq 0x05 -and $bytes[14] -eq 0x01) {
                        $capturedRecvLiveObjectDirectFrames++
                    }
                    if (($envelope -eq [byte][char]'P' -or $envelope -eq 0x70) -and $bytes[13] -eq 0x04 -and $bytes[14] -eq 0x01) {
                        $capturedRecvAreaClientAreaDirectFrames++
                    }
                }
            }

            if ($null -eq $proxyServerEndpoint) {
                Drain-DummyServer -Server $server -ProxyServerEndpoint ([ref]$proxyServerEndpoint) -ReceivedCount ([ref]$proxyPacketsReceivedByDummyServer) -DeadlineUtc $deadlineUtc -TimeoutSeconds $TimeoutSeconds -Stage "discover proxy server endpoint before $($file.Name)"
            }
            if ($null -ne $proxyServerEndpoint) {
                [void]$server.Send($bytes, $bytes.Length, $proxyServerEndpoint)
                $serverPacketsSent++
                $proxyOutputWaitAfterPacket = Get-ServerMCompletionOutputWait `
                    -Bytes $bytes `
                    -PendingDeflatedFrames ([ref]$serverDeflatedFramesUntilCompletion) `
                    -ProxyOutputWaitMilliseconds $ProxyOutputWaitMilliseconds
            } else {
                $serverPacketsSkipped++
            }
        }

        $clientPacketsBeforeDrain = $proxyPacketsReceivedByDummyClient
        Start-Sleep -Milliseconds $PacketDelayMilliseconds
        Assert-ReplayDeadline -DeadlineUtc $deadlineUtc -TimeoutSeconds $TimeoutSeconds -Stage "packet delay after $($file.Name)"
        Drain-DummyServer -Server $server -ProxyServerEndpoint ([ref]$proxyServerEndpoint) -ReceivedCount ([ref]$proxyPacketsReceivedByDummyServer) -DeadlineUtc $deadlineUtc -TimeoutSeconds $TimeoutSeconds -Stage "drain dummy server after $($file.Name)"
        Drain-DummyClient `
            -Client $client `
            -ReceivedCount ([ref]$proxyPacketsReceivedByDummyClient) `
            -GeneratedAckCount ([ref]$generatedClientAcks) `
            -AckedSequences $ackedSequences `
            -CrcTable $crcTable `
            -GenerateClientAcks $generateClientAcks `
            -DeadlineUtc $deadlineUtc `
            -TimeoutSeconds $TimeoutSeconds `
            -Stage "drain dummy client after $($file.Name)"
        Drain-DummyServer -Server $server -ProxyServerEndpoint ([ref]$proxyServerEndpoint) -ReceivedCount ([ref]$proxyPacketsReceivedByDummyServer) -DeadlineUtc $deadlineUtc -TimeoutSeconds $TimeoutSeconds -Stage "final drain dummy server after $($file.Name)"
        if ($proxyOutputWaitAfterPacket -gt 0 -and $proxyPacketsReceivedByDummyClient -eq $clientPacketsBeforeDrain) {
            $proxyOutputWaitEvents++
            $sawProxyOutput = Wait-DummyClientOutput `
                -Server $server `
                -Client $client `
                -ProxyServerEndpoint ([ref]$proxyServerEndpoint) `
                -ServerReceivedCount ([ref]$proxyPacketsReceivedByDummyServer) `
                -ClientReceivedCount ([ref]$proxyPacketsReceivedByDummyClient) `
                -GeneratedAckCount ([ref]$generatedClientAcks) `
                -AckedSequences $ackedSequences `
                -CrcTable $crcTable `
                -GenerateClientAcks $generateClientAcks `
                -InitialClientReceivedCount $clientPacketsBeforeDrain `
                -WaitMilliseconds $proxyOutputWaitAfterPacket `
                -DeadlineUtc $deadlineUtc `
                -TimeoutSeconds $TimeoutSeconds `
                -Stage "wait for proxy output after $($file.Name)"
            if (-not $sawProxyOutput) {
                $proxyOutputWaitTimeouts++
            }
        }
    }

    for ($i = 0; $i -lt $FinalDrainRounds; $i++) {
        Assert-ReplayDeadline -DeadlineUtc $deadlineUtc -TimeoutSeconds $TimeoutSeconds -Stage "final drain round $i"
        Start-Sleep -Milliseconds 100
        Drain-DummyClient `
            -Client $client `
            -ReceivedCount ([ref]$proxyPacketsReceivedByDummyClient) `
            -GeneratedAckCount ([ref]$generatedClientAcks) `
            -AckedSequences $ackedSequences `
            -CrcTable $crcTable `
            -GenerateClientAcks $generateClientAcks `
            -DeadlineUtc $deadlineUtc `
            -TimeoutSeconds $TimeoutSeconds `
            -Stage "final drain dummy client round $i"
        Drain-DummyServer -Server $server -ProxyServerEndpoint ([ref]$proxyServerEndpoint) -ReceivedCount ([ref]$proxyPacketsReceivedByDummyServer) -DeadlineUtc $deadlineUtc -TimeoutSeconds $TimeoutSeconds -Stage "final drain dummy server round $i"
    }

    $quarantineDir = Join-Path $RunRoot 'quarantine'
    $proxyLogText = ''
    if (Test-Path -LiteralPath $proxyLog -PathType Leaf) {
        $proxyLogText = Get-Content -LiteralPath $proxyLog -Raw -ErrorAction SilentlyContinue
    }
    $proxyLogLineCount = 0
    if (-not [string]::IsNullOrEmpty($proxyLogText)) {
        $proxyLogLineCount = ($proxyLogText -split "`r?`n").Count
    }
    $proxyStderrText = ''
    if (Test-Path -LiteralPath $proxyStderr -PathType Leaf) {
        $proxyStderrText = Get-Content -LiteralPath $proxyStderr -Raw -ErrorAction SilentlyContinue
    }
    $terminalResidualSummary = Get-TerminalFragmentResidualSummary -Text $proxyStderrText
    $terminalResidualReport = Export-TerminalFragmentResidualEvidence `
        -Text $proxyStderrText `
        -QuarantineDir $quarantineDir `
        -OutputPath (Join-Path $RunRoot 'live-object-terminal-residuals.json') `
        -PayloadCopyPath (Join-Path $RunRoot 'live-object-terminal-residual.bin')

    $quickbarHintExists = Test-Path -LiteralPath $quickbarItemRefreshHint -PathType Leaf
    $quickbarHintParseError = ''
    $quickbarHintJson = $null
    if ($quickbarHintExists) {
        try {
            $quickbarHintJson = Get-Content -LiteralPath $quickbarItemRefreshHint -Raw | ConvertFrom-Json
        } catch {
            $quickbarHintParseError = $_.Exception.Message
        }
    }
    $quickbarHintPending = $false
    $quickbarHintCandidateObjectId = 0
    $quickbarHintCandidateProof = ''
    $quickbarHintCandidateSource = ''
    $quickbarHintNoHintReason = ''
    $quickbarHintPostCommittedItemRefreshResolution = ''
    $quickbarHintFirstActionMatchesCandidate = $false
    $quickbarHintFirstActionMatchesPreservedActiveItem = $false
    $quickbarHintFirstPreservedActiveItemSlotKnown = $false
    $quickbarHintFirstPreservedActiveItemSlot = 0
    $quickbarHintFirstPreservedActiveItemFirstPageSlot = $false
    $quickbarHintFirstPreservedActiveItemSlotMatchesRecommendedSetButtonSlot = $false
    $quickbarHintFirstActionMatchClass = ''
    $quickbarHintRecommendedActionOutcome = ''
    $quickbarHintActivePropertyOutcome = ''
    $quickbarHintServerQuickbarResponseTiming = ''
    $quickbarHintStreamProbeItemButtonsRejectedMissingStateClearedDelete = 0
    $quickbarHintStreamProbeItemButtonsRejectedMissingStateClearedAreaReset = 0
    $quickbarHintStreamProbeItemObjectsRejectedMissingStateProven = 0
    $quickbarHintStreamProbeItemObjectsRejectedMissingStateActive = 0
    $quickbarHintStreamProbeItemObjectsRejectedMissingStateFeature25First = 0
    $quickbarHintStreamProbeItemObjectsRejectedMissingStateFeature25Second = 0
    $quickbarHintStreamProbeItemObjectsRejectedMissingStateFeature25LegacyTail = 0
    $quickbarHintStreamProbeItemObjectsRejectedMissingStateUnknown = 0
    $quickbarHintStreamProbeItemObjectsRejectedMissingStateClearedDelete = 0
    $quickbarHintStreamProbeItemObjectsRejectedMissingStateClearedAreaReset = 0
    $quickbarHintStreamProbeItemObjectsPreservedByExplicitSelfMaterialization = 0
    $quickbarHintStreamProbeItemObjectsPreservedByActiveState = 0
    $quickbarHintStreamProbeItemObjectsPreservedByFeature25First = 0
    $quickbarHintStreamProbeItemObjectsPreservedByFeature25Second = 0
    $quickbarHintStreamProbeItemObjectsPreservedByFeature25LegacyTail = 0
    $quickbarHintInventoryFeature25ReferenceRecords = 0
    $quickbarHintInventoryFeature25ItemRefMentions = 0
    $quickbarHintInventoryFeature25MaterializedItemRefMentions = 0
    $quickbarHintInventoryFeature25DeferredItemRefMentions = 0
    $quickbarHintInventoryFeature25MaterializationOutcome = ''
    $quickbarHintInventoryFeature25HandoffOutcome = ''
    $quickbarHintInventoryEquipmentHandoffReady = $false
    $quickbarHintInventoryEquipmentHandoffOutcome = ''
    $quickbarHintInventoryEquipmentHandoffEvents = 0
    $quickbarHintInventoryEquipmentHandoffReadyEvents = 0
    $quickbarHintInventoryEquipmentHandoffBlockedWithoutReadyStateEvents = 0
    $quickbarHintInventoryEquipmentHandoffReadyWithDeferredFeature25Events = 0
    $quickbarHintInventoryEquipmentHandoffServerInventoryEvents = 0
    $quickbarHintInventoryEquipmentHandoffServerInventoryReadyEvents = 0
    $quickbarHintInventoryEquipmentHandoffServerInventoryBlockedWithoutReadyStateEvents = 0
    $quickbarHintInventoryEquipmentHandoffClientGuiInventoryEvents = 0
    $quickbarHintInventoryEquipmentHandoffClientGuiInventoryReadyEvents = 0
    $quickbarHintInventoryEquipmentHandoffClientGuiInventoryBlockedWithoutReadyStateEvents = 0
    $quickbarHintLastInventoryEquipmentHandoffKnown = $false
    $quickbarHintLastInventoryEquipmentHandoffConsumer = ''
    $quickbarHintLastInventoryEquipmentHandoffEventIndex = 0
    $quickbarHintLastInventoryEquipmentHandoffOutcome = ''
    $quickbarHintLastInventoryEquipmentHandoffReadyObjects = 0
    $quickbarHintLastInventoryEquipmentHandoffDeferredFeature25OnlyObjects = 0
    $quickbarHintLastInventoryEquipmentHandoffCandidateKnown = $false
    $quickbarHintLastInventoryEquipmentHandoffCandidateObjectId = 0
    $quickbarHintLastInventoryEquipmentHandoffCandidateProof = ''
    $quickbarHintLastInventoryEquipmentHandoffCandidateSource = ''
    $quickbarHintInventoryEquipmentBridgeHandoffAction = ''
    $quickbarHintInventoryEquipmentBridgeHandoffReady = $false
    $quickbarHintInventoryEquipmentBridgeHandoffConsumer = ''
    $quickbarHintInventoryEquipmentBridgeHandoffEventIndex = 0
    $quickbarHintInventoryEquipmentBridgeHandoffOutcome = ''
    $quickbarHintInventoryEquipmentBridgeHandoffReadyObjects = 0
    $quickbarHintInventoryEquipmentBridgeHandoffDeferredFeature25OnlyObjects = 0
    $quickbarHintInventoryEquipmentBridgeHandoffCandidateKnown = $false
    $quickbarHintInventoryEquipmentBridgeHandoffCandidateObjectId = 0
    $quickbarHintInventoryEquipmentBridgeHandoffCandidateProof = ''
    $quickbarHintInventoryEquipmentBridgeHandoffCandidateSource = ''
    $quickbarHintInventoryEquipmentBridgeHandoffEmissions = 0
    $quickbarHintInventoryEquipmentBridgeHandoffLastEmittedKnown = $false
    $quickbarHintInventoryEquipmentBridgeHandoffLastEmittedIndex = 0
    $quickbarHintInventoryEquipmentBridgeHandoffLastEmittedConsumer = ''
    $quickbarHintInventoryEquipmentBridgeHandoffLastEmittedEventIndex = 0
    $quickbarHintInventoryEquipmentBridgeHandoffLastEmittedCandidateObjectId = 0
    $quickbarHintInventoryEquipmentBridgeHandoffLastEmittedCandidateSource = ''
    $quickbarHintInventoryEquipmentBridgeHandoffStateUpdates = 0
    $quickbarHintInventoryEquipmentBridgeHandoffLastStateUpdateKnown = $false
    $quickbarHintInventoryEquipmentBridgeHandoffLastStateUpdateIndex = 0
    $quickbarHintInventoryEquipmentBridgeHandoffLastStateUpdateEmissionIndex = 0
    $quickbarHintInventoryEquipmentBridgeHandoffLastStateUpdateConsumer = ''
    $quickbarHintInventoryEquipmentBridgeHandoffLastStateUpdateEventIndex = 0
    $quickbarHintInventoryEquipmentBridgeHandoffLastStateUpdateCandidateObjectId = 0
    $quickbarHintInventoryEquipmentBridgeHandoffLastStateUpdateCandidateProof = ''
    $quickbarHintInventoryEquipmentBridgeHandoffLastStateUpdateCandidateSource = ''
    $quickbarHintInventoryEquipmentBridgeHandoffLastStateUpdateReadyObjects = 0
    $quickbarHintInventoryEquipmentBridgeHandoffLastStateUpdateDeferredFeature25OnlyObjects = 0
    $quickbarHintInventoryEquipmentBridgeOutputQueuedPackets = 0
    $quickbarHintInventoryEquipmentBridgeOutputDeferredClientGuiUpdates = 0
    $quickbarHintInventoryEquipmentBridgeOutputDeferredMissingClaimUpdates = 0
    $quickbarHintInventoryEquipmentBridgeOutputBlockedCandidateMismatchUpdates = 0
    $quickbarHintInventoryEquipmentBridgeOutputStatus = ''
    $quickbarHintInventoryEquipmentBridgeOutputRequiresClientGuiWriter = $false
    $quickbarHintInventoryEquipmentBridgeOutputLastDecisionUpdateIndex = 0
    $quickbarHintInventoryEquipmentBridgeOutputLastDecisionReadyObjects = 0
    $quickbarHintInventoryEquipmentBridgeOutputLastDecisionDeferredFeature25OnlyObjects = 0
    $quickbarHintInventoryEquipmentBridgeOutputLastDecisionClientGuiInventoryClaimKnown = $false
    $quickbarHintInventoryEquipmentBridgeOutputLastDecisionClientGuiInventoryClaimKind = ''
    $quickbarHintInventoryEquipmentBridgeOutputLastDecisionClientGuiInventoryClaimObjectId = 0
    $quickbarHintInventoryEquipmentBridgeOutputLastDecisionClientGuiInventoryClaimPanel = 0
    $quickbarHintInventoryEquipmentBridgeOutputLastDecisionClientGuiInventoryClaimPlayerInventoryGui = $false
    $quickbarHintInventoryEquipmentBridgeOutputLastDecisionClientGuiInventoryClaimRewrittenSelfObjectId = $false
    $quickbarHintInventoryEquipmentBridgeOutputClientGuiWriterPlanAction = ''
    $quickbarHintInventoryEquipmentBridgeOutputClientGuiWriterPlanEmissionEnabled = $false
    $quickbarHintInventoryEquipmentBridgeOutputClientGuiWriterPlanBlockedReason = ''
    $quickbarHintInventoryEquipmentBridgeOutputClientGuiWriterPlanPayloadAvailable = $false
    $quickbarHintInventoryEquipmentBridgeOutputClientGuiWriterPlanPayloadKind = ''
    $quickbarHintInventoryEquipmentBridgeOutputClientGuiWriterPlanPayloadHex = ''
    $quickbarHintInventoryEquipmentBridgeOutputClientGuiWriterPlanStatusObjectId = 0
    $quickbarHintInventoryEquipmentBridgeOutputClientGuiWriterPlanStatusObjectIsCurrentPlayer = $false
    $quickbarHintInventoryEquipmentBridgeOutputClientGuiWriterPlanSelectPanel = 0
    $quickbarHintInventoryEquipmentBridgeOutputClientGuiWriterPlanPlayerInventoryGui = $false
    $quickbarHintInventoryEquipmentBridgeOutputLastDecisionCandidateObjectStatus = ''
    $quickbarHintInventoryEquipmentBridgeOutputLastDecisionCandidateObjectStatusProof = ''
    $quickbarHintInventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimObjectStatus = ''
    $quickbarHintInventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimObjectStatusProof = ''
    $quickbarHintInventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimClosestProvenItemKnown = $false
    $quickbarHintInventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimClosestProvenItemObjectId = 0
    $quickbarHintInventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimClosestProvenItemDistance = 0
    $quickbarHintInventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimLowerProvenItemKnown = $false
    $quickbarHintInventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimLowerProvenItemObjectId = 0
    $quickbarHintInventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimLowerProvenItemDistance = 0
    $quickbarHintInventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimHigherProvenItemKnown = $false
    $quickbarHintInventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimHigherProvenItemObjectId = 0
    $quickbarHintInventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimHigherProvenItemDistance = 0
    $quickbarHintInventoryEquipmentBridgeOutputLastDeferredClientGuiUpdateIndex = 0
    $quickbarHintInventoryEquipmentBridgeOutputLastDeferredMissingClaimUpdateIndex = 0
    $quickbarHintInventoryEquipmentBridgeOutputLastBlockedCandidateMismatchUpdateIndex = 0
    $quickbarHintInventoryEquipmentBridgeOutputLastQueuedKnown = $false
    $quickbarHintInventoryEquipmentBridgeOutputLastQueuedUpdateIndex = 0
    $quickbarHintInventoryEquipmentBridgeOutputLastQueuedEmissionIndex = 0
    $quickbarHintInventoryEquipmentBridgeOutputLastQueuedEventIndex = 0
    $quickbarHintInventoryEquipmentBridgeOutputLastQueuedMinor = 0
    $quickbarHintInventoryEquipmentBridgeOutputLastQueuedObjectId = 0
    $quickbarHintInventoryEquipmentBridgeOutputLastQueuedResult = $false
    $quickbarHintInventoryEquipmentBridgeOutputLastQueuedEquipSlot = 0
    $quickbarHintInventoryEquipmentBridgeOutputLastQueuedTriggerSequence = 0
    $quickbarHintInventoryEquipmentBridgeOutputLastQueuedSyntheticSequence = 0
    $quickbarHintInventoryEquipmentBridgeOutputQueuedClientGuiStatusPackets = 0
    $quickbarHintInventoryEquipmentBridgeOutputLastQueuedClientGuiStatusKnown = $false
    $quickbarHintInventoryEquipmentBridgeOutputLastQueuedClientGuiStatusUpdateIndex = 0
    $quickbarHintInventoryEquipmentBridgeOutputLastQueuedClientGuiStatusEmissionIndex = 0
    $quickbarHintInventoryEquipmentBridgeOutputLastQueuedClientGuiStatusEventIndex = 0
    $quickbarHintInventoryEquipmentBridgeOutputLastQueuedClientGuiStatusObjectId = 0
    $quickbarHintInventoryEquipmentBridgeOutputLastQueuedClientGuiStatusPlayerInventoryGui = $false
    $quickbarHintInventoryEquipmentBridgeOutputLastQueuedClientGuiStatusPayloadHex = ''
    $quickbarHintInventoryEquipmentBridgeOutputLastQueuedClientGuiStatusTriggerClientSequence = 0
    $quickbarHintInventoryEquipmentBridgeOutputLastQueuedClientGuiStatusSyntheticSequence = 0
    $quickbarHintInventoryEquipmentBridgeOutputLastQueuedClientGuiStatusAckSequence = 0
    $quickbarHintInventoryEquipmentBridgeOutputLastQueuedClientGuiStatusCandidateKnown = $false
    $quickbarHintInventoryEquipmentBridgeOutputLastQueuedClientGuiStatusCandidateObjectId = 0
    $quickbarHintInventoryEquipmentBridgeOutputLastQueuedClientGuiStatusCandidateProof = ''
    $quickbarHintInventoryEquipmentBridgeOutputLastQueuedClientGuiStatusCandidateSource = ''
    $quickbarHintInventoryEquipmentBridgeOutputLastQueuedClientGuiStatusReadyObjects = 0
    $quickbarHintInventoryEquipmentBridgeOutputLastQueuedClientGuiStatusDeferredFeature25OnlyObjects = 0
    $quickbarHintInventoryEquipmentBridgeOutputClientGuiStatusResponseLiveObjectPackets = 0
    $quickbarHintInventoryEquipmentBridgeOutputClientGuiStatusResponseLiveGuiRecordPackets = 0
    $quickbarHintInventoryEquipmentBridgeOutputClientGuiStatusResponseMaterializedItemPackets = 0
    $quickbarHintInventoryEquipmentBridgeOutputLastClientGuiStatusResponseKnown = $false
    $quickbarHintInventoryEquipmentBridgeOutputLastClientGuiStatusResponseQueuedUpdateIndex = 0
    $quickbarHintInventoryEquipmentBridgeOutputLastClientGuiStatusResponseServerSequence = 0
    $quickbarHintInventoryEquipmentBridgeOutputLastClientGuiStatusResponseAckSequence = 0
    $quickbarHintInventoryEquipmentBridgeOutputLastClientGuiStatusResponseLiveGuiRecords = 0
    $quickbarHintInventoryEquipmentBridgeOutputLastClientGuiStatusResponseLiveGuiFragmentBits = 0
    $quickbarHintInventoryEquipmentBridgeOutputLastClientGuiStatusResponseMaterializedItemObjectIds = 0
    $quickbarHintInventoryEquipmentBridgeOutputLastClientGuiStatusResponseReadyObjects = 0
    $quickbarHintInventoryEquipmentBridgeOutputLastClientGuiStatusResponseCandidateKnown = $false
    $quickbarHintInventoryEquipmentBridgeOutputLastClientGuiStatusResponseCandidateObjectId = 0
    $quickbarHintInventoryEquipmentBridgeOutputLastClientGuiStatusResponseCandidateProof = ''
    $quickbarHintInventoryEquipmentBridgeOutputLastClientGuiStatusResponseCandidateSource = ''
    $quickbarHintInventoryEquipmentBridgeOutputClientGuiStatusResponseOutcome = ''
    $quickbarHintInventoryEquipmentBridgeOutputBestClientGuiStatusResponseKnown = $false
    $quickbarHintInventoryEquipmentBridgeOutputBestClientGuiStatusResponseQueuedUpdateIndex = 0
    $quickbarHintInventoryEquipmentBridgeOutputBestClientGuiStatusResponseServerSequence = 0
    $quickbarHintInventoryEquipmentBridgeOutputBestClientGuiStatusResponseAckSequence = 0
    $quickbarHintInventoryEquipmentBridgeOutputBestClientGuiStatusResponseLiveGuiRecords = 0
    $quickbarHintInventoryEquipmentBridgeOutputBestClientGuiStatusResponseLiveGuiFragmentBits = 0
    $quickbarHintInventoryEquipmentBridgeOutputBestClientGuiStatusResponseMaterializedItemObjectIds = 0
    $quickbarHintInventoryEquipmentBridgeOutputBestClientGuiStatusResponseReadyObjects = 0
    $quickbarHintInventoryEquipmentBridgeOutputBestClientGuiStatusResponseCandidateKnown = $false
    $quickbarHintInventoryEquipmentBridgeOutputBestClientGuiStatusResponseCandidateObjectId = 0
    $quickbarHintInventoryEquipmentBridgeOutputBestClientGuiStatusResponseCandidateProof = ''
    $quickbarHintInventoryEquipmentBridgeOutputBestClientGuiStatusResponseCandidateSource = ''
    $quickbarHintInventoryEquipmentBridgeOutputBestClientGuiStatusResponseAssociation = ''
    $quickbarHintInventoryEquipmentBridgeOutputBestClientGuiStatusResponseMatchesQueuedStatusCandidate = $false
    $quickbarHintInventoryEquipmentBridgeOutputBestClientGuiStatusResponseCandidateDeltaFromQueuedStatusCandidate = 0
    $quickbarHintCompactItemEmissionReadyObjects = 0
    $quickbarHintCompactItemEmissionDeferredFeature25OnlyObjects = 0
    $quickbarHintStreamProbeCompactItemEmissionReadyObjects = 0
    $quickbarHintStreamProbeCompactItemEmissionDeferredFeature25OnlyObjects = 0
    $quickbarHintInventoryFeature25FirstItemRefs = 0
    $quickbarHintInventoryFeature25FirstItemRefMentions = 0
    $quickbarHintInventoryFeature25FirstMaterializedItemRefMentions = 0
    $quickbarHintInventoryFeature25FirstDeferredItemRefMentions = 0
    $quickbarHintInventoryFeature25SecondItemRefs = 0
    $quickbarHintInventoryFeature25SecondItemRefMentions = 0
    $quickbarHintInventoryFeature25SecondMaterializedItemRefMentions = 0
    $quickbarHintInventoryFeature25SecondDeferredItemRefMentions = 0
    $quickbarHintInventoryFeature25LegacyTailItemRefs = 0
    $quickbarHintInventoryFeature25LegacyTailItemRefMentions = 0
    $quickbarHintInventoryFeature25LegacyTailMaterializedItemRefMentions = 0
    $quickbarHintInventoryFeature25LegacyTailDeferredItemRefMentions = 0
    $quickbarHintClearedInventoryItemObjectIds = 0
    $quickbarHintQuickbarItemUseCountStateRows = 0
    $quickbarHintQuickbarItemUseCountUpdatesObserved = 0
    $quickbarHintCandidateQuickbarItemUseCountStateKnown = $false
    $quickbarHintCandidateQuickbarItemUseCountStateSlotRelation = ''
    $quickbarHintCandidateQuickbarItemUseCountStateSlotMatchesFirstPreservedActiveItem = $false
    $quickbarHintCandidateQuickbarItemUseCountStateSlot = 0
    $quickbarHintCandidateQuickbarItemUseCountStateButtonType = 0
    $quickbarHintCandidateQuickbarItemUseCountStateObjectId = 0
    $quickbarHintCandidateQuickbarItemUseCountStateActivePropertyIndex = 0
    $quickbarHintCandidateQuickbarItemUseCountStateUseCount = 0
    $quickbarHintFirstPreservedActiveItemQuickbarUseCountStateKnown = $false
    $quickbarHintFirstPreservedActiveItemQuickbarUseCountStateSlotRelation = ''
    $quickbarHintFirstPreservedActiveItemQuickbarUseCountStateSlotMatchesFirstPreservedActiveItem = $false
    $quickbarHintFirstPreservedActiveItemQuickbarUseCountStateSlot = 0
    $quickbarHintFirstPreservedActiveItemQuickbarUseCountStateButtonType = 0
    $quickbarHintFirstPreservedActiveItemQuickbarUseCountStateObjectId = 0
    $quickbarHintFirstPreservedActiveItemQuickbarUseCountStateActivePropertyIndex = 0
    $quickbarHintFirstPreservedActiveItemQuickbarUseCountStateUseCount = 0
    $quickbarHintFirstServerQuickbarItemUseCountCandidateRowKnown = $false
    $quickbarHintFirstServerQuickbarItemUseCountCandidateRowTiming = ''
    $quickbarHintFirstServerQuickbarItemUseCountCandidateRowSlotRelation = ''
    $quickbarHintFirstServerQuickbarItemUseCountCandidateRowSlotMatchesFirstPreservedActiveItem = $false
    $quickbarHintFirstServerQuickbarItemUseCountCandidateRowSlot = 0
    $quickbarHintFirstServerQuickbarItemUseCountCandidateRowButtonType = 0
    $quickbarHintFirstServerQuickbarItemUseCountCandidateRowObjectId = 0
    $quickbarHintFirstServerQuickbarItemUseCountCandidateRowActivePropertyIndex = 0
    $quickbarHintFirstServerQuickbarItemUseCountCandidateRowUseCount = 0
    $quickbarHintFirstServerQuickbarItemUseCountCandidateRowBeforeFirstClientActionKnown = $false
    $quickbarHintFirstServerQuickbarItemUseCountCandidateRowBeforeFirstClientActionSlot = 0
    $quickbarHintFirstServerQuickbarItemUseCountCandidateRowBeforeFirstClientActionButtonType = 0
    $quickbarHintFirstServerQuickbarItemUseCountCandidateRowBeforeFirstClientActionActivePropertyIndex = 0
    $quickbarHintFirstServerQuickbarItemUseCountCandidateRowBeforeFirstClientActionUseCount = 0
    $quickbarHintFirstServerQuickbarItemUseCountCandidateRowAfterFirstClientActionKnown = $false
    $quickbarHintFirstServerQuickbarItemUseCountCandidateRowAfterFirstClientActionSlot = 0
    $quickbarHintFirstServerQuickbarItemUseCountCandidateRowAfterFirstClientActionButtonType = 0
    $quickbarHintFirstServerQuickbarItemUseCountCandidateRowAfterFirstClientActionActivePropertyIndex = 0
    $quickbarHintFirstServerQuickbarItemUseCountCandidateRowAfterFirstClientActionUseCount = 0
    $quickbarHintFirstClientActionTiming = ''
    $quickbarHintFollowupEventsBeforeFirstClientAction = 0
    $quickbarHintServerToClientEventsSincePendingRefresh = 0
    $quickbarHintClientToServerEventsSincePendingRefresh = 0
    $quickbarHintClientGuiEventEventsSincePendingRefresh = 0
    $quickbarHintServerQuickbarItemUseCountEventsSincePendingRefresh = 0
    $quickbarHintServerQuickbarItemUseCountRecordsSincePendingRefresh = 0
    $quickbarHintServerQuickbarItemUseCountRowsSincePendingRefresh = 0
    $quickbarHintServerQuickbarItemUseCountCandidateRowsSincePendingRefresh = 0
    $quickbarHintServerActiveItemPropertyEventsSincePendingRefresh = 0
    $quickbarHintServerActiveItemPropertyUsesEventsSincePendingRefresh = 0
    $quickbarHintServerActiveItemPropertyFullEventsSincePendingRefresh = 0
    $quickbarHintServerActiveItemPropertyCandidateEventsSincePendingRefresh = 0
    $quickbarHintServerActiveItemPropertyCandidateUsesEventsSincePendingRefresh = 0
    $quickbarHintServerActiveItemPropertyCandidateFullEventsSincePendingRefresh = 0
    $quickbarHintServerActiveItemPropertyCandidateChangedUseCountRowsSincePendingRefresh = 0
    $quickbarHintServerActiveItemPropertyCandidateFullPropertyRowsSincePendingRefresh = 0
    $quickbarHintFirstEventAfterClientAction = ''
    $quickbarHintEventsAfterFirstClientAction = 0
    $quickbarHintServerToClientEventsAfterFirstClientAction = 0
    $quickbarHintClientToServerEventsAfterFirstClientAction = 0
    $quickbarHintLiveObjectEventsAfterFirstClientAction = 0
    $quickbarHintQuickbarEventsAfterFirstClientAction = 0
    $quickbarHintServerQuickbarItemUseCountEventsAfterFirstClientAction = 0
    $quickbarHintServerQuickbarItemUseCountRecordsAfterFirstClientAction = 0
    $quickbarHintServerQuickbarItemUseCountRowsAfterFirstClientAction = 0
    $quickbarHintServerQuickbarItemUseCountCandidateRowsAfterFirstClientAction = 0
    $quickbarHintServerActiveItemPropertyEventsAfterFirstClientAction = 0
    $quickbarHintServerActiveItemPropertyUsesEventsAfterFirstClientAction = 0
    $quickbarHintServerActiveItemPropertyFullEventsAfterFirstClientAction = 0
    $quickbarHintServerActiveItemPropertyCandidateEventsAfterFirstClientAction = 0
    $quickbarHintServerActiveItemPropertyCandidateUsesEventsAfterFirstClientAction = 0
    $quickbarHintServerActiveItemPropertyCandidateFullEventsAfterFirstClientAction = 0
    $quickbarHintServerActiveItemPropertyCandidateChangedUseCountRowsAfterFirstClientAction = 0
    $quickbarHintServerActiveItemPropertyCandidateFullPropertyRowsAfterFirstClientAction = 0
    $quickbarHintInventoryEventsAfterFirstClientAction = 0
    $quickbarHintClientGuiEventEventsAfterFirstClientAction = 0
    $quickbarHintOtherEventsAfterFirstClientAction = 0
    if ($null -ne $quickbarHintJson) {
        $getQuickbarHintInt64 = {
            param([string]$Name)
            $prop = $quickbarHintJson.PSObject.Properties[$Name]
            if ($null -ne $prop -and $null -ne $prop.Value) {
                return [int64]$prop.Value
            }
            return 0
        }
        $getQuickbarHintInt64Any = {
            param([string[]]$Names)
            foreach ($name in $Names) {
                $prop = $quickbarHintJson.PSObject.Properties[$name]
                if ($null -ne $prop -and $null -ne $prop.Value) {
                    return [int64]$prop.Value
                }
            }
            return 0
        }
        $getQuickbarHintBoolAny = {
            param([string[]]$Names)
            foreach ($name in $Names) {
                $prop = $quickbarHintJson.PSObject.Properties[$name]
                if ($null -ne $prop -and $null -ne $prop.Value) {
                    return [bool]$prop.Value
                }
            }
            return $false
        }
        $getQuickbarHintStringAny = {
            param([string[]]$Names)
            foreach ($name in $Names) {
                $prop = $quickbarHintJson.PSObject.Properties[$name]
                if ($null -ne $prop -and $null -ne $prop.Value) {
                    return [string]$prop.Value
                }
            }
            return ''
        }
        $pendingProp = $quickbarHintJson.PSObject.Properties['pending_item_refresh']
        if ($null -ne $pendingProp) {
            $quickbarHintPending = [bool]$pendingProp.Value
        }
        $candidateProp = $quickbarHintJson.PSObject.Properties['candidate_object_id']
        if ($null -ne $candidateProp -and $null -ne $candidateProp.Value) {
            $quickbarHintCandidateObjectId = [int64]$candidateProp.Value
        }
        $proofProp = $quickbarHintJson.PSObject.Properties['candidate_proof']
        if ($null -ne $proofProp -and $null -ne $proofProp.Value) {
            $quickbarHintCandidateProof = [string]$proofProp.Value
        }
        $sourceProp = $quickbarHintJson.PSObject.Properties['candidate_source']
        if ($null -ne $sourceProp -and $null -ne $sourceProp.Value) {
            $quickbarHintCandidateSource = [string]$sourceProp.Value
        }
        $noHintReasonProp = $quickbarHintJson.PSObject.Properties['no_hint_reason']
        if ($null -ne $noHintReasonProp -and $null -ne $noHintReasonProp.Value) {
            $quickbarHintNoHintReason = [string]$noHintReasonProp.Value
        }
        $matchProp = $quickbarHintJson.PSObject.Properties['first_client_action_matches_candidate']
        if ($null -ne $matchProp -and $null -ne $matchProp.Value) {
            $quickbarHintFirstActionMatchesCandidate = [bool]$matchProp.Value
        }
        $preservedMatchProp = $quickbarHintJson.PSObject.Properties['first_client_action_matches_preserved_active_item']
        if ($null -ne $preservedMatchProp -and $null -ne $preservedMatchProp.Value) {
            $quickbarHintFirstActionMatchesPreservedActiveItem = [bool]$preservedMatchProp.Value
        }
        $firstPreservedActiveItemSlotKnownProp = $quickbarHintJson.PSObject.Properties['first_preserved_active_item_slot_known']
        if ($null -ne $firstPreservedActiveItemSlotKnownProp -and $null -ne $firstPreservedActiveItemSlotKnownProp.Value) {
            $quickbarHintFirstPreservedActiveItemSlotKnown = [bool]$firstPreservedActiveItemSlotKnownProp.Value
        }
        $quickbarHintFirstPreservedActiveItemSlot = & $getQuickbarHintInt64 'first_preserved_active_item_slot'
        $firstPreservedActiveItemFirstPageSlotProp = $quickbarHintJson.PSObject.Properties['first_preserved_active_item_first_page_slot']
        if ($null -ne $firstPreservedActiveItemFirstPageSlotProp -and $null -ne $firstPreservedActiveItemFirstPageSlotProp.Value) {
            $quickbarHintFirstPreservedActiveItemFirstPageSlot = [bool]$firstPreservedActiveItemFirstPageSlotProp.Value
        }
        $firstPreservedActiveItemSlotMatchesRecommendedSetButtonSlotProp = $quickbarHintJson.PSObject.Properties['first_preserved_active_item_slot_matches_recommended_set_button_slot']
        if ($null -ne $firstPreservedActiveItemSlotMatchesRecommendedSetButtonSlotProp -and $null -ne $firstPreservedActiveItemSlotMatchesRecommendedSetButtonSlotProp.Value) {
            $quickbarHintFirstPreservedActiveItemSlotMatchesRecommendedSetButtonSlot = [bool]$firstPreservedActiveItemSlotMatchesRecommendedSetButtonSlotProp.Value
        }
        $quickbarHintStreamProbeItemButtonsRejectedMissingStateClearedDelete = & $getQuickbarHintInt64 'stream_probe_item_buttons_rejected_missing_state_cleared_delete'
        $quickbarHintStreamProbeItemButtonsRejectedMissingStateClearedAreaReset = & $getQuickbarHintInt64 'stream_probe_item_buttons_rejected_missing_state_cleared_area_reset'
        $quickbarHintStreamProbeItemObjectsRejectedMissingStateProven = & $getQuickbarHintInt64 'stream_probe_item_objects_rejected_missing_state_proven'
        $quickbarHintStreamProbeItemObjectsRejectedMissingStateActive = & $getQuickbarHintInt64 'stream_probe_item_objects_rejected_missing_state_active'
        $quickbarHintStreamProbeItemObjectsRejectedMissingStateFeature25First = & $getQuickbarHintInt64 'stream_probe_item_objects_rejected_missing_state_feature25_first'
        $quickbarHintStreamProbeItemObjectsRejectedMissingStateFeature25Second = & $getQuickbarHintInt64 'stream_probe_item_objects_rejected_missing_state_feature25_second'
        $quickbarHintStreamProbeItemObjectsRejectedMissingStateFeature25LegacyTail = & $getQuickbarHintInt64 'stream_probe_item_objects_rejected_missing_state_feature25_legacy_tail'
        $quickbarHintStreamProbeItemObjectsRejectedMissingStateUnknown = & $getQuickbarHintInt64 'stream_probe_item_objects_rejected_missing_state_unknown'
        $quickbarHintStreamProbeItemObjectsRejectedMissingStateClearedDelete = & $getQuickbarHintInt64 'stream_probe_item_objects_rejected_missing_state_cleared_delete'
        $quickbarHintStreamProbeItemObjectsRejectedMissingStateClearedAreaReset = & $getQuickbarHintInt64 'stream_probe_item_objects_rejected_missing_state_cleared_area_reset'
        $quickbarHintStreamProbeItemObjectsPreservedByExplicitSelfMaterialization = & $getQuickbarHintInt64 'stream_probe_item_objects_preserved_by_explicit_self_materialization'
        $quickbarHintStreamProbeItemObjectsPreservedByActiveState = & $getQuickbarHintInt64 'stream_probe_item_objects_preserved_by_active_state'
        $quickbarHintStreamProbeItemObjectsPreservedByFeature25First = & $getQuickbarHintInt64 'stream_probe_item_objects_preserved_by_feature25_first'
        $quickbarHintStreamProbeItemObjectsPreservedByFeature25Second = & $getQuickbarHintInt64 'stream_probe_item_objects_preserved_by_feature25_second'
        $quickbarHintStreamProbeItemObjectsPreservedByFeature25LegacyTail = & $getQuickbarHintInt64 'stream_probe_item_objects_preserved_by_feature25_legacy_tail'
        $quickbarHintInventoryFeature25ReferenceRecords = & $getQuickbarHintInt64 'inventory_feature25_reference_records'
        $quickbarHintInventoryFeature25ItemRefMentions = & $getQuickbarHintInt64 'inventory_feature25_item_ref_mentions'
        $quickbarHintInventoryFeature25MaterializedItemRefMentions = & $getQuickbarHintInt64 'inventory_feature25_materialized_item_ref_mentions'
        $quickbarHintInventoryFeature25DeferredItemRefMentions = & $getQuickbarHintInt64 'inventory_feature25_deferred_item_ref_mentions'
        $quickbarHintCompactItemEmissionReadyObjects = & $getQuickbarHintInt64 'compact_item_emission_ready_objects'
        $quickbarHintCompactItemEmissionDeferredFeature25OnlyObjects = & $getQuickbarHintInt64 'compact_item_emission_deferred_feature25_only_objects'
        $quickbarHintStreamProbeCompactItemEmissionReadyObjects = & $getQuickbarHintInt64 'stream_probe_compact_item_emission_ready_objects'
        $quickbarHintStreamProbeCompactItemEmissionDeferredFeature25OnlyObjects = & $getQuickbarHintInt64 'stream_probe_compact_item_emission_deferred_feature25_only_objects'
        $feature25OutcomeProp = $quickbarHintJson.PSObject.Properties['inventory_feature25_materialization_outcome']
        if ($null -ne $feature25OutcomeProp -and $null -ne $feature25OutcomeProp.Value) {
            $quickbarHintInventoryFeature25MaterializationOutcome = [string]$feature25OutcomeProp.Value
        }
        $feature25HandoffOutcomeProp = $quickbarHintJson.PSObject.Properties['inventory_feature25_handoff_outcome']
        if ($null -ne $feature25HandoffOutcomeProp -and $null -ne $feature25HandoffOutcomeProp.Value) {
            $quickbarHintInventoryFeature25HandoffOutcome = [string]$feature25HandoffOutcomeProp.Value
        }
        $inventoryEquipmentHandoffReadyProp = $quickbarHintJson.PSObject.Properties['inventory_equipment_handoff_ready']
        if ($null -ne $inventoryEquipmentHandoffReadyProp -and $null -ne $inventoryEquipmentHandoffReadyProp.Value) {
            $quickbarHintInventoryEquipmentHandoffReady = [bool]$inventoryEquipmentHandoffReadyProp.Value
        }
        $inventoryEquipmentHandoffOutcomeProp = $quickbarHintJson.PSObject.Properties['inventory_equipment_handoff_outcome']
        if ($null -ne $inventoryEquipmentHandoffOutcomeProp -and $null -ne $inventoryEquipmentHandoffOutcomeProp.Value) {
            $quickbarHintInventoryEquipmentHandoffOutcome = [string]$inventoryEquipmentHandoffOutcomeProp.Value
        }
        $quickbarHintInventoryEquipmentHandoffEvents = & $getQuickbarHintInt64 'inventory_equipment_handoff_events'
        $quickbarHintInventoryEquipmentHandoffReadyEvents = & $getQuickbarHintInt64 'inventory_equipment_handoff_ready_events'
        $quickbarHintInventoryEquipmentHandoffBlockedWithoutReadyStateEvents = & $getQuickbarHintInt64 'inventory_equipment_handoff_blocked_without_ready_state_events'
        $quickbarHintInventoryEquipmentHandoffReadyWithDeferredFeature25Events = & $getQuickbarHintInt64 'inventory_equipment_handoff_ready_with_deferred_feature25_events'
        $quickbarHintInventoryEquipmentHandoffServerInventoryEvents = & $getQuickbarHintInt64 'inventory_equipment_handoff_server_inventory_events'
        $quickbarHintInventoryEquipmentHandoffServerInventoryReadyEvents = & $getQuickbarHintInt64 'inventory_equipment_handoff_server_inventory_ready_events'
        $quickbarHintInventoryEquipmentHandoffServerInventoryBlockedWithoutReadyStateEvents = & $getQuickbarHintInt64 'inventory_equipment_handoff_server_inventory_blocked_without_ready_state_events'
        $quickbarHintInventoryEquipmentHandoffClientGuiInventoryEvents = & $getQuickbarHintInt64 'inventory_equipment_handoff_client_gui_inventory_events'
        $quickbarHintInventoryEquipmentHandoffClientGuiInventoryReadyEvents = & $getQuickbarHintInt64 'inventory_equipment_handoff_client_gui_inventory_ready_events'
        $quickbarHintInventoryEquipmentHandoffClientGuiInventoryBlockedWithoutReadyStateEvents = & $getQuickbarHintInt64 'inventory_equipment_handoff_client_gui_inventory_blocked_without_ready_state_events'
        $lastInventoryEquipmentHandoffKnownProp = $quickbarHintJson.PSObject.Properties['last_inventory_equipment_handoff_known']
        if ($null -ne $lastInventoryEquipmentHandoffKnownProp -and $null -ne $lastInventoryEquipmentHandoffKnownProp.Value) {
            $quickbarHintLastInventoryEquipmentHandoffKnown = [bool]$lastInventoryEquipmentHandoffKnownProp.Value
        }
        $lastInventoryEquipmentHandoffConsumerProp = $quickbarHintJson.PSObject.Properties['last_inventory_equipment_handoff_consumer']
        if ($null -ne $lastInventoryEquipmentHandoffConsumerProp -and $null -ne $lastInventoryEquipmentHandoffConsumerProp.Value) {
            $quickbarHintLastInventoryEquipmentHandoffConsumer = [string]$lastInventoryEquipmentHandoffConsumerProp.Value
        }
        $quickbarHintLastInventoryEquipmentHandoffEventIndex = & $getQuickbarHintInt64 'last_inventory_equipment_handoff_event_index'
        $lastInventoryEquipmentHandoffOutcomeProp = $quickbarHintJson.PSObject.Properties['last_inventory_equipment_handoff_outcome']
        if ($null -ne $lastInventoryEquipmentHandoffOutcomeProp -and $null -ne $lastInventoryEquipmentHandoffOutcomeProp.Value) {
            $quickbarHintLastInventoryEquipmentHandoffOutcome = [string]$lastInventoryEquipmentHandoffOutcomeProp.Value
        }
        $quickbarHintLastInventoryEquipmentHandoffReadyObjects = & $getQuickbarHintInt64 'last_inventory_equipment_handoff_ready_objects'
        $quickbarHintLastInventoryEquipmentHandoffDeferredFeature25OnlyObjects = & $getQuickbarHintInt64 'last_inventory_equipment_handoff_deferred_feature25_only_objects'
        $lastInventoryEquipmentHandoffCandidateKnownProp = $quickbarHintJson.PSObject.Properties['last_inventory_equipment_handoff_candidate_known']
        if ($null -ne $lastInventoryEquipmentHandoffCandidateKnownProp -and $null -ne $lastInventoryEquipmentHandoffCandidateKnownProp.Value) {
            $quickbarHintLastInventoryEquipmentHandoffCandidateKnown = [bool]$lastInventoryEquipmentHandoffCandidateKnownProp.Value
        }
        $quickbarHintLastInventoryEquipmentHandoffCandidateObjectId = & $getQuickbarHintInt64 'last_inventory_equipment_handoff_candidate_object_id'
        $lastInventoryEquipmentHandoffCandidateProofProp = $quickbarHintJson.PSObject.Properties['last_inventory_equipment_handoff_candidate_proof']
        if ($null -ne $lastInventoryEquipmentHandoffCandidateProofProp -and $null -ne $lastInventoryEquipmentHandoffCandidateProofProp.Value) {
            $quickbarHintLastInventoryEquipmentHandoffCandidateProof = [string]$lastInventoryEquipmentHandoffCandidateProofProp.Value
        }
        $lastInventoryEquipmentHandoffCandidateSourceProp = $quickbarHintJson.PSObject.Properties['last_inventory_equipment_handoff_candidate_source']
        if ($null -ne $lastInventoryEquipmentHandoffCandidateSourceProp -and $null -ne $lastInventoryEquipmentHandoffCandidateSourceProp.Value) {
            $quickbarHintLastInventoryEquipmentHandoffCandidateSource = [string]$lastInventoryEquipmentHandoffCandidateSourceProp.Value
        }
        $inventoryEquipmentBridgeHandoffActionProp = $quickbarHintJson.PSObject.Properties['inventory_equipment_bridge_handoff_action']
        if ($null -ne $inventoryEquipmentBridgeHandoffActionProp -and $null -ne $inventoryEquipmentBridgeHandoffActionProp.Value) {
            $quickbarHintInventoryEquipmentBridgeHandoffAction = [string]$inventoryEquipmentBridgeHandoffActionProp.Value
        }
        $inventoryEquipmentBridgeHandoffReadyProp = $quickbarHintJson.PSObject.Properties['inventory_equipment_bridge_handoff_ready']
        if ($null -ne $inventoryEquipmentBridgeHandoffReadyProp -and $null -ne $inventoryEquipmentBridgeHandoffReadyProp.Value) {
            $quickbarHintInventoryEquipmentBridgeHandoffReady = [bool]$inventoryEquipmentBridgeHandoffReadyProp.Value
        }
        $inventoryEquipmentBridgeHandoffConsumerProp = $quickbarHintJson.PSObject.Properties['inventory_equipment_bridge_handoff_consumer']
        if ($null -ne $inventoryEquipmentBridgeHandoffConsumerProp -and $null -ne $inventoryEquipmentBridgeHandoffConsumerProp.Value) {
            $quickbarHintInventoryEquipmentBridgeHandoffConsumer = [string]$inventoryEquipmentBridgeHandoffConsumerProp.Value
        }
        $quickbarHintInventoryEquipmentBridgeHandoffEventIndex = & $getQuickbarHintInt64 'inventory_equipment_bridge_handoff_event_index'
        $inventoryEquipmentBridgeHandoffOutcomeProp = $quickbarHintJson.PSObject.Properties['inventory_equipment_bridge_handoff_outcome']
        if ($null -ne $inventoryEquipmentBridgeHandoffOutcomeProp -and $null -ne $inventoryEquipmentBridgeHandoffOutcomeProp.Value) {
            $quickbarHintInventoryEquipmentBridgeHandoffOutcome = [string]$inventoryEquipmentBridgeHandoffOutcomeProp.Value
        }
        $quickbarHintInventoryEquipmentBridgeHandoffReadyObjects = & $getQuickbarHintInt64 'inventory_equipment_bridge_handoff_ready_objects'
        $quickbarHintInventoryEquipmentBridgeHandoffDeferredFeature25OnlyObjects = & $getQuickbarHintInt64 'inventory_equipment_bridge_handoff_deferred_feature25_only_objects'
        $inventoryEquipmentBridgeHandoffCandidateKnownProp = $quickbarHintJson.PSObject.Properties['inventory_equipment_bridge_handoff_candidate_known']
        if ($null -ne $inventoryEquipmentBridgeHandoffCandidateKnownProp -and $null -ne $inventoryEquipmentBridgeHandoffCandidateKnownProp.Value) {
            $quickbarHintInventoryEquipmentBridgeHandoffCandidateKnown = [bool]$inventoryEquipmentBridgeHandoffCandidateKnownProp.Value
        }
        $quickbarHintInventoryEquipmentBridgeHandoffCandidateObjectId = & $getQuickbarHintInt64 'inventory_equipment_bridge_handoff_candidate_object_id'
        $inventoryEquipmentBridgeHandoffCandidateProofProp = $quickbarHintJson.PSObject.Properties['inventory_equipment_bridge_handoff_candidate_proof']
        if ($null -ne $inventoryEquipmentBridgeHandoffCandidateProofProp -and $null -ne $inventoryEquipmentBridgeHandoffCandidateProofProp.Value) {
            $quickbarHintInventoryEquipmentBridgeHandoffCandidateProof = [string]$inventoryEquipmentBridgeHandoffCandidateProofProp.Value
        }
        $inventoryEquipmentBridgeHandoffCandidateSourceProp = $quickbarHintJson.PSObject.Properties['inventory_equipment_bridge_handoff_candidate_source']
        if ($null -ne $inventoryEquipmentBridgeHandoffCandidateSourceProp -and $null -ne $inventoryEquipmentBridgeHandoffCandidateSourceProp.Value) {
            $quickbarHintInventoryEquipmentBridgeHandoffCandidateSource = [string]$inventoryEquipmentBridgeHandoffCandidateSourceProp.Value
        }
        $quickbarHintInventoryEquipmentBridgeHandoffEmissions = & $getQuickbarHintInt64 'inventory_equipment_bridge_handoff_emissions'
        $inventoryEquipmentBridgeHandoffLastEmittedKnownProp = $quickbarHintJson.PSObject.Properties['inventory_equipment_bridge_handoff_last_emitted_known']
        if ($null -ne $inventoryEquipmentBridgeHandoffLastEmittedKnownProp -and $null -ne $inventoryEquipmentBridgeHandoffLastEmittedKnownProp.Value) {
            $quickbarHintInventoryEquipmentBridgeHandoffLastEmittedKnown = [bool]$inventoryEquipmentBridgeHandoffLastEmittedKnownProp.Value
        }
        $quickbarHintInventoryEquipmentBridgeHandoffLastEmittedIndex = & $getQuickbarHintInt64 'inventory_equipment_bridge_handoff_last_emitted_index'
        $inventoryEquipmentBridgeHandoffLastEmittedConsumerProp = $quickbarHintJson.PSObject.Properties['inventory_equipment_bridge_handoff_last_emitted_consumer']
        if ($null -ne $inventoryEquipmentBridgeHandoffLastEmittedConsumerProp -and $null -ne $inventoryEquipmentBridgeHandoffLastEmittedConsumerProp.Value) {
            $quickbarHintInventoryEquipmentBridgeHandoffLastEmittedConsumer = [string]$inventoryEquipmentBridgeHandoffLastEmittedConsumerProp.Value
        }
        $quickbarHintInventoryEquipmentBridgeHandoffLastEmittedEventIndex = & $getQuickbarHintInt64 'inventory_equipment_bridge_handoff_last_emitted_event_index'
        $quickbarHintInventoryEquipmentBridgeHandoffLastEmittedCandidateObjectId = & $getQuickbarHintInt64 'inventory_equipment_bridge_handoff_last_emitted_candidate_object_id'
        $inventoryEquipmentBridgeHandoffLastEmittedCandidateSourceProp = $quickbarHintJson.PSObject.Properties['inventory_equipment_bridge_handoff_last_emitted_candidate_source']
        if ($null -ne $inventoryEquipmentBridgeHandoffLastEmittedCandidateSourceProp -and $null -ne $inventoryEquipmentBridgeHandoffLastEmittedCandidateSourceProp.Value) {
            $quickbarHintInventoryEquipmentBridgeHandoffLastEmittedCandidateSource = [string]$inventoryEquipmentBridgeHandoffLastEmittedCandidateSourceProp.Value
        }
        $quickbarHintInventoryEquipmentBridgeHandoffStateUpdates = & $getQuickbarHintInt64 'inventory_equipment_bridge_handoff_state_updates'
        $inventoryEquipmentBridgeHandoffLastStateUpdateKnownProp = $quickbarHintJson.PSObject.Properties['inventory_equipment_bridge_handoff_last_state_update_known']
        if ($null -ne $inventoryEquipmentBridgeHandoffLastStateUpdateKnownProp -and $null -ne $inventoryEquipmentBridgeHandoffLastStateUpdateKnownProp.Value) {
            $quickbarHintInventoryEquipmentBridgeHandoffLastStateUpdateKnown = [bool]$inventoryEquipmentBridgeHandoffLastStateUpdateKnownProp.Value
        }
        $quickbarHintInventoryEquipmentBridgeHandoffLastStateUpdateIndex = & $getQuickbarHintInt64 'inventory_equipment_bridge_handoff_last_state_update_index'
        $quickbarHintInventoryEquipmentBridgeHandoffLastStateUpdateEmissionIndex = & $getQuickbarHintInt64 'inventory_equipment_bridge_handoff_last_state_update_emission_index'
        $inventoryEquipmentBridgeHandoffLastStateUpdateConsumerProp = $quickbarHintJson.PSObject.Properties['inventory_equipment_bridge_handoff_last_state_update_consumer']
        if ($null -ne $inventoryEquipmentBridgeHandoffLastStateUpdateConsumerProp -and $null -ne $inventoryEquipmentBridgeHandoffLastStateUpdateConsumerProp.Value) {
            $quickbarHintInventoryEquipmentBridgeHandoffLastStateUpdateConsumer = [string]$inventoryEquipmentBridgeHandoffLastStateUpdateConsumerProp.Value
        }
        $quickbarHintInventoryEquipmentBridgeHandoffLastStateUpdateEventIndex = & $getQuickbarHintInt64 'inventory_equipment_bridge_handoff_last_state_update_event_index'
        $quickbarHintInventoryEquipmentBridgeHandoffLastStateUpdateCandidateObjectId = & $getQuickbarHintInt64 'inventory_equipment_bridge_handoff_last_state_update_candidate_object_id'
        $inventoryEquipmentBridgeHandoffLastStateUpdateCandidateProofProp = $quickbarHintJson.PSObject.Properties['inventory_equipment_bridge_handoff_last_state_update_candidate_proof']
        if ($null -ne $inventoryEquipmentBridgeHandoffLastStateUpdateCandidateProofProp -and $null -ne $inventoryEquipmentBridgeHandoffLastStateUpdateCandidateProofProp.Value) {
            $quickbarHintInventoryEquipmentBridgeHandoffLastStateUpdateCandidateProof = [string]$inventoryEquipmentBridgeHandoffLastStateUpdateCandidateProofProp.Value
        }
        $inventoryEquipmentBridgeHandoffLastStateUpdateCandidateSourceProp = $quickbarHintJson.PSObject.Properties['inventory_equipment_bridge_handoff_last_state_update_candidate_source']
        if ($null -ne $inventoryEquipmentBridgeHandoffLastStateUpdateCandidateSourceProp -and $null -ne $inventoryEquipmentBridgeHandoffLastStateUpdateCandidateSourceProp.Value) {
            $quickbarHintInventoryEquipmentBridgeHandoffLastStateUpdateCandidateSource = [string]$inventoryEquipmentBridgeHandoffLastStateUpdateCandidateSourceProp.Value
        }
        $quickbarHintInventoryEquipmentBridgeHandoffLastStateUpdateReadyObjects = & $getQuickbarHintInt64 'inventory_equipment_bridge_handoff_last_state_update_ready_objects'
        $quickbarHintInventoryEquipmentBridgeHandoffLastStateUpdateDeferredFeature25OnlyObjects = & $getQuickbarHintInt64 'inventory_equipment_bridge_handoff_last_state_update_deferred_feature25_only_objects'
        $quickbarHintInventoryEquipmentBridgeOutputQueuedPackets = & $getQuickbarHintInt64 'inventory_equipment_bridge_output_queued_packets'
        $quickbarHintInventoryEquipmentBridgeOutputDeferredClientGuiUpdates = & $getQuickbarHintInt64 'inventory_equipment_bridge_output_deferred_client_gui_updates'
        $quickbarHintInventoryEquipmentBridgeOutputDeferredMissingClaimUpdates = & $getQuickbarHintInt64 'inventory_equipment_bridge_output_deferred_missing_claim_updates'
        $quickbarHintInventoryEquipmentBridgeOutputBlockedCandidateMismatchUpdates = & $getQuickbarHintInt64 'inventory_equipment_bridge_output_blocked_candidate_mismatch_updates'
        $inventoryEquipmentBridgeOutputStatusProp = $quickbarHintJson.PSObject.Properties['inventory_equipment_bridge_output_status']
        if ($null -ne $inventoryEquipmentBridgeOutputStatusProp -and $null -ne $inventoryEquipmentBridgeOutputStatusProp.Value) {
            $quickbarHintInventoryEquipmentBridgeOutputStatus = [string]$inventoryEquipmentBridgeOutputStatusProp.Value
        }
        $inventoryEquipmentBridgeOutputRequiresClientGuiWriterProp = $quickbarHintJson.PSObject.Properties['inventory_equipment_bridge_output_requires_client_gui_writer']
        if ($null -ne $inventoryEquipmentBridgeOutputRequiresClientGuiWriterProp -and $null -ne $inventoryEquipmentBridgeOutputRequiresClientGuiWriterProp.Value) {
            $quickbarHintInventoryEquipmentBridgeOutputRequiresClientGuiWriter = [bool]$inventoryEquipmentBridgeOutputRequiresClientGuiWriterProp.Value
        }
        $quickbarHintInventoryEquipmentBridgeOutputLastDecisionUpdateIndex = & $getQuickbarHintInt64 'inventory_equipment_bridge_output_last_decision_update_index'
        $inventoryEquipmentBridgeOutputLastDecisionKnownProp = $quickbarHintJson.PSObject.Properties['inventory_equipment_bridge_output_last_decision_known']
        if ($null -ne $inventoryEquipmentBridgeOutputLastDecisionKnownProp -and $null -ne $inventoryEquipmentBridgeOutputLastDecisionKnownProp.Value) {
            $quickbarHintInventoryEquipmentBridgeOutputLastDecisionKnown = [bool]$inventoryEquipmentBridgeOutputLastDecisionKnownProp.Value
        }
        $inventoryEquipmentBridgeOutputLastDecisionReasonProp = $quickbarHintJson.PSObject.Properties['inventory_equipment_bridge_output_last_decision_reason']
        if ($null -ne $inventoryEquipmentBridgeOutputLastDecisionReasonProp -and $null -ne $inventoryEquipmentBridgeOutputLastDecisionReasonProp.Value) {
            $quickbarHintInventoryEquipmentBridgeOutputLastDecisionReason = [string]$inventoryEquipmentBridgeOutputLastDecisionReasonProp.Value
        }
        $inventoryEquipmentBridgeOutputLastDecisionConsumerProp = $quickbarHintJson.PSObject.Properties['inventory_equipment_bridge_output_last_decision_consumer']
        if ($null -ne $inventoryEquipmentBridgeOutputLastDecisionConsumerProp -and $null -ne $inventoryEquipmentBridgeOutputLastDecisionConsumerProp.Value) {
            $quickbarHintInventoryEquipmentBridgeOutputLastDecisionConsumer = [string]$inventoryEquipmentBridgeOutputLastDecisionConsumerProp.Value
        }
        $quickbarHintInventoryEquipmentBridgeOutputLastDecisionEmissionIndex = & $getQuickbarHintInt64 'inventory_equipment_bridge_output_last_decision_emission_index'
        $quickbarHintInventoryEquipmentBridgeOutputLastDecisionEventIndex = & $getQuickbarHintInt64 'inventory_equipment_bridge_output_last_decision_event_index'
        $quickbarHintInventoryEquipmentBridgeOutputLastDecisionCandidateObjectId = & $getQuickbarHintInt64 'inventory_equipment_bridge_output_last_decision_candidate_object_id'
        $inventoryEquipmentBridgeOutputLastDecisionCandidateProofProp = $quickbarHintJson.PSObject.Properties['inventory_equipment_bridge_output_last_decision_candidate_proof']
        if ($null -ne $inventoryEquipmentBridgeOutputLastDecisionCandidateProofProp -and $null -ne $inventoryEquipmentBridgeOutputLastDecisionCandidateProofProp.Value) {
            $quickbarHintInventoryEquipmentBridgeOutputLastDecisionCandidateProof = [string]$inventoryEquipmentBridgeOutputLastDecisionCandidateProofProp.Value
        }
        $inventoryEquipmentBridgeOutputLastDecisionCandidateSourceProp = $quickbarHintJson.PSObject.Properties['inventory_equipment_bridge_output_last_decision_candidate_source']
        if ($null -ne $inventoryEquipmentBridgeOutputLastDecisionCandidateSourceProp -and $null -ne $inventoryEquipmentBridgeOutputLastDecisionCandidateSourceProp.Value) {
            $quickbarHintInventoryEquipmentBridgeOutputLastDecisionCandidateSource = [string]$inventoryEquipmentBridgeOutputLastDecisionCandidateSourceProp.Value
        }
        $inventoryEquipmentBridgeOutputLastDecisionCandidateObjectStatusProp = $quickbarHintJson.PSObject.Properties['inventory_equipment_bridge_output_last_decision_candidate_object_status']
        if ($null -ne $inventoryEquipmentBridgeOutputLastDecisionCandidateObjectStatusProp -and $null -ne $inventoryEquipmentBridgeOutputLastDecisionCandidateObjectStatusProp.Value) {
            $quickbarHintInventoryEquipmentBridgeOutputLastDecisionCandidateObjectStatus = [string]$inventoryEquipmentBridgeOutputLastDecisionCandidateObjectStatusProp.Value
        }
        $inventoryEquipmentBridgeOutputLastDecisionCandidateObjectStatusProofProp = $quickbarHintJson.PSObject.Properties['inventory_equipment_bridge_output_last_decision_candidate_object_status_proof']
        if ($null -ne $inventoryEquipmentBridgeOutputLastDecisionCandidateObjectStatusProofProp -and $null -ne $inventoryEquipmentBridgeOutputLastDecisionCandidateObjectStatusProofProp.Value) {
            $quickbarHintInventoryEquipmentBridgeOutputLastDecisionCandidateObjectStatusProof = [string]$inventoryEquipmentBridgeOutputLastDecisionCandidateObjectStatusProofProp.Value
        }
        $quickbarHintInventoryEquipmentBridgeOutputLastDecisionReadyObjects = & $getQuickbarHintInt64 'inventory_equipment_bridge_output_last_decision_ready_objects'
        $quickbarHintInventoryEquipmentBridgeOutputLastDecisionDeferredFeature25OnlyObjects = & $getQuickbarHintInt64 'inventory_equipment_bridge_output_last_decision_deferred_feature25_only_objects'
        $inventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimKnownProp = $quickbarHintJson.PSObject.Properties['inventory_equipment_bridge_output_last_decision_server_inventory_claim_known']
        if ($null -ne $inventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimKnownProp -and $null -ne $inventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimKnownProp.Value) {
            $quickbarHintInventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimKnown = [bool]$inventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimKnownProp.Value
        }
        $quickbarHintInventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimMinor = & $getQuickbarHintInt64 'inventory_equipment_bridge_output_last_decision_server_inventory_claim_minor'
        $quickbarHintInventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimObjectId = & $getQuickbarHintInt64 'inventory_equipment_bridge_output_last_decision_server_inventory_claim_object_id'
        $inventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimObjectStatusProp = $quickbarHintJson.PSObject.Properties['inventory_equipment_bridge_output_last_decision_server_inventory_claim_object_status']
        if ($null -ne $inventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimObjectStatusProp -and $null -ne $inventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimObjectStatusProp.Value) {
            $quickbarHintInventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimObjectStatus = [string]$inventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimObjectStatusProp.Value
        }
        $inventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimObjectStatusProofProp = $quickbarHintJson.PSObject.Properties['inventory_equipment_bridge_output_last_decision_server_inventory_claim_object_status_proof']
        if ($null -ne $inventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimObjectStatusProofProp -and $null -ne $inventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimObjectStatusProofProp.Value) {
            $quickbarHintInventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimObjectStatusProof = [string]$inventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimObjectStatusProofProp.Value
        }
        $inventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimClosestProvenItemKnownProp = $quickbarHintJson.PSObject.Properties['inventory_equipment_bridge_output_last_decision_server_inventory_claim_closest_proven_item_known']
        if ($null -ne $inventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimClosestProvenItemKnownProp -and $null -ne $inventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimClosestProvenItemKnownProp.Value) {
            $quickbarHintInventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimClosestProvenItemKnown = [bool]$inventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimClosestProvenItemKnownProp.Value
        }
        $quickbarHintInventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimClosestProvenItemObjectId = & $getQuickbarHintInt64 'inventory_equipment_bridge_output_last_decision_server_inventory_claim_closest_proven_item_object_id'
        $quickbarHintInventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimClosestProvenItemDistance = & $getQuickbarHintInt64 'inventory_equipment_bridge_output_last_decision_server_inventory_claim_closest_proven_item_distance'
        $inventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimLowerProvenItemKnownProp = $quickbarHintJson.PSObject.Properties['inventory_equipment_bridge_output_last_decision_server_inventory_claim_lower_proven_item_known']
        if ($null -ne $inventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimLowerProvenItemKnownProp -and $null -ne $inventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimLowerProvenItemKnownProp.Value) {
            $quickbarHintInventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimLowerProvenItemKnown = [bool]$inventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimLowerProvenItemKnownProp.Value
        }
        $quickbarHintInventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimLowerProvenItemObjectId = & $getQuickbarHintInt64 'inventory_equipment_bridge_output_last_decision_server_inventory_claim_lower_proven_item_object_id'
        $quickbarHintInventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimLowerProvenItemDistance = & $getQuickbarHintInt64 'inventory_equipment_bridge_output_last_decision_server_inventory_claim_lower_proven_item_distance'
        $inventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimHigherProvenItemKnownProp = $quickbarHintJson.PSObject.Properties['inventory_equipment_bridge_output_last_decision_server_inventory_claim_higher_proven_item_known']
        if ($null -ne $inventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimHigherProvenItemKnownProp -and $null -ne $inventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimHigherProvenItemKnownProp.Value) {
            $quickbarHintInventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimHigherProvenItemKnown = [bool]$inventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimHigherProvenItemKnownProp.Value
        }
        $quickbarHintInventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimHigherProvenItemObjectId = & $getQuickbarHintInt64 'inventory_equipment_bridge_output_last_decision_server_inventory_claim_higher_proven_item_object_id'
        $quickbarHintInventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimHigherProvenItemDistance = & $getQuickbarHintInt64 'inventory_equipment_bridge_output_last_decision_server_inventory_claim_higher_proven_item_distance'
        $inventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimResultProp = $quickbarHintJson.PSObject.Properties['inventory_equipment_bridge_output_last_decision_server_inventory_claim_result']
        if ($null -ne $inventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimResultProp -and $null -ne $inventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimResultProp.Value) {
            $quickbarHintInventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimResult = [bool]$inventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimResultProp.Value
        }
        $quickbarHintInventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimEquipSlot = & $getQuickbarHintInt64 'inventory_equipment_bridge_output_last_decision_server_inventory_claim_equip_slot'
        $inventoryEquipmentBridgeOutputLastDecisionClientGuiInventoryClaimKnownProp = $quickbarHintJson.PSObject.Properties['inventory_equipment_bridge_output_last_decision_client_gui_inventory_claim_known']
        if ($null -ne $inventoryEquipmentBridgeOutputLastDecisionClientGuiInventoryClaimKnownProp -and $null -ne $inventoryEquipmentBridgeOutputLastDecisionClientGuiInventoryClaimKnownProp.Value) {
            $quickbarHintInventoryEquipmentBridgeOutputLastDecisionClientGuiInventoryClaimKnown = [bool]$inventoryEquipmentBridgeOutputLastDecisionClientGuiInventoryClaimKnownProp.Value
        }
        $inventoryEquipmentBridgeOutputLastDecisionClientGuiInventoryClaimKindProp = $quickbarHintJson.PSObject.Properties['inventory_equipment_bridge_output_last_decision_client_gui_inventory_claim_kind']
        if ($null -ne $inventoryEquipmentBridgeOutputLastDecisionClientGuiInventoryClaimKindProp -and $null -ne $inventoryEquipmentBridgeOutputLastDecisionClientGuiInventoryClaimKindProp.Value) {
            $quickbarHintInventoryEquipmentBridgeOutputLastDecisionClientGuiInventoryClaimKind = [string]$inventoryEquipmentBridgeOutputLastDecisionClientGuiInventoryClaimKindProp.Value
        }
        $quickbarHintInventoryEquipmentBridgeOutputLastDecisionClientGuiInventoryClaimObjectId = & $getQuickbarHintInt64 'inventory_equipment_bridge_output_last_decision_client_gui_inventory_claim_object_id'
        $quickbarHintInventoryEquipmentBridgeOutputLastDecisionClientGuiInventoryClaimPanel = & $getQuickbarHintInt64 'inventory_equipment_bridge_output_last_decision_client_gui_inventory_claim_panel'
        $inventoryEquipmentBridgeOutputLastDecisionClientGuiInventoryClaimPlayerInventoryGuiProp = $quickbarHintJson.PSObject.Properties['inventory_equipment_bridge_output_last_decision_client_gui_inventory_claim_player_inventory_gui']
        if ($null -ne $inventoryEquipmentBridgeOutputLastDecisionClientGuiInventoryClaimPlayerInventoryGuiProp -and $null -ne $inventoryEquipmentBridgeOutputLastDecisionClientGuiInventoryClaimPlayerInventoryGuiProp.Value) {
            $quickbarHintInventoryEquipmentBridgeOutputLastDecisionClientGuiInventoryClaimPlayerInventoryGui = [bool]$inventoryEquipmentBridgeOutputLastDecisionClientGuiInventoryClaimPlayerInventoryGuiProp.Value
        }
        $inventoryEquipmentBridgeOutputLastDecisionClientGuiInventoryClaimRewrittenSelfObjectIdProp = $quickbarHintJson.PSObject.Properties['inventory_equipment_bridge_output_last_decision_client_gui_inventory_claim_rewritten_self_object_id']
        if ($null -ne $inventoryEquipmentBridgeOutputLastDecisionClientGuiInventoryClaimRewrittenSelfObjectIdProp -and $null -ne $inventoryEquipmentBridgeOutputLastDecisionClientGuiInventoryClaimRewrittenSelfObjectIdProp.Value) {
            $quickbarHintInventoryEquipmentBridgeOutputLastDecisionClientGuiInventoryClaimRewrittenSelfObjectId = [bool]$inventoryEquipmentBridgeOutputLastDecisionClientGuiInventoryClaimRewrittenSelfObjectIdProp.Value
        }
        $inventoryEquipmentBridgeOutputClientGuiWriterPlanActionProp = $quickbarHintJson.PSObject.Properties['inventory_equipment_bridge_output_client_gui_writer_plan_action']
        if ($null -ne $inventoryEquipmentBridgeOutputClientGuiWriterPlanActionProp -and $null -ne $inventoryEquipmentBridgeOutputClientGuiWriterPlanActionProp.Value) {
            $quickbarHintInventoryEquipmentBridgeOutputClientGuiWriterPlanAction = [string]$inventoryEquipmentBridgeOutputClientGuiWriterPlanActionProp.Value
        }
        $inventoryEquipmentBridgeOutputClientGuiWriterPlanEmissionEnabledProp = $quickbarHintJson.PSObject.Properties['inventory_equipment_bridge_output_client_gui_writer_plan_emission_enabled']
        if ($null -ne $inventoryEquipmentBridgeOutputClientGuiWriterPlanEmissionEnabledProp -and $null -ne $inventoryEquipmentBridgeOutputClientGuiWriterPlanEmissionEnabledProp.Value) {
            $quickbarHintInventoryEquipmentBridgeOutputClientGuiWriterPlanEmissionEnabled = [bool]$inventoryEquipmentBridgeOutputClientGuiWriterPlanEmissionEnabledProp.Value
        }
        $inventoryEquipmentBridgeOutputClientGuiWriterPlanBlockedReasonProp = $quickbarHintJson.PSObject.Properties['inventory_equipment_bridge_output_client_gui_writer_plan_blocked_reason']
        if ($null -ne $inventoryEquipmentBridgeOutputClientGuiWriterPlanBlockedReasonProp -and $null -ne $inventoryEquipmentBridgeOutputClientGuiWriterPlanBlockedReasonProp.Value) {
            $quickbarHintInventoryEquipmentBridgeOutputClientGuiWriterPlanBlockedReason = [string]$inventoryEquipmentBridgeOutputClientGuiWriterPlanBlockedReasonProp.Value
        }
        $inventoryEquipmentBridgeOutputClientGuiWriterPlanPayloadAvailableProp = $quickbarHintJson.PSObject.Properties['inventory_equipment_bridge_output_client_gui_writer_plan_payload_available']
        if ($null -ne $inventoryEquipmentBridgeOutputClientGuiWriterPlanPayloadAvailableProp -and $null -ne $inventoryEquipmentBridgeOutputClientGuiWriterPlanPayloadAvailableProp.Value) {
            $quickbarHintInventoryEquipmentBridgeOutputClientGuiWriterPlanPayloadAvailable = [bool]$inventoryEquipmentBridgeOutputClientGuiWriterPlanPayloadAvailableProp.Value
        }
        $inventoryEquipmentBridgeOutputClientGuiWriterPlanPayloadKindProp = $quickbarHintJson.PSObject.Properties['inventory_equipment_bridge_output_client_gui_writer_plan_payload_kind']
        if ($null -ne $inventoryEquipmentBridgeOutputClientGuiWriterPlanPayloadKindProp -and $null -ne $inventoryEquipmentBridgeOutputClientGuiWriterPlanPayloadKindProp.Value) {
            $quickbarHintInventoryEquipmentBridgeOutputClientGuiWriterPlanPayloadKind = [string]$inventoryEquipmentBridgeOutputClientGuiWriterPlanPayloadKindProp.Value
        }
        $inventoryEquipmentBridgeOutputClientGuiWriterPlanPayloadHexProp = $quickbarHintJson.PSObject.Properties['inventory_equipment_bridge_output_client_gui_writer_plan_payload_hex']
        if ($null -ne $inventoryEquipmentBridgeOutputClientGuiWriterPlanPayloadHexProp -and $null -ne $inventoryEquipmentBridgeOutputClientGuiWriterPlanPayloadHexProp.Value) {
            $quickbarHintInventoryEquipmentBridgeOutputClientGuiWriterPlanPayloadHex = [string]$inventoryEquipmentBridgeOutputClientGuiWriterPlanPayloadHexProp.Value
        }
        $quickbarHintInventoryEquipmentBridgeOutputClientGuiWriterPlanStatusObjectId = & $getQuickbarHintInt64 'inventory_equipment_bridge_output_client_gui_writer_plan_status_object_id'
        $inventoryEquipmentBridgeOutputClientGuiWriterPlanStatusObjectIsCurrentPlayerProp = $quickbarHintJson.PSObject.Properties['inventory_equipment_bridge_output_client_gui_writer_plan_status_object_is_current_player']
        if ($null -ne $inventoryEquipmentBridgeOutputClientGuiWriterPlanStatusObjectIsCurrentPlayerProp -and $null -ne $inventoryEquipmentBridgeOutputClientGuiWriterPlanStatusObjectIsCurrentPlayerProp.Value) {
            $quickbarHintInventoryEquipmentBridgeOutputClientGuiWriterPlanStatusObjectIsCurrentPlayer = [bool]$inventoryEquipmentBridgeOutputClientGuiWriterPlanStatusObjectIsCurrentPlayerProp.Value
        }
        $quickbarHintInventoryEquipmentBridgeOutputClientGuiWriterPlanSelectPanel = & $getQuickbarHintInt64 'inventory_equipment_bridge_output_client_gui_writer_plan_select_panel'
        $inventoryEquipmentBridgeOutputClientGuiWriterPlanPlayerInventoryGuiProp = $quickbarHintJson.PSObject.Properties['inventory_equipment_bridge_output_client_gui_writer_plan_player_inventory_gui']
        if ($null -ne $inventoryEquipmentBridgeOutputClientGuiWriterPlanPlayerInventoryGuiProp -and $null -ne $inventoryEquipmentBridgeOutputClientGuiWriterPlanPlayerInventoryGuiProp.Value) {
            $quickbarHintInventoryEquipmentBridgeOutputClientGuiWriterPlanPlayerInventoryGui = [bool]$inventoryEquipmentBridgeOutputClientGuiWriterPlanPlayerInventoryGuiProp.Value
        }
        $quickbarHintInventoryEquipmentBridgeOutputLastDeferredClientGuiUpdateIndex = & $getQuickbarHintInt64 'inventory_equipment_bridge_output_last_deferred_client_gui_update_index'
        $quickbarHintInventoryEquipmentBridgeOutputLastDeferredMissingClaimUpdateIndex = & $getQuickbarHintInt64 'inventory_equipment_bridge_output_last_deferred_missing_claim_update_index'
        $quickbarHintInventoryEquipmentBridgeOutputLastBlockedCandidateMismatchUpdateIndex = & $getQuickbarHintInt64 'inventory_equipment_bridge_output_last_blocked_candidate_mismatch_update_index'
        $inventoryEquipmentBridgeOutputLastQueuedKnownProp = $quickbarHintJson.PSObject.Properties['inventory_equipment_bridge_output_last_queued_known']
        if ($null -ne $inventoryEquipmentBridgeOutputLastQueuedKnownProp -and $null -ne $inventoryEquipmentBridgeOutputLastQueuedKnownProp.Value) {
            $quickbarHintInventoryEquipmentBridgeOutputLastQueuedKnown = [bool]$inventoryEquipmentBridgeOutputLastQueuedKnownProp.Value
        }
        $quickbarHintInventoryEquipmentBridgeOutputLastQueuedUpdateIndex = & $getQuickbarHintInt64 'inventory_equipment_bridge_output_last_queued_update_index'
        $quickbarHintInventoryEquipmentBridgeOutputLastQueuedEmissionIndex = & $getQuickbarHintInt64 'inventory_equipment_bridge_output_last_queued_emission_index'
        $quickbarHintInventoryEquipmentBridgeOutputLastQueuedEventIndex = & $getQuickbarHintInt64 'inventory_equipment_bridge_output_last_queued_event_index'
        $quickbarHintInventoryEquipmentBridgeOutputLastQueuedMinor = & $getQuickbarHintInt64 'inventory_equipment_bridge_output_last_queued_minor'
        $quickbarHintInventoryEquipmentBridgeOutputLastQueuedObjectId = & $getQuickbarHintInt64 'inventory_equipment_bridge_output_last_queued_object_id'
        $inventoryEquipmentBridgeOutputLastQueuedResultProp = $quickbarHintJson.PSObject.Properties['inventory_equipment_bridge_output_last_queued_result']
        if ($null -ne $inventoryEquipmentBridgeOutputLastQueuedResultProp -and $null -ne $inventoryEquipmentBridgeOutputLastQueuedResultProp.Value) {
            $quickbarHintInventoryEquipmentBridgeOutputLastQueuedResult = [bool]$inventoryEquipmentBridgeOutputLastQueuedResultProp.Value
        }
        $quickbarHintInventoryEquipmentBridgeOutputLastQueuedEquipSlot = & $getQuickbarHintInt64 'inventory_equipment_bridge_output_last_queued_equip_slot'
        $quickbarHintInventoryEquipmentBridgeOutputLastQueuedTriggerSequence = & $getQuickbarHintInt64 'inventory_equipment_bridge_output_last_queued_trigger_sequence'
        $quickbarHintInventoryEquipmentBridgeOutputLastQueuedSyntheticSequence = & $getQuickbarHintInt64 'inventory_equipment_bridge_output_last_queued_synthetic_sequence'
        $quickbarHintInventoryEquipmentBridgeOutputQueuedClientGuiStatusPackets = & $getQuickbarHintInt64 'inventory_equipment_bridge_output_queued_client_gui_status_packets'
        $inventoryEquipmentBridgeOutputLastQueuedClientGuiStatusKnownProp = $quickbarHintJson.PSObject.Properties['inventory_equipment_bridge_output_last_queued_client_gui_status_known']
        if ($null -ne $inventoryEquipmentBridgeOutputLastQueuedClientGuiStatusKnownProp -and $null -ne $inventoryEquipmentBridgeOutputLastQueuedClientGuiStatusKnownProp.Value) {
            $quickbarHintInventoryEquipmentBridgeOutputLastQueuedClientGuiStatusKnown = [bool]$inventoryEquipmentBridgeOutputLastQueuedClientGuiStatusKnownProp.Value
        }
        $quickbarHintInventoryEquipmentBridgeOutputLastQueuedClientGuiStatusUpdateIndex = & $getQuickbarHintInt64 'inventory_equipment_bridge_output_last_queued_client_gui_status_update_index'
        $quickbarHintInventoryEquipmentBridgeOutputLastQueuedClientGuiStatusEmissionIndex = & $getQuickbarHintInt64 'inventory_equipment_bridge_output_last_queued_client_gui_status_emission_index'
        $quickbarHintInventoryEquipmentBridgeOutputLastQueuedClientGuiStatusEventIndex = & $getQuickbarHintInt64 'inventory_equipment_bridge_output_last_queued_client_gui_status_event_index'
        $quickbarHintInventoryEquipmentBridgeOutputLastQueuedClientGuiStatusObjectId = & $getQuickbarHintInt64 'inventory_equipment_bridge_output_last_queued_client_gui_status_object_id'
        $inventoryEquipmentBridgeOutputLastQueuedClientGuiStatusPlayerInventoryGuiProp = $quickbarHintJson.PSObject.Properties['inventory_equipment_bridge_output_last_queued_client_gui_status_player_inventory_gui']
        if ($null -ne $inventoryEquipmentBridgeOutputLastQueuedClientGuiStatusPlayerInventoryGuiProp -and $null -ne $inventoryEquipmentBridgeOutputLastQueuedClientGuiStatusPlayerInventoryGuiProp.Value) {
            $quickbarHintInventoryEquipmentBridgeOutputLastQueuedClientGuiStatusPlayerInventoryGui = [bool]$inventoryEquipmentBridgeOutputLastQueuedClientGuiStatusPlayerInventoryGuiProp.Value
        }
        $inventoryEquipmentBridgeOutputLastQueuedClientGuiStatusPayloadHexProp = $quickbarHintJson.PSObject.Properties['inventory_equipment_bridge_output_last_queued_client_gui_status_payload_hex']
        if ($null -ne $inventoryEquipmentBridgeOutputLastQueuedClientGuiStatusPayloadHexProp -and $null -ne $inventoryEquipmentBridgeOutputLastQueuedClientGuiStatusPayloadHexProp.Value) {
            $quickbarHintInventoryEquipmentBridgeOutputLastQueuedClientGuiStatusPayloadHex = [string]$inventoryEquipmentBridgeOutputLastQueuedClientGuiStatusPayloadHexProp.Value
        }
        $quickbarHintInventoryEquipmentBridgeOutputLastQueuedClientGuiStatusTriggerClientSequence = & $getQuickbarHintInt64 'inventory_equipment_bridge_output_last_queued_client_gui_status_trigger_client_sequence'
        $quickbarHintInventoryEquipmentBridgeOutputLastQueuedClientGuiStatusSyntheticSequence = & $getQuickbarHintInt64 'inventory_equipment_bridge_output_last_queued_client_gui_status_synthetic_sequence'
        $quickbarHintInventoryEquipmentBridgeOutputLastQueuedClientGuiStatusAckSequence = & $getQuickbarHintInt64 'inventory_equipment_bridge_output_last_queued_client_gui_status_ack_sequence'
        $inventoryEquipmentBridgeOutputLastQueuedClientGuiStatusCandidateKnownProp = $quickbarHintJson.PSObject.Properties['inventory_equipment_bridge_output_last_queued_client_gui_status_candidate_known']
        if ($null -ne $inventoryEquipmentBridgeOutputLastQueuedClientGuiStatusCandidateKnownProp -and $null -ne $inventoryEquipmentBridgeOutputLastQueuedClientGuiStatusCandidateKnownProp.Value) {
            $quickbarHintInventoryEquipmentBridgeOutputLastQueuedClientGuiStatusCandidateKnown = [bool]$inventoryEquipmentBridgeOutputLastQueuedClientGuiStatusCandidateKnownProp.Value
        }
        $quickbarHintInventoryEquipmentBridgeOutputLastQueuedClientGuiStatusCandidateObjectId = & $getQuickbarHintInt64 'inventory_equipment_bridge_output_last_queued_client_gui_status_candidate_object_id'
        $inventoryEquipmentBridgeOutputLastQueuedClientGuiStatusCandidateProofProp = $quickbarHintJson.PSObject.Properties['inventory_equipment_bridge_output_last_queued_client_gui_status_candidate_proof']
        if ($null -ne $inventoryEquipmentBridgeOutputLastQueuedClientGuiStatusCandidateProofProp -and $null -ne $inventoryEquipmentBridgeOutputLastQueuedClientGuiStatusCandidateProofProp.Value) {
            $quickbarHintInventoryEquipmentBridgeOutputLastQueuedClientGuiStatusCandidateProof = [string]$inventoryEquipmentBridgeOutputLastQueuedClientGuiStatusCandidateProofProp.Value
        }
        $inventoryEquipmentBridgeOutputLastQueuedClientGuiStatusCandidateSourceProp = $quickbarHintJson.PSObject.Properties['inventory_equipment_bridge_output_last_queued_client_gui_status_candidate_source']
        if ($null -ne $inventoryEquipmentBridgeOutputLastQueuedClientGuiStatusCandidateSourceProp -and $null -ne $inventoryEquipmentBridgeOutputLastQueuedClientGuiStatusCandidateSourceProp.Value) {
            $quickbarHintInventoryEquipmentBridgeOutputLastQueuedClientGuiStatusCandidateSource = [string]$inventoryEquipmentBridgeOutputLastQueuedClientGuiStatusCandidateSourceProp.Value
        }
        $quickbarHintInventoryEquipmentBridgeOutputLastQueuedClientGuiStatusReadyObjects = & $getQuickbarHintInt64 'inventory_equipment_bridge_output_last_queued_client_gui_status_ready_objects'
        $quickbarHintInventoryEquipmentBridgeOutputLastQueuedClientGuiStatusDeferredFeature25OnlyObjects = & $getQuickbarHintInt64 'inventory_equipment_bridge_output_last_queued_client_gui_status_deferred_feature25_only_objects'
        $quickbarHintInventoryEquipmentBridgeOutputClientGuiStatusResponseLiveObjectPackets = & $getQuickbarHintInt64 'inventory_equipment_bridge_output_client_gui_status_response_live_object_packets'
        $quickbarHintInventoryEquipmentBridgeOutputClientGuiStatusResponseLiveGuiRecordPackets = & $getQuickbarHintInt64 'inventory_equipment_bridge_output_client_gui_status_response_live_gui_record_packets'
        $quickbarHintInventoryEquipmentBridgeOutputClientGuiStatusResponseMaterializedItemPackets = & $getQuickbarHintInt64 'inventory_equipment_bridge_output_client_gui_status_response_materialized_item_packets'
        $inventoryEquipmentBridgeOutputLastClientGuiStatusResponseKnownProp = $quickbarHintJson.PSObject.Properties['inventory_equipment_bridge_output_last_client_gui_status_response_known']
        if ($null -ne $inventoryEquipmentBridgeOutputLastClientGuiStatusResponseKnownProp -and $null -ne $inventoryEquipmentBridgeOutputLastClientGuiStatusResponseKnownProp.Value) {
            $quickbarHintInventoryEquipmentBridgeOutputLastClientGuiStatusResponseKnown = [bool]$inventoryEquipmentBridgeOutputLastClientGuiStatusResponseKnownProp.Value
        }
        $quickbarHintInventoryEquipmentBridgeOutputLastClientGuiStatusResponseQueuedUpdateIndex = & $getQuickbarHintInt64 'inventory_equipment_bridge_output_last_client_gui_status_response_queued_update_index'
        $quickbarHintInventoryEquipmentBridgeOutputLastClientGuiStatusResponseServerSequence = & $getQuickbarHintInt64 'inventory_equipment_bridge_output_last_client_gui_status_response_server_sequence'
        $quickbarHintInventoryEquipmentBridgeOutputLastClientGuiStatusResponseAckSequence = & $getQuickbarHintInt64 'inventory_equipment_bridge_output_last_client_gui_status_response_ack_sequence'
        $quickbarHintInventoryEquipmentBridgeOutputLastClientGuiStatusResponseLiveGuiRecords = & $getQuickbarHintInt64 'inventory_equipment_bridge_output_last_client_gui_status_response_live_gui_records'
        $quickbarHintInventoryEquipmentBridgeOutputLastClientGuiStatusResponseLiveGuiFragmentBits = & $getQuickbarHintInt64 'inventory_equipment_bridge_output_last_client_gui_status_response_live_gui_fragment_bits'
        $quickbarHintInventoryEquipmentBridgeOutputLastClientGuiStatusResponseMaterializedItemObjectIds = & $getQuickbarHintInt64 'inventory_equipment_bridge_output_last_client_gui_status_response_materialized_item_object_ids'
        $quickbarHintInventoryEquipmentBridgeOutputLastClientGuiStatusResponseReadyObjects = & $getQuickbarHintInt64 'inventory_equipment_bridge_output_last_client_gui_status_response_ready_objects'
        $inventoryEquipmentBridgeOutputLastClientGuiStatusResponseCandidateKnownProp = $quickbarHintJson.PSObject.Properties['inventory_equipment_bridge_output_last_client_gui_status_response_candidate_known']
        if ($null -ne $inventoryEquipmentBridgeOutputLastClientGuiStatusResponseCandidateKnownProp -and $null -ne $inventoryEquipmentBridgeOutputLastClientGuiStatusResponseCandidateKnownProp.Value) {
            $quickbarHintInventoryEquipmentBridgeOutputLastClientGuiStatusResponseCandidateKnown = [bool]$inventoryEquipmentBridgeOutputLastClientGuiStatusResponseCandidateKnownProp.Value
        }
        $quickbarHintInventoryEquipmentBridgeOutputLastClientGuiStatusResponseCandidateObjectId = & $getQuickbarHintInt64 'inventory_equipment_bridge_output_last_client_gui_status_response_candidate_object_id'
        $inventoryEquipmentBridgeOutputLastClientGuiStatusResponseCandidateProofProp = $quickbarHintJson.PSObject.Properties['inventory_equipment_bridge_output_last_client_gui_status_response_candidate_proof']
        if ($null -ne $inventoryEquipmentBridgeOutputLastClientGuiStatusResponseCandidateProofProp -and $null -ne $inventoryEquipmentBridgeOutputLastClientGuiStatusResponseCandidateProofProp.Value) {
            $quickbarHintInventoryEquipmentBridgeOutputLastClientGuiStatusResponseCandidateProof = [string]$inventoryEquipmentBridgeOutputLastClientGuiStatusResponseCandidateProofProp.Value
        }
        $inventoryEquipmentBridgeOutputLastClientGuiStatusResponseCandidateSourceProp = $quickbarHintJson.PSObject.Properties['inventory_equipment_bridge_output_last_client_gui_status_response_candidate_source']
        if ($null -ne $inventoryEquipmentBridgeOutputLastClientGuiStatusResponseCandidateSourceProp -and $null -ne $inventoryEquipmentBridgeOutputLastClientGuiStatusResponseCandidateSourceProp.Value) {
            $quickbarHintInventoryEquipmentBridgeOutputLastClientGuiStatusResponseCandidateSource = [string]$inventoryEquipmentBridgeOutputLastClientGuiStatusResponseCandidateSourceProp.Value
        }
        $inventoryEquipmentBridgeOutputClientGuiStatusResponseOutcomeProp = $quickbarHintJson.PSObject.Properties['inventory_equipment_bridge_output_client_gui_status_response_outcome']
        if ($null -ne $inventoryEquipmentBridgeOutputClientGuiStatusResponseOutcomeProp -and $null -ne $inventoryEquipmentBridgeOutputClientGuiStatusResponseOutcomeProp.Value) {
            $quickbarHintInventoryEquipmentBridgeOutputClientGuiStatusResponseOutcome = [string]$inventoryEquipmentBridgeOutputClientGuiStatusResponseOutcomeProp.Value
        }
        $inventoryEquipmentBridgeOutputBestClientGuiStatusResponseKnownProp = $quickbarHintJson.PSObject.Properties['inventory_equipment_bridge_output_best_client_gui_status_response_known']
        if ($null -ne $inventoryEquipmentBridgeOutputBestClientGuiStatusResponseKnownProp -and $null -ne $inventoryEquipmentBridgeOutputBestClientGuiStatusResponseKnownProp.Value) {
            $quickbarHintInventoryEquipmentBridgeOutputBestClientGuiStatusResponseKnown = [bool]$inventoryEquipmentBridgeOutputBestClientGuiStatusResponseKnownProp.Value
        }
        $quickbarHintInventoryEquipmentBridgeOutputBestClientGuiStatusResponseQueuedUpdateIndex = & $getQuickbarHintInt64 'inventory_equipment_bridge_output_best_client_gui_status_response_queued_update_index'
        $quickbarHintInventoryEquipmentBridgeOutputBestClientGuiStatusResponseServerSequence = & $getQuickbarHintInt64 'inventory_equipment_bridge_output_best_client_gui_status_response_server_sequence'
        $quickbarHintInventoryEquipmentBridgeOutputBestClientGuiStatusResponseAckSequence = & $getQuickbarHintInt64 'inventory_equipment_bridge_output_best_client_gui_status_response_ack_sequence'
        $quickbarHintInventoryEquipmentBridgeOutputBestClientGuiStatusResponseLiveGuiRecords = & $getQuickbarHintInt64 'inventory_equipment_bridge_output_best_client_gui_status_response_live_gui_records'
        $quickbarHintInventoryEquipmentBridgeOutputBestClientGuiStatusResponseLiveGuiFragmentBits = & $getQuickbarHintInt64 'inventory_equipment_bridge_output_best_client_gui_status_response_live_gui_fragment_bits'
        $quickbarHintInventoryEquipmentBridgeOutputBestClientGuiStatusResponseMaterializedItemObjectIds = & $getQuickbarHintInt64 'inventory_equipment_bridge_output_best_client_gui_status_response_materialized_item_object_ids'
        $quickbarHintInventoryEquipmentBridgeOutputBestClientGuiStatusResponseReadyObjects = & $getQuickbarHintInt64 'inventory_equipment_bridge_output_best_client_gui_status_response_ready_objects'
        $inventoryEquipmentBridgeOutputBestClientGuiStatusResponseCandidateKnownProp = $quickbarHintJson.PSObject.Properties['inventory_equipment_bridge_output_best_client_gui_status_response_candidate_known']
        if ($null -ne $inventoryEquipmentBridgeOutputBestClientGuiStatusResponseCandidateKnownProp -and $null -ne $inventoryEquipmentBridgeOutputBestClientGuiStatusResponseCandidateKnownProp.Value) {
            $quickbarHintInventoryEquipmentBridgeOutputBestClientGuiStatusResponseCandidateKnown = [bool]$inventoryEquipmentBridgeOutputBestClientGuiStatusResponseCandidateKnownProp.Value
        }
        $quickbarHintInventoryEquipmentBridgeOutputBestClientGuiStatusResponseCandidateObjectId = & $getQuickbarHintInt64 'inventory_equipment_bridge_output_best_client_gui_status_response_candidate_object_id'
        $inventoryEquipmentBridgeOutputBestClientGuiStatusResponseCandidateProofProp = $quickbarHintJson.PSObject.Properties['inventory_equipment_bridge_output_best_client_gui_status_response_candidate_proof']
        if ($null -ne $inventoryEquipmentBridgeOutputBestClientGuiStatusResponseCandidateProofProp -and $null -ne $inventoryEquipmentBridgeOutputBestClientGuiStatusResponseCandidateProofProp.Value) {
            $quickbarHintInventoryEquipmentBridgeOutputBestClientGuiStatusResponseCandidateProof = [string]$inventoryEquipmentBridgeOutputBestClientGuiStatusResponseCandidateProofProp.Value
        }
        $inventoryEquipmentBridgeOutputBestClientGuiStatusResponseCandidateSourceProp = $quickbarHintJson.PSObject.Properties['inventory_equipment_bridge_output_best_client_gui_status_response_candidate_source']
        if ($null -ne $inventoryEquipmentBridgeOutputBestClientGuiStatusResponseCandidateSourceProp -and $null -ne $inventoryEquipmentBridgeOutputBestClientGuiStatusResponseCandidateSourceProp.Value) {
            $quickbarHintInventoryEquipmentBridgeOutputBestClientGuiStatusResponseCandidateSource = [string]$inventoryEquipmentBridgeOutputBestClientGuiStatusResponseCandidateSourceProp.Value
        }
        $inventoryEquipmentBridgeOutputBestClientGuiStatusResponseAssociationProp = $quickbarHintJson.PSObject.Properties['inventory_equipment_bridge_output_best_client_gui_status_response_association']
        if ($null -ne $inventoryEquipmentBridgeOutputBestClientGuiStatusResponseAssociationProp -and $null -ne $inventoryEquipmentBridgeOutputBestClientGuiStatusResponseAssociationProp.Value) {
            $quickbarHintInventoryEquipmentBridgeOutputBestClientGuiStatusResponseAssociation = [string]$inventoryEquipmentBridgeOutputBestClientGuiStatusResponseAssociationProp.Value
        }
        $inventoryEquipmentBridgeOutputBestClientGuiStatusResponseMatchesQueuedStatusCandidateProp = $quickbarHintJson.PSObject.Properties['inventory_equipment_bridge_output_best_client_gui_status_response_matches_queued_status_candidate']
        if ($null -ne $inventoryEquipmentBridgeOutputBestClientGuiStatusResponseMatchesQueuedStatusCandidateProp -and $null -ne $inventoryEquipmentBridgeOutputBestClientGuiStatusResponseMatchesQueuedStatusCandidateProp.Value) {
            $quickbarHintInventoryEquipmentBridgeOutputBestClientGuiStatusResponseMatchesQueuedStatusCandidate = [bool]$inventoryEquipmentBridgeOutputBestClientGuiStatusResponseMatchesQueuedStatusCandidateProp.Value
        }
        $quickbarHintInventoryEquipmentBridgeOutputBestClientGuiStatusResponseCandidateDeltaFromQueuedStatusCandidate = & $getQuickbarHintInt64 'inventory_equipment_bridge_output_best_client_gui_status_response_candidate_delta_from_queued_status_candidate'
        $quickbarHintInventoryFeature25FirstItemRefs = & $getQuickbarHintInt64 'inventory_feature25_first_item_refs'
        $quickbarHintInventoryFeature25FirstItemRefMentions = & $getQuickbarHintInt64 'inventory_feature25_first_item_ref_mentions'
        $quickbarHintInventoryFeature25FirstMaterializedItemRefMentions = & $getQuickbarHintInt64 'inventory_feature25_first_materialized_item_ref_mentions'
        $quickbarHintInventoryFeature25FirstDeferredItemRefMentions = & $getQuickbarHintInt64 'inventory_feature25_first_deferred_item_ref_mentions'
        $quickbarHintInventoryFeature25SecondItemRefs = & $getQuickbarHintInt64 'inventory_feature25_second_item_refs'
        $quickbarHintInventoryFeature25SecondItemRefMentions = & $getQuickbarHintInt64 'inventory_feature25_second_item_ref_mentions'
        $quickbarHintInventoryFeature25SecondMaterializedItemRefMentions = & $getQuickbarHintInt64 'inventory_feature25_second_materialized_item_ref_mentions'
        $quickbarHintInventoryFeature25SecondDeferredItemRefMentions = & $getQuickbarHintInt64 'inventory_feature25_second_deferred_item_ref_mentions'
        $quickbarHintInventoryFeature25LegacyTailItemRefs = & $getQuickbarHintInt64 'inventory_feature25_legacy_tail_item_refs'
        $quickbarHintInventoryFeature25LegacyTailItemRefMentions = & $getQuickbarHintInt64 'inventory_feature25_legacy_tail_item_ref_mentions'
        $quickbarHintInventoryFeature25LegacyTailMaterializedItemRefMentions = & $getQuickbarHintInt64 'inventory_feature25_legacy_tail_materialized_item_ref_mentions'
        $quickbarHintInventoryFeature25LegacyTailDeferredItemRefMentions = & $getQuickbarHintInt64 'inventory_feature25_legacy_tail_deferred_item_ref_mentions'
        $quickbarHintClearedInventoryItemObjectIds = & $getQuickbarHintInt64 'cleared_inventory_item_object_ids'
        $matchClassProp = $quickbarHintJson.PSObject.Properties['first_client_action_match_class']
        if ($null -ne $matchClassProp -and $null -ne $matchClassProp.Value) {
            $quickbarHintFirstActionMatchClass = [string]$matchClassProp.Value
        }
        $recommendedActionOutcomeProp = $quickbarHintJson.PSObject.Properties['pending_item_refresh_recommended_action_outcome']
        if ($null -ne $recommendedActionOutcomeProp -and $null -ne $recommendedActionOutcomeProp.Value) {
            $quickbarHintRecommendedActionOutcome = [string]$recommendedActionOutcomeProp.Value
        }
        $activePropertyOutcomeProp = $quickbarHintJson.PSObject.Properties['pending_item_refresh_active_property_outcome']
        if ($null -ne $activePropertyOutcomeProp -and $null -ne $activePropertyOutcomeProp.Value) {
            $quickbarHintActivePropertyOutcome = [string]$activePropertyOutcomeProp.Value
        }
        $serverQuickbarResponseTimingProp = $quickbarHintJson.PSObject.Properties['pending_item_refresh_server_quickbar_response_timing']
        if ($null -ne $serverQuickbarResponseTimingProp -and $null -ne $serverQuickbarResponseTimingProp.Value) {
            $quickbarHintServerQuickbarResponseTiming = [string]$serverQuickbarResponseTimingProp.Value
        }
        $postCommittedItemRefreshResolutionProp = $quickbarHintJson.PSObject.Properties['post_committed_item_refresh_resolution']
        if ($null -ne $postCommittedItemRefreshResolutionProp -and $null -ne $postCommittedItemRefreshResolutionProp.Value) {
            $quickbarHintPostCommittedItemRefreshResolution = [string]$postCommittedItemRefreshResolutionProp.Value
        }
        $quickbarHintQuickbarItemUseCountStateRows = & $getQuickbarHintInt64 'quickbar_item_use_count_state_rows'
        $quickbarHintQuickbarItemUseCountUpdatesObserved = & $getQuickbarHintInt64 'quickbar_item_use_count_updates_observed'
        $candidateUseCountStateKnownProp = $quickbarHintJson.PSObject.Properties['candidate_quickbar_item_use_count_state_known']
        if ($null -ne $candidateUseCountStateKnownProp -and $null -ne $candidateUseCountStateKnownProp.Value) {
            $quickbarHintCandidateQuickbarItemUseCountStateKnown = [bool]$candidateUseCountStateKnownProp.Value
        }
        $candidateUseCountStateSlotRelationProp = $quickbarHintJson.PSObject.Properties['candidate_quickbar_item_use_count_state_slot_relation']
        if ($null -ne $candidateUseCountStateSlotRelationProp -and $null -ne $candidateUseCountStateSlotRelationProp.Value) {
            $quickbarHintCandidateQuickbarItemUseCountStateSlotRelation = [string]$candidateUseCountStateSlotRelationProp.Value
        }
        $candidateUseCountStateSlotMatchesFirstPreservedActiveItemProp = $quickbarHintJson.PSObject.Properties['candidate_quickbar_item_use_count_state_slot_matches_first_preserved_active_item']
        if ($null -ne $candidateUseCountStateSlotMatchesFirstPreservedActiveItemProp -and $null -ne $candidateUseCountStateSlotMatchesFirstPreservedActiveItemProp.Value) {
            $quickbarHintCandidateQuickbarItemUseCountStateSlotMatchesFirstPreservedActiveItem = [bool]$candidateUseCountStateSlotMatchesFirstPreservedActiveItemProp.Value
        }
        $quickbarHintCandidateQuickbarItemUseCountStateSlot = & $getQuickbarHintInt64 'candidate_quickbar_item_use_count_state_slot'
        $quickbarHintCandidateQuickbarItemUseCountStateButtonType = & $getQuickbarHintInt64 'candidate_quickbar_item_use_count_state_button_type'
        $quickbarHintCandidateQuickbarItemUseCountStateObjectId = & $getQuickbarHintInt64 'candidate_quickbar_item_use_count_state_object_id'
        $quickbarHintCandidateQuickbarItemUseCountStateActivePropertyIndex = & $getQuickbarHintInt64 'candidate_quickbar_item_use_count_state_active_property_index'
        $quickbarHintCandidateQuickbarItemUseCountStateUseCount = & $getQuickbarHintInt64 'candidate_quickbar_item_use_count_state_use_count'
        $quickbarHintFirstPreservedActiveItemQuickbarUseCountStateKnown = & $getQuickbarHintBoolAny -Names @('first_preserved_active_item_quickbar_use_count_state_known', 'stream_probe_first_preserved_active_item_quickbar_use_count_state_known')
        $quickbarHintFirstPreservedActiveItemQuickbarUseCountStateSlotRelation = & $getQuickbarHintStringAny -Names @('first_preserved_active_item_quickbar_use_count_state_slot_relation', 'stream_probe_first_preserved_active_item_quickbar_use_count_state_slot_relation')
        $quickbarHintFirstPreservedActiveItemQuickbarUseCountStateSlotMatchesFirstPreservedActiveItem = & $getQuickbarHintBoolAny -Names @('first_preserved_active_item_quickbar_use_count_state_slot_matches_first_preserved_active_item', 'stream_probe_first_preserved_active_item_quickbar_use_count_state_slot_matches_first_preserved_active_item')
        $quickbarHintFirstPreservedActiveItemQuickbarUseCountStateSlot = & $getQuickbarHintInt64Any -Names @('first_preserved_active_item_quickbar_use_count_state_slot', 'stream_probe_first_preserved_active_item_quickbar_use_count_state_slot')
        $quickbarHintFirstPreservedActiveItemQuickbarUseCountStateButtonType = & $getQuickbarHintInt64Any -Names @('first_preserved_active_item_quickbar_use_count_state_button_type', 'stream_probe_first_preserved_active_item_quickbar_use_count_state_button_type')
        $quickbarHintFirstPreservedActiveItemQuickbarUseCountStateObjectId = & $getQuickbarHintInt64Any -Names @('first_preserved_active_item_quickbar_use_count_state_object_id', 'stream_probe_first_preserved_active_item_quickbar_use_count_state_object_id')
        $quickbarHintFirstPreservedActiveItemQuickbarUseCountStateActivePropertyIndex = & $getQuickbarHintInt64Any -Names @('first_preserved_active_item_quickbar_use_count_state_active_property_index', 'stream_probe_first_preserved_active_item_quickbar_use_count_state_active_property_index')
        $quickbarHintFirstPreservedActiveItemQuickbarUseCountStateUseCount = & $getQuickbarHintInt64Any -Names @('first_preserved_active_item_quickbar_use_count_state_use_count', 'stream_probe_first_preserved_active_item_quickbar_use_count_state_use_count')
        $firstUseCountCandidateRowKnownProp = $quickbarHintJson.PSObject.Properties['first_server_quickbar_item_use_count_candidate_row_known']
        if ($null -ne $firstUseCountCandidateRowKnownProp -and $null -ne $firstUseCountCandidateRowKnownProp.Value) {
            $quickbarHintFirstServerQuickbarItemUseCountCandidateRowKnown = [bool]$firstUseCountCandidateRowKnownProp.Value
        }
        $firstUseCountCandidateRowTimingProp = $quickbarHintJson.PSObject.Properties['first_server_quickbar_item_use_count_candidate_row_timing']
        if ($null -ne $firstUseCountCandidateRowTimingProp -and $null -ne $firstUseCountCandidateRowTimingProp.Value) {
            $quickbarHintFirstServerQuickbarItemUseCountCandidateRowTiming = [string]$firstUseCountCandidateRowTimingProp.Value
        }
        $firstUseCountCandidateRowSlotRelationProp = $quickbarHintJson.PSObject.Properties['first_server_quickbar_item_use_count_candidate_row_slot_relation']
        if ($null -ne $firstUseCountCandidateRowSlotRelationProp -and $null -ne $firstUseCountCandidateRowSlotRelationProp.Value) {
            $quickbarHintFirstServerQuickbarItemUseCountCandidateRowSlotRelation = [string]$firstUseCountCandidateRowSlotRelationProp.Value
        }
        $firstUseCountCandidateRowSlotMatchesFirstPreservedActiveItemProp = $quickbarHintJson.PSObject.Properties['first_server_quickbar_item_use_count_candidate_row_slot_matches_first_preserved_active_item']
        if ($null -ne $firstUseCountCandidateRowSlotMatchesFirstPreservedActiveItemProp -and $null -ne $firstUseCountCandidateRowSlotMatchesFirstPreservedActiveItemProp.Value) {
            $quickbarHintFirstServerQuickbarItemUseCountCandidateRowSlotMatchesFirstPreservedActiveItem = [bool]$firstUseCountCandidateRowSlotMatchesFirstPreservedActiveItemProp.Value
        }
        $quickbarHintFirstServerQuickbarItemUseCountCandidateRowSlot = & $getQuickbarHintInt64 'first_server_quickbar_item_use_count_candidate_row_slot'
        $quickbarHintFirstServerQuickbarItemUseCountCandidateRowButtonType = & $getQuickbarHintInt64 'first_server_quickbar_item_use_count_candidate_row_button_type'
        $quickbarHintFirstServerQuickbarItemUseCountCandidateRowObjectId = & $getQuickbarHintInt64 'first_server_quickbar_item_use_count_candidate_row_object_id'
        $quickbarHintFirstServerQuickbarItemUseCountCandidateRowActivePropertyIndex = & $getQuickbarHintInt64 'first_server_quickbar_item_use_count_candidate_row_active_property_index'
        $quickbarHintFirstServerQuickbarItemUseCountCandidateRowUseCount = & $getQuickbarHintInt64 'first_server_quickbar_item_use_count_candidate_row_use_count'
        $beforeUseCountCandidateRowKnownProp = $quickbarHintJson.PSObject.Properties['first_server_quickbar_item_use_count_candidate_row_before_first_client_action_known']
        if ($null -ne $beforeUseCountCandidateRowKnownProp -and $null -ne $beforeUseCountCandidateRowKnownProp.Value) {
            $quickbarHintFirstServerQuickbarItemUseCountCandidateRowBeforeFirstClientActionKnown = [bool]$beforeUseCountCandidateRowKnownProp.Value
        }
        $quickbarHintFirstServerQuickbarItemUseCountCandidateRowBeforeFirstClientActionSlot = & $getQuickbarHintInt64 'first_server_quickbar_item_use_count_candidate_row_before_first_client_action_slot'
        $quickbarHintFirstServerQuickbarItemUseCountCandidateRowBeforeFirstClientActionButtonType = & $getQuickbarHintInt64 'first_server_quickbar_item_use_count_candidate_row_before_first_client_action_button_type'
        $quickbarHintFirstServerQuickbarItemUseCountCandidateRowBeforeFirstClientActionActivePropertyIndex = & $getQuickbarHintInt64 'first_server_quickbar_item_use_count_candidate_row_before_first_client_action_active_property_index'
        $quickbarHintFirstServerQuickbarItemUseCountCandidateRowBeforeFirstClientActionUseCount = & $getQuickbarHintInt64 'first_server_quickbar_item_use_count_candidate_row_before_first_client_action_use_count'
        $afterUseCountCandidateRowKnownProp = $quickbarHintJson.PSObject.Properties['first_server_quickbar_item_use_count_candidate_row_after_first_client_action_known']
        if ($null -ne $afterUseCountCandidateRowKnownProp -and $null -ne $afterUseCountCandidateRowKnownProp.Value) {
            $quickbarHintFirstServerQuickbarItemUseCountCandidateRowAfterFirstClientActionKnown = [bool]$afterUseCountCandidateRowKnownProp.Value
        }
        $quickbarHintFirstServerQuickbarItemUseCountCandidateRowAfterFirstClientActionSlot = & $getQuickbarHintInt64 'first_server_quickbar_item_use_count_candidate_row_after_first_client_action_slot'
        $quickbarHintFirstServerQuickbarItemUseCountCandidateRowAfterFirstClientActionButtonType = & $getQuickbarHintInt64 'first_server_quickbar_item_use_count_candidate_row_after_first_client_action_button_type'
        $quickbarHintFirstServerQuickbarItemUseCountCandidateRowAfterFirstClientActionActivePropertyIndex = & $getQuickbarHintInt64 'first_server_quickbar_item_use_count_candidate_row_after_first_client_action_active_property_index'
        $quickbarHintFirstServerQuickbarItemUseCountCandidateRowAfterFirstClientActionUseCount = & $getQuickbarHintInt64 'first_server_quickbar_item_use_count_candidate_row_after_first_client_action_use_count'
        $timingProp = $quickbarHintJson.PSObject.Properties['first_client_action_timing']
        if ($null -ne $timingProp -and $null -ne $timingProp.Value) {
            $quickbarHintFirstClientActionTiming = [string]$timingProp.Value
        }
        $beforeActionFollowupsProp = $quickbarHintJson.PSObject.Properties['followup_events_before_first_client_action']
        if ($null -ne $beforeActionFollowupsProp -and $null -ne $beforeActionFollowupsProp.Value) {
            $quickbarHintFollowupEventsBeforeFirstClientAction = [int64]$beforeActionFollowupsProp.Value
        }
        $sincePendingServerToClientProp = $quickbarHintJson.PSObject.Properties['server_to_client_events_since_pending_refresh']
        if ($null -ne $sincePendingServerToClientProp -and $null -ne $sincePendingServerToClientProp.Value) {
            $quickbarHintServerToClientEventsSincePendingRefresh = [int64]$sincePendingServerToClientProp.Value
        }
        $sincePendingClientToServerProp = $quickbarHintJson.PSObject.Properties['client_to_server_events_since_pending_refresh']
        if ($null -ne $sincePendingClientToServerProp -and $null -ne $sincePendingClientToServerProp.Value) {
            $quickbarHintClientToServerEventsSincePendingRefresh = [int64]$sincePendingClientToServerProp.Value
        }
        $sincePendingClientGuiEventProp = $quickbarHintJson.PSObject.Properties['client_gui_event_events_since_pending_refresh']
        if ($null -ne $sincePendingClientGuiEventProp -and $null -ne $sincePendingClientGuiEventProp.Value) {
            $quickbarHintClientGuiEventEventsSincePendingRefresh = [int64]$sincePendingClientGuiEventProp.Value
        }
        $sincePendingUseCountEventsProp = $quickbarHintJson.PSObject.Properties['server_quickbar_item_use_count_events_since_pending_refresh']
        if ($null -ne $sincePendingUseCountEventsProp -and $null -ne $sincePendingUseCountEventsProp.Value) {
            $quickbarHintServerQuickbarItemUseCountEventsSincePendingRefresh = [int64]$sincePendingUseCountEventsProp.Value
        }
        $sincePendingUseCountRecordsProp = $quickbarHintJson.PSObject.Properties['server_quickbar_item_use_count_records_since_pending_refresh']
        if ($null -ne $sincePendingUseCountRecordsProp -and $null -ne $sincePendingUseCountRecordsProp.Value) {
            $quickbarHintServerQuickbarItemUseCountRecordsSincePendingRefresh = [int64]$sincePendingUseCountRecordsProp.Value
        }
        $sincePendingUseCountRowsProp = $quickbarHintJson.PSObject.Properties['server_quickbar_item_use_count_rows_since_pending_refresh']
        if ($null -ne $sincePendingUseCountRowsProp -and $null -ne $sincePendingUseCountRowsProp.Value) {
            $quickbarHintServerQuickbarItemUseCountRowsSincePendingRefresh = [int64]$sincePendingUseCountRowsProp.Value
        }
        $sincePendingUseCountCandidateRowsProp = $quickbarHintJson.PSObject.Properties['server_quickbar_item_use_count_candidate_rows_since_pending_refresh']
        if ($null -ne $sincePendingUseCountCandidateRowsProp -and $null -ne $sincePendingUseCountCandidateRowsProp.Value) {
            $quickbarHintServerQuickbarItemUseCountCandidateRowsSincePendingRefresh = [int64]$sincePendingUseCountCandidateRowsProp.Value
        }
        $quickbarHintServerActiveItemPropertyEventsSincePendingRefresh = & $getQuickbarHintInt64 'server_active_item_property_events_since_pending_refresh'
        $quickbarHintServerActiveItemPropertyUsesEventsSincePendingRefresh = & $getQuickbarHintInt64 'server_active_item_property_uses_events_since_pending_refresh'
        $quickbarHintServerActiveItemPropertyFullEventsSincePendingRefresh = & $getQuickbarHintInt64 'server_active_item_property_full_events_since_pending_refresh'
        $quickbarHintServerActiveItemPropertyCandidateEventsSincePendingRefresh = & $getQuickbarHintInt64 'server_active_item_property_candidate_events_since_pending_refresh'
        $quickbarHintServerActiveItemPropertyCandidateUsesEventsSincePendingRefresh = & $getQuickbarHintInt64 'server_active_item_property_candidate_uses_events_since_pending_refresh'
        $quickbarHintServerActiveItemPropertyCandidateFullEventsSincePendingRefresh = & $getQuickbarHintInt64 'server_active_item_property_candidate_full_events_since_pending_refresh'
        $quickbarHintServerActiveItemPropertyCandidateChangedUseCountRowsSincePendingRefresh = & $getQuickbarHintInt64 'server_active_item_property_candidate_changed_use_count_rows_since_pending_refresh'
        $quickbarHintServerActiveItemPropertyCandidateFullPropertyRowsSincePendingRefresh = & $getQuickbarHintInt64 'server_active_item_property_candidate_full_property_rows_since_pending_refresh'
        $firstAfterActionProp = $quickbarHintJson.PSObject.Properties['first_event_after_client_action']
        if ($null -ne $firstAfterActionProp -and $null -ne $firstAfterActionProp.Value) {
            $quickbarHintFirstEventAfterClientAction = [string]$firstAfterActionProp.Value
        }
        $afterActionEventsProp = $quickbarHintJson.PSObject.Properties['events_after_first_client_action']
        if ($null -ne $afterActionEventsProp -and $null -ne $afterActionEventsProp.Value) {
            $quickbarHintEventsAfterFirstClientAction = [int64]$afterActionEventsProp.Value
        }
        $afterActionServerToClientProp = $quickbarHintJson.PSObject.Properties['server_to_client_events_after_first_client_action']
        if ($null -ne $afterActionServerToClientProp -and $null -ne $afterActionServerToClientProp.Value) {
            $quickbarHintServerToClientEventsAfterFirstClientAction = [int64]$afterActionServerToClientProp.Value
        }
        $afterActionClientToServerProp = $quickbarHintJson.PSObject.Properties['client_to_server_events_after_first_client_action']
        if ($null -ne $afterActionClientToServerProp -and $null -ne $afterActionClientToServerProp.Value) {
            $quickbarHintClientToServerEventsAfterFirstClientAction = [int64]$afterActionClientToServerProp.Value
        }
        $afterActionLiveObjectProp = $quickbarHintJson.PSObject.Properties['live_object_events_after_first_client_action']
        if ($null -ne $afterActionLiveObjectProp -and $null -ne $afterActionLiveObjectProp.Value) {
            $quickbarHintLiveObjectEventsAfterFirstClientAction = [int64]$afterActionLiveObjectProp.Value
        }
        $afterActionQuickbarProp = $quickbarHintJson.PSObject.Properties['quickbar_events_after_first_client_action']
        if ($null -ne $afterActionQuickbarProp -and $null -ne $afterActionQuickbarProp.Value) {
            $quickbarHintQuickbarEventsAfterFirstClientAction = [int64]$afterActionQuickbarProp.Value
        }
        $afterActionUseCountEventsProp = $quickbarHintJson.PSObject.Properties['server_quickbar_item_use_count_events_after_first_client_action']
        if ($null -ne $afterActionUseCountEventsProp -and $null -ne $afterActionUseCountEventsProp.Value) {
            $quickbarHintServerQuickbarItemUseCountEventsAfterFirstClientAction = [int64]$afterActionUseCountEventsProp.Value
        }
        $afterActionUseCountRecordsProp = $quickbarHintJson.PSObject.Properties['server_quickbar_item_use_count_records_after_first_client_action']
        if ($null -ne $afterActionUseCountRecordsProp -and $null -ne $afterActionUseCountRecordsProp.Value) {
            $quickbarHintServerQuickbarItemUseCountRecordsAfterFirstClientAction = [int64]$afterActionUseCountRecordsProp.Value
        }
        $afterActionUseCountRowsProp = $quickbarHintJson.PSObject.Properties['server_quickbar_item_use_count_rows_after_first_client_action']
        if ($null -ne $afterActionUseCountRowsProp -and $null -ne $afterActionUseCountRowsProp.Value) {
            $quickbarHintServerQuickbarItemUseCountRowsAfterFirstClientAction = [int64]$afterActionUseCountRowsProp.Value
        }
        $afterActionUseCountCandidateRowsProp = $quickbarHintJson.PSObject.Properties['server_quickbar_item_use_count_candidate_rows_after_first_client_action']
        if ($null -ne $afterActionUseCountCandidateRowsProp -and $null -ne $afterActionUseCountCandidateRowsProp.Value) {
            $quickbarHintServerQuickbarItemUseCountCandidateRowsAfterFirstClientAction = [int64]$afterActionUseCountCandidateRowsProp.Value
        }
        $quickbarHintServerActiveItemPropertyEventsAfterFirstClientAction = & $getQuickbarHintInt64 'server_active_item_property_events_after_first_client_action'
        $quickbarHintServerActiveItemPropertyUsesEventsAfterFirstClientAction = & $getQuickbarHintInt64 'server_active_item_property_uses_events_after_first_client_action'
        $quickbarHintServerActiveItemPropertyFullEventsAfterFirstClientAction = & $getQuickbarHintInt64 'server_active_item_property_full_events_after_first_client_action'
        $quickbarHintServerActiveItemPropertyCandidateEventsAfterFirstClientAction = & $getQuickbarHintInt64 'server_active_item_property_candidate_events_after_first_client_action'
        $quickbarHintServerActiveItemPropertyCandidateUsesEventsAfterFirstClientAction = & $getQuickbarHintInt64 'server_active_item_property_candidate_uses_events_after_first_client_action'
        $quickbarHintServerActiveItemPropertyCandidateFullEventsAfterFirstClientAction = & $getQuickbarHintInt64 'server_active_item_property_candidate_full_events_after_first_client_action'
        $quickbarHintServerActiveItemPropertyCandidateChangedUseCountRowsAfterFirstClientAction = & $getQuickbarHintInt64 'server_active_item_property_candidate_changed_use_count_rows_after_first_client_action'
        $quickbarHintServerActiveItemPropertyCandidateFullPropertyRowsAfterFirstClientAction = & $getQuickbarHintInt64 'server_active_item_property_candidate_full_property_rows_after_first_client_action'
        $afterActionInventoryProp = $quickbarHintJson.PSObject.Properties['inventory_events_after_first_client_action']
        if ($null -ne $afterActionInventoryProp -and $null -ne $afterActionInventoryProp.Value) {
            $quickbarHintInventoryEventsAfterFirstClientAction = [int64]$afterActionInventoryProp.Value
        }
        $afterActionClientGuiEventProp = $quickbarHintJson.PSObject.Properties['client_gui_event_events_after_first_client_action']
        if ($null -ne $afterActionClientGuiEventProp -and $null -ne $afterActionClientGuiEventProp.Value) {
            $quickbarHintClientGuiEventEventsAfterFirstClientAction = [int64]$afterActionClientGuiEventProp.Value
        }
        $afterActionOtherProp = $quickbarHintJson.PSObject.Properties['other_events_after_first_client_action']
        if ($null -ne $afterActionOtherProp -and $null -ne $afterActionOtherProp.Value) {
            $quickbarHintOtherEventsAfterFirstClientAction = [int64]$afterActionOtherProp.Value
        }
    }

    $summary = [pscustomobject]@{
        RunRoot = $RunRoot
        PacketDir = $packetDirResolved
        ProxyExe = $proxyResolved
        ProxyLog = $proxyLog
        QuickbarItemRefreshHint = $quickbarItemRefreshHint
        QuickbarItemRefreshHintExists = $quickbarHintExists
        QuickbarItemRefreshHintParseError = $quickbarHintParseError
        QuickbarItemRefreshHintPending = $quickbarHintPending
        QuickbarItemRefreshHintCandidateObjectId = $quickbarHintCandidateObjectId
        QuickbarItemRefreshHintCandidateProof = $quickbarHintCandidateProof
        QuickbarItemRefreshHintCandidateSource = $quickbarHintCandidateSource
        QuickbarItemRefreshHintNoHintReason = $quickbarHintNoHintReason
        QuickbarItemRefreshHintPostCommittedItemRefreshResolution = $quickbarHintPostCommittedItemRefreshResolution
        QuickbarItemRefreshHintFirstActionMatchesCandidate = $quickbarHintFirstActionMatchesCandidate
        QuickbarItemRefreshHintFirstActionMatchesPreservedActiveItem = $quickbarHintFirstActionMatchesPreservedActiveItem
        QuickbarItemRefreshHintFirstPreservedActiveItemSlotKnown = $quickbarHintFirstPreservedActiveItemSlotKnown
        QuickbarItemRefreshHintFirstPreservedActiveItemSlot = $quickbarHintFirstPreservedActiveItemSlot
        QuickbarItemRefreshHintFirstPreservedActiveItemFirstPageSlot = $quickbarHintFirstPreservedActiveItemFirstPageSlot
        QuickbarItemRefreshHintFirstPreservedActiveItemSlotMatchesRecommendedSetButtonSlot = $quickbarHintFirstPreservedActiveItemSlotMatchesRecommendedSetButtonSlot
        QuickbarItemRefreshHintFirstActionMatchClass = $quickbarHintFirstActionMatchClass
        QuickbarItemRefreshHintRecommendedActionOutcome = $quickbarHintRecommendedActionOutcome
        QuickbarItemRefreshHintActivePropertyOutcome = $quickbarHintActivePropertyOutcome
        QuickbarItemRefreshHintServerQuickbarResponseTiming = $quickbarHintServerQuickbarResponseTiming
        QuickbarItemRefreshHintStreamProbeItemButtonsRejectedMissingStateClearedDelete = $quickbarHintStreamProbeItemButtonsRejectedMissingStateClearedDelete
        QuickbarItemRefreshHintStreamProbeItemButtonsRejectedMissingStateClearedAreaReset = $quickbarHintStreamProbeItemButtonsRejectedMissingStateClearedAreaReset
        QuickbarItemRefreshHintStreamProbeItemObjectsRejectedMissingStateProven = $quickbarHintStreamProbeItemObjectsRejectedMissingStateProven
        QuickbarItemRefreshHintStreamProbeItemObjectsRejectedMissingStateActive = $quickbarHintStreamProbeItemObjectsRejectedMissingStateActive
        QuickbarItemRefreshHintStreamProbeItemObjectsRejectedMissingStateFeature25First = $quickbarHintStreamProbeItemObjectsRejectedMissingStateFeature25First
        QuickbarItemRefreshHintStreamProbeItemObjectsRejectedMissingStateFeature25Second = $quickbarHintStreamProbeItemObjectsRejectedMissingStateFeature25Second
        QuickbarItemRefreshHintStreamProbeItemObjectsRejectedMissingStateFeature25LegacyTail = $quickbarHintStreamProbeItemObjectsRejectedMissingStateFeature25LegacyTail
        QuickbarItemRefreshHintStreamProbeItemObjectsRejectedMissingStateUnknown = $quickbarHintStreamProbeItemObjectsRejectedMissingStateUnknown
        QuickbarItemRefreshHintStreamProbeItemObjectsRejectedMissingStateClearedDelete = $quickbarHintStreamProbeItemObjectsRejectedMissingStateClearedDelete
        QuickbarItemRefreshHintStreamProbeItemObjectsRejectedMissingStateClearedAreaReset = $quickbarHintStreamProbeItemObjectsRejectedMissingStateClearedAreaReset
        QuickbarItemRefreshHintStreamProbeItemObjectsPreservedByExplicitSelfMaterialization = $quickbarHintStreamProbeItemObjectsPreservedByExplicitSelfMaterialization
        QuickbarItemRefreshHintStreamProbeItemObjectsPreservedByActiveState = $quickbarHintStreamProbeItemObjectsPreservedByActiveState
        QuickbarItemRefreshHintStreamProbeItemObjectsPreservedByFeature25First = $quickbarHintStreamProbeItemObjectsPreservedByFeature25First
        QuickbarItemRefreshHintStreamProbeItemObjectsPreservedByFeature25Second = $quickbarHintStreamProbeItemObjectsPreservedByFeature25Second
        QuickbarItemRefreshHintStreamProbeItemObjectsPreservedByFeature25LegacyTail = $quickbarHintStreamProbeItemObjectsPreservedByFeature25LegacyTail
        QuickbarItemRefreshHintInventoryFeature25ReferenceRecords = $quickbarHintInventoryFeature25ReferenceRecords
        QuickbarItemRefreshHintInventoryFeature25ItemRefMentions = $quickbarHintInventoryFeature25ItemRefMentions
        QuickbarItemRefreshHintInventoryFeature25MaterializedItemRefMentions = $quickbarHintInventoryFeature25MaterializedItemRefMentions
        QuickbarItemRefreshHintInventoryFeature25DeferredItemRefMentions = $quickbarHintInventoryFeature25DeferredItemRefMentions
        QuickbarItemRefreshHintInventoryFeature25MaterializationOutcome = $quickbarHintInventoryFeature25MaterializationOutcome
        QuickbarItemRefreshHintInventoryFeature25HandoffOutcome = $quickbarHintInventoryFeature25HandoffOutcome
        QuickbarItemRefreshHintInventoryEquipmentHandoffReady = $quickbarHintInventoryEquipmentHandoffReady
        QuickbarItemRefreshHintInventoryEquipmentHandoffOutcome = $quickbarHintInventoryEquipmentHandoffOutcome
        QuickbarItemRefreshHintInventoryEquipmentHandoffEvents = $quickbarHintInventoryEquipmentHandoffEvents
        QuickbarItemRefreshHintInventoryEquipmentHandoffReadyEvents = $quickbarHintInventoryEquipmentHandoffReadyEvents
        QuickbarItemRefreshHintInventoryEquipmentHandoffBlockedWithoutReadyStateEvents = $quickbarHintInventoryEquipmentHandoffBlockedWithoutReadyStateEvents
        QuickbarItemRefreshHintInventoryEquipmentHandoffReadyWithDeferredFeature25Events = $quickbarHintInventoryEquipmentHandoffReadyWithDeferredFeature25Events
        QuickbarItemRefreshHintInventoryEquipmentHandoffServerInventoryEvents = $quickbarHintInventoryEquipmentHandoffServerInventoryEvents
        QuickbarItemRefreshHintInventoryEquipmentHandoffServerInventoryReadyEvents = $quickbarHintInventoryEquipmentHandoffServerInventoryReadyEvents
        QuickbarItemRefreshHintInventoryEquipmentHandoffServerInventoryBlockedWithoutReadyStateEvents = $quickbarHintInventoryEquipmentHandoffServerInventoryBlockedWithoutReadyStateEvents
        QuickbarItemRefreshHintInventoryEquipmentHandoffClientGuiInventoryEvents = $quickbarHintInventoryEquipmentHandoffClientGuiInventoryEvents
        QuickbarItemRefreshHintInventoryEquipmentHandoffClientGuiInventoryReadyEvents = $quickbarHintInventoryEquipmentHandoffClientGuiInventoryReadyEvents
        QuickbarItemRefreshHintInventoryEquipmentHandoffClientGuiInventoryBlockedWithoutReadyStateEvents = $quickbarHintInventoryEquipmentHandoffClientGuiInventoryBlockedWithoutReadyStateEvents
        QuickbarItemRefreshHintLastInventoryEquipmentHandoffKnown = $quickbarHintLastInventoryEquipmentHandoffKnown
        QuickbarItemRefreshHintLastInventoryEquipmentHandoffConsumer = $quickbarHintLastInventoryEquipmentHandoffConsumer
        QuickbarItemRefreshHintLastInventoryEquipmentHandoffEventIndex = $quickbarHintLastInventoryEquipmentHandoffEventIndex
        QuickbarItemRefreshHintLastInventoryEquipmentHandoffOutcome = $quickbarHintLastInventoryEquipmentHandoffOutcome
        QuickbarItemRefreshHintLastInventoryEquipmentHandoffReadyObjects = $quickbarHintLastInventoryEquipmentHandoffReadyObjects
        QuickbarItemRefreshHintLastInventoryEquipmentHandoffDeferredFeature25OnlyObjects = $quickbarHintLastInventoryEquipmentHandoffDeferredFeature25OnlyObjects
        QuickbarItemRefreshHintLastInventoryEquipmentHandoffCandidateKnown = $quickbarHintLastInventoryEquipmentHandoffCandidateKnown
        QuickbarItemRefreshHintLastInventoryEquipmentHandoffCandidateObjectId = $quickbarHintLastInventoryEquipmentHandoffCandidateObjectId
        QuickbarItemRefreshHintLastInventoryEquipmentHandoffCandidateProof = $quickbarHintLastInventoryEquipmentHandoffCandidateProof
        QuickbarItemRefreshHintLastInventoryEquipmentHandoffCandidateSource = $quickbarHintLastInventoryEquipmentHandoffCandidateSource
        QuickbarItemRefreshHintInventoryEquipmentBridgeHandoffAction = $quickbarHintInventoryEquipmentBridgeHandoffAction
        QuickbarItemRefreshHintInventoryEquipmentBridgeHandoffReady = $quickbarHintInventoryEquipmentBridgeHandoffReady
        QuickbarItemRefreshHintInventoryEquipmentBridgeHandoffConsumer = $quickbarHintInventoryEquipmentBridgeHandoffConsumer
        QuickbarItemRefreshHintInventoryEquipmentBridgeHandoffEventIndex = $quickbarHintInventoryEquipmentBridgeHandoffEventIndex
        QuickbarItemRefreshHintInventoryEquipmentBridgeHandoffOutcome = $quickbarHintInventoryEquipmentBridgeHandoffOutcome
        QuickbarItemRefreshHintInventoryEquipmentBridgeHandoffReadyObjects = $quickbarHintInventoryEquipmentBridgeHandoffReadyObjects
        QuickbarItemRefreshHintInventoryEquipmentBridgeHandoffDeferredFeature25OnlyObjects = $quickbarHintInventoryEquipmentBridgeHandoffDeferredFeature25OnlyObjects
        QuickbarItemRefreshHintInventoryEquipmentBridgeHandoffCandidateKnown = $quickbarHintInventoryEquipmentBridgeHandoffCandidateKnown
        QuickbarItemRefreshHintInventoryEquipmentBridgeHandoffCandidateObjectId = $quickbarHintInventoryEquipmentBridgeHandoffCandidateObjectId
        QuickbarItemRefreshHintInventoryEquipmentBridgeHandoffCandidateProof = $quickbarHintInventoryEquipmentBridgeHandoffCandidateProof
        QuickbarItemRefreshHintInventoryEquipmentBridgeHandoffCandidateSource = $quickbarHintInventoryEquipmentBridgeHandoffCandidateSource
        QuickbarItemRefreshHintInventoryEquipmentBridgeHandoffEmissions = $quickbarHintInventoryEquipmentBridgeHandoffEmissions
        QuickbarItemRefreshHintInventoryEquipmentBridgeHandoffLastEmittedKnown = $quickbarHintInventoryEquipmentBridgeHandoffLastEmittedKnown
        QuickbarItemRefreshHintInventoryEquipmentBridgeHandoffLastEmittedIndex = $quickbarHintInventoryEquipmentBridgeHandoffLastEmittedIndex
        QuickbarItemRefreshHintInventoryEquipmentBridgeHandoffLastEmittedConsumer = $quickbarHintInventoryEquipmentBridgeHandoffLastEmittedConsumer
        QuickbarItemRefreshHintInventoryEquipmentBridgeHandoffLastEmittedEventIndex = $quickbarHintInventoryEquipmentBridgeHandoffLastEmittedEventIndex
        QuickbarItemRefreshHintInventoryEquipmentBridgeHandoffLastEmittedCandidateObjectId = $quickbarHintInventoryEquipmentBridgeHandoffLastEmittedCandidateObjectId
        QuickbarItemRefreshHintInventoryEquipmentBridgeHandoffLastEmittedCandidateSource = $quickbarHintInventoryEquipmentBridgeHandoffLastEmittedCandidateSource
        QuickbarItemRefreshHintInventoryEquipmentBridgeHandoffStateUpdates = $quickbarHintInventoryEquipmentBridgeHandoffStateUpdates
        QuickbarItemRefreshHintInventoryEquipmentBridgeHandoffLastStateUpdateKnown = $quickbarHintInventoryEquipmentBridgeHandoffLastStateUpdateKnown
        QuickbarItemRefreshHintInventoryEquipmentBridgeHandoffLastStateUpdateIndex = $quickbarHintInventoryEquipmentBridgeHandoffLastStateUpdateIndex
        QuickbarItemRefreshHintInventoryEquipmentBridgeHandoffLastStateUpdateEmissionIndex = $quickbarHintInventoryEquipmentBridgeHandoffLastStateUpdateEmissionIndex
        QuickbarItemRefreshHintInventoryEquipmentBridgeHandoffLastStateUpdateConsumer = $quickbarHintInventoryEquipmentBridgeHandoffLastStateUpdateConsumer
        QuickbarItemRefreshHintInventoryEquipmentBridgeHandoffLastStateUpdateEventIndex = $quickbarHintInventoryEquipmentBridgeHandoffLastStateUpdateEventIndex
        QuickbarItemRefreshHintInventoryEquipmentBridgeHandoffLastStateUpdateCandidateObjectId = $quickbarHintInventoryEquipmentBridgeHandoffLastStateUpdateCandidateObjectId
        QuickbarItemRefreshHintInventoryEquipmentBridgeHandoffLastStateUpdateCandidateProof = $quickbarHintInventoryEquipmentBridgeHandoffLastStateUpdateCandidateProof
        QuickbarItemRefreshHintInventoryEquipmentBridgeHandoffLastStateUpdateCandidateSource = $quickbarHintInventoryEquipmentBridgeHandoffLastStateUpdateCandidateSource
        QuickbarItemRefreshHintInventoryEquipmentBridgeHandoffLastStateUpdateReadyObjects = $quickbarHintInventoryEquipmentBridgeHandoffLastStateUpdateReadyObjects
        QuickbarItemRefreshHintInventoryEquipmentBridgeHandoffLastStateUpdateDeferredFeature25OnlyObjects = $quickbarHintInventoryEquipmentBridgeHandoffLastStateUpdateDeferredFeature25OnlyObjects
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputQueuedPackets = $quickbarHintInventoryEquipmentBridgeOutputQueuedPackets
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputDeferredClientGuiUpdates = $quickbarHintInventoryEquipmentBridgeOutputDeferredClientGuiUpdates
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputDeferredMissingClaimUpdates = $quickbarHintInventoryEquipmentBridgeOutputDeferredMissingClaimUpdates
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputBlockedCandidateMismatchUpdates = $quickbarHintInventoryEquipmentBridgeOutputBlockedCandidateMismatchUpdates
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputStatus = $quickbarHintInventoryEquipmentBridgeOutputStatus
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputRequiresClientGuiWriter = $quickbarHintInventoryEquipmentBridgeOutputRequiresClientGuiWriter
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputLastDecisionUpdateIndex = $quickbarHintInventoryEquipmentBridgeOutputLastDecisionUpdateIndex
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputLastDecisionKnown = $quickbarHintInventoryEquipmentBridgeOutputLastDecisionKnown
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputLastDecisionReason = $quickbarHintInventoryEquipmentBridgeOutputLastDecisionReason
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputLastDecisionConsumer = $quickbarHintInventoryEquipmentBridgeOutputLastDecisionConsumer
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputLastDecisionEmissionIndex = $quickbarHintInventoryEquipmentBridgeOutputLastDecisionEmissionIndex
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputLastDecisionEventIndex = $quickbarHintInventoryEquipmentBridgeOutputLastDecisionEventIndex
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputLastDecisionCandidateObjectId = $quickbarHintInventoryEquipmentBridgeOutputLastDecisionCandidateObjectId
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputLastDecisionCandidateProof = $quickbarHintInventoryEquipmentBridgeOutputLastDecisionCandidateProof
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputLastDecisionCandidateSource = $quickbarHintInventoryEquipmentBridgeOutputLastDecisionCandidateSource
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputLastDecisionCandidateObjectStatus = $quickbarHintInventoryEquipmentBridgeOutputLastDecisionCandidateObjectStatus
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputLastDecisionCandidateObjectStatusProof = $quickbarHintInventoryEquipmentBridgeOutputLastDecisionCandidateObjectStatusProof
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputLastDecisionReadyObjects = $quickbarHintInventoryEquipmentBridgeOutputLastDecisionReadyObjects
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputLastDecisionDeferredFeature25OnlyObjects = $quickbarHintInventoryEquipmentBridgeOutputLastDecisionDeferredFeature25OnlyObjects
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimKnown = $quickbarHintInventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimKnown
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimMinor = $quickbarHintInventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimMinor
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimObjectId = $quickbarHintInventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimObjectId
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimObjectStatus = $quickbarHintInventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimObjectStatus
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimObjectStatusProof = $quickbarHintInventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimObjectStatusProof
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimClosestProvenItemKnown = $quickbarHintInventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimClosestProvenItemKnown
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimClosestProvenItemObjectId = $quickbarHintInventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimClosestProvenItemObjectId
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimClosestProvenItemDistance = $quickbarHintInventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimClosestProvenItemDistance
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimLowerProvenItemKnown = $quickbarHintInventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimLowerProvenItemKnown
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimLowerProvenItemObjectId = $quickbarHintInventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimLowerProvenItemObjectId
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimLowerProvenItemDistance = $quickbarHintInventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimLowerProvenItemDistance
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimHigherProvenItemKnown = $quickbarHintInventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimHigherProvenItemKnown
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimHigherProvenItemObjectId = $quickbarHintInventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimHigherProvenItemObjectId
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimHigherProvenItemDistance = $quickbarHintInventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimHigherProvenItemDistance
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimResult = $quickbarHintInventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimResult
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimEquipSlot = $quickbarHintInventoryEquipmentBridgeOutputLastDecisionServerInventoryClaimEquipSlot
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputLastDecisionClientGuiInventoryClaimKnown = $quickbarHintInventoryEquipmentBridgeOutputLastDecisionClientGuiInventoryClaimKnown
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputLastDecisionClientGuiInventoryClaimKind = $quickbarHintInventoryEquipmentBridgeOutputLastDecisionClientGuiInventoryClaimKind
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputLastDecisionClientGuiInventoryClaimObjectId = $quickbarHintInventoryEquipmentBridgeOutputLastDecisionClientGuiInventoryClaimObjectId
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputLastDecisionClientGuiInventoryClaimPanel = $quickbarHintInventoryEquipmentBridgeOutputLastDecisionClientGuiInventoryClaimPanel
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputLastDecisionClientGuiInventoryClaimPlayerInventoryGui = $quickbarHintInventoryEquipmentBridgeOutputLastDecisionClientGuiInventoryClaimPlayerInventoryGui
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputLastDecisionClientGuiInventoryClaimRewrittenSelfObjectId = $quickbarHintInventoryEquipmentBridgeOutputLastDecisionClientGuiInventoryClaimRewrittenSelfObjectId
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputClientGuiWriterPlanAction = $quickbarHintInventoryEquipmentBridgeOutputClientGuiWriterPlanAction
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputClientGuiWriterPlanEmissionEnabled = $quickbarHintInventoryEquipmentBridgeOutputClientGuiWriterPlanEmissionEnabled
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputClientGuiWriterPlanBlockedReason = $quickbarHintInventoryEquipmentBridgeOutputClientGuiWriterPlanBlockedReason
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputClientGuiWriterPlanPayloadAvailable = $quickbarHintInventoryEquipmentBridgeOutputClientGuiWriterPlanPayloadAvailable
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputClientGuiWriterPlanPayloadKind = $quickbarHintInventoryEquipmentBridgeOutputClientGuiWriterPlanPayloadKind
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputClientGuiWriterPlanPayloadHex = $quickbarHintInventoryEquipmentBridgeOutputClientGuiWriterPlanPayloadHex
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputClientGuiWriterPlanStatusObjectId = $quickbarHintInventoryEquipmentBridgeOutputClientGuiWriterPlanStatusObjectId
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputClientGuiWriterPlanStatusObjectIsCurrentPlayer = $quickbarHintInventoryEquipmentBridgeOutputClientGuiWriterPlanStatusObjectIsCurrentPlayer
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputClientGuiWriterPlanSelectPanel = $quickbarHintInventoryEquipmentBridgeOutputClientGuiWriterPlanSelectPanel
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputClientGuiWriterPlanPlayerInventoryGui = $quickbarHintInventoryEquipmentBridgeOutputClientGuiWriterPlanPlayerInventoryGui
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputLastDeferredClientGuiUpdateIndex = $quickbarHintInventoryEquipmentBridgeOutputLastDeferredClientGuiUpdateIndex
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputLastDeferredMissingClaimUpdateIndex = $quickbarHintInventoryEquipmentBridgeOutputLastDeferredMissingClaimUpdateIndex
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputLastBlockedCandidateMismatchUpdateIndex = $quickbarHintInventoryEquipmentBridgeOutputLastBlockedCandidateMismatchUpdateIndex
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputLastQueuedKnown = $quickbarHintInventoryEquipmentBridgeOutputLastQueuedKnown
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputLastQueuedUpdateIndex = $quickbarHintInventoryEquipmentBridgeOutputLastQueuedUpdateIndex
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputLastQueuedEmissionIndex = $quickbarHintInventoryEquipmentBridgeOutputLastQueuedEmissionIndex
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputLastQueuedEventIndex = $quickbarHintInventoryEquipmentBridgeOutputLastQueuedEventIndex
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputLastQueuedMinor = $quickbarHintInventoryEquipmentBridgeOutputLastQueuedMinor
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputLastQueuedObjectId = $quickbarHintInventoryEquipmentBridgeOutputLastQueuedObjectId
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputLastQueuedResult = $quickbarHintInventoryEquipmentBridgeOutputLastQueuedResult
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputLastQueuedEquipSlot = $quickbarHintInventoryEquipmentBridgeOutputLastQueuedEquipSlot
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputLastQueuedTriggerSequence = $quickbarHintInventoryEquipmentBridgeOutputLastQueuedTriggerSequence
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputLastQueuedSyntheticSequence = $quickbarHintInventoryEquipmentBridgeOutputLastQueuedSyntheticSequence
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputQueuedClientGuiStatusPackets = $quickbarHintInventoryEquipmentBridgeOutputQueuedClientGuiStatusPackets
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputLastQueuedClientGuiStatusKnown = $quickbarHintInventoryEquipmentBridgeOutputLastQueuedClientGuiStatusKnown
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputLastQueuedClientGuiStatusUpdateIndex = $quickbarHintInventoryEquipmentBridgeOutputLastQueuedClientGuiStatusUpdateIndex
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputLastQueuedClientGuiStatusEmissionIndex = $quickbarHintInventoryEquipmentBridgeOutputLastQueuedClientGuiStatusEmissionIndex
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputLastQueuedClientGuiStatusEventIndex = $quickbarHintInventoryEquipmentBridgeOutputLastQueuedClientGuiStatusEventIndex
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputLastQueuedClientGuiStatusObjectId = $quickbarHintInventoryEquipmentBridgeOutputLastQueuedClientGuiStatusObjectId
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputLastQueuedClientGuiStatusPlayerInventoryGui = $quickbarHintInventoryEquipmentBridgeOutputLastQueuedClientGuiStatusPlayerInventoryGui
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputLastQueuedClientGuiStatusPayloadHex = $quickbarHintInventoryEquipmentBridgeOutputLastQueuedClientGuiStatusPayloadHex
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputLastQueuedClientGuiStatusTriggerClientSequence = $quickbarHintInventoryEquipmentBridgeOutputLastQueuedClientGuiStatusTriggerClientSequence
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputLastQueuedClientGuiStatusSyntheticSequence = $quickbarHintInventoryEquipmentBridgeOutputLastQueuedClientGuiStatusSyntheticSequence
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputLastQueuedClientGuiStatusAckSequence = $quickbarHintInventoryEquipmentBridgeOutputLastQueuedClientGuiStatusAckSequence
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputLastQueuedClientGuiStatusCandidateKnown = $quickbarHintInventoryEquipmentBridgeOutputLastQueuedClientGuiStatusCandidateKnown
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputLastQueuedClientGuiStatusCandidateObjectId = $quickbarHintInventoryEquipmentBridgeOutputLastQueuedClientGuiStatusCandidateObjectId
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputLastQueuedClientGuiStatusCandidateProof = $quickbarHintInventoryEquipmentBridgeOutputLastQueuedClientGuiStatusCandidateProof
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputLastQueuedClientGuiStatusCandidateSource = $quickbarHintInventoryEquipmentBridgeOutputLastQueuedClientGuiStatusCandidateSource
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputLastQueuedClientGuiStatusReadyObjects = $quickbarHintInventoryEquipmentBridgeOutputLastQueuedClientGuiStatusReadyObjects
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputLastQueuedClientGuiStatusDeferredFeature25OnlyObjects = $quickbarHintInventoryEquipmentBridgeOutputLastQueuedClientGuiStatusDeferredFeature25OnlyObjects
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputClientGuiStatusResponseLiveObjectPackets = $quickbarHintInventoryEquipmentBridgeOutputClientGuiStatusResponseLiveObjectPackets
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputClientGuiStatusResponseLiveGuiRecordPackets = $quickbarHintInventoryEquipmentBridgeOutputClientGuiStatusResponseLiveGuiRecordPackets
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputClientGuiStatusResponseMaterializedItemPackets = $quickbarHintInventoryEquipmentBridgeOutputClientGuiStatusResponseMaterializedItemPackets
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputLastClientGuiStatusResponseKnown = $quickbarHintInventoryEquipmentBridgeOutputLastClientGuiStatusResponseKnown
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputLastClientGuiStatusResponseQueuedUpdateIndex = $quickbarHintInventoryEquipmentBridgeOutputLastClientGuiStatusResponseQueuedUpdateIndex
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputLastClientGuiStatusResponseServerSequence = $quickbarHintInventoryEquipmentBridgeOutputLastClientGuiStatusResponseServerSequence
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputLastClientGuiStatusResponseAckSequence = $quickbarHintInventoryEquipmentBridgeOutputLastClientGuiStatusResponseAckSequence
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputLastClientGuiStatusResponseLiveGuiRecords = $quickbarHintInventoryEquipmentBridgeOutputLastClientGuiStatusResponseLiveGuiRecords
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputLastClientGuiStatusResponseLiveGuiFragmentBits = $quickbarHintInventoryEquipmentBridgeOutputLastClientGuiStatusResponseLiveGuiFragmentBits
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputLastClientGuiStatusResponseMaterializedItemObjectIds = $quickbarHintInventoryEquipmentBridgeOutputLastClientGuiStatusResponseMaterializedItemObjectIds
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputLastClientGuiStatusResponseReadyObjects = $quickbarHintInventoryEquipmentBridgeOutputLastClientGuiStatusResponseReadyObjects
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputLastClientGuiStatusResponseCandidateKnown = $quickbarHintInventoryEquipmentBridgeOutputLastClientGuiStatusResponseCandidateKnown
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputLastClientGuiStatusResponseCandidateObjectId = $quickbarHintInventoryEquipmentBridgeOutputLastClientGuiStatusResponseCandidateObjectId
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputLastClientGuiStatusResponseCandidateProof = $quickbarHintInventoryEquipmentBridgeOutputLastClientGuiStatusResponseCandidateProof
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputLastClientGuiStatusResponseCandidateSource = $quickbarHintInventoryEquipmentBridgeOutputLastClientGuiStatusResponseCandidateSource
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputClientGuiStatusResponseOutcome = $quickbarHintInventoryEquipmentBridgeOutputClientGuiStatusResponseOutcome
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputBestClientGuiStatusResponseKnown = $quickbarHintInventoryEquipmentBridgeOutputBestClientGuiStatusResponseKnown
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputBestClientGuiStatusResponseQueuedUpdateIndex = $quickbarHintInventoryEquipmentBridgeOutputBestClientGuiStatusResponseQueuedUpdateIndex
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputBestClientGuiStatusResponseServerSequence = $quickbarHintInventoryEquipmentBridgeOutputBestClientGuiStatusResponseServerSequence
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputBestClientGuiStatusResponseAckSequence = $quickbarHintInventoryEquipmentBridgeOutputBestClientGuiStatusResponseAckSequence
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputBestClientGuiStatusResponseLiveGuiRecords = $quickbarHintInventoryEquipmentBridgeOutputBestClientGuiStatusResponseLiveGuiRecords
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputBestClientGuiStatusResponseLiveGuiFragmentBits = $quickbarHintInventoryEquipmentBridgeOutputBestClientGuiStatusResponseLiveGuiFragmentBits
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputBestClientGuiStatusResponseMaterializedItemObjectIds = $quickbarHintInventoryEquipmentBridgeOutputBestClientGuiStatusResponseMaterializedItemObjectIds
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputBestClientGuiStatusResponseReadyObjects = $quickbarHintInventoryEquipmentBridgeOutputBestClientGuiStatusResponseReadyObjects
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputBestClientGuiStatusResponseCandidateKnown = $quickbarHintInventoryEquipmentBridgeOutputBestClientGuiStatusResponseCandidateKnown
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputBestClientGuiStatusResponseCandidateObjectId = $quickbarHintInventoryEquipmentBridgeOutputBestClientGuiStatusResponseCandidateObjectId
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputBestClientGuiStatusResponseCandidateProof = $quickbarHintInventoryEquipmentBridgeOutputBestClientGuiStatusResponseCandidateProof
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputBestClientGuiStatusResponseCandidateSource = $quickbarHintInventoryEquipmentBridgeOutputBestClientGuiStatusResponseCandidateSource
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputBestClientGuiStatusResponseAssociation = $quickbarHintInventoryEquipmentBridgeOutputBestClientGuiStatusResponseAssociation
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputBestClientGuiStatusResponseMatchesQueuedStatusCandidate = $quickbarHintInventoryEquipmentBridgeOutputBestClientGuiStatusResponseMatchesQueuedStatusCandidate
        QuickbarItemRefreshHintInventoryEquipmentBridgeOutputBestClientGuiStatusResponseCandidateDeltaFromQueuedStatusCandidate = $quickbarHintInventoryEquipmentBridgeOutputBestClientGuiStatusResponseCandidateDeltaFromQueuedStatusCandidate
        QuickbarItemRefreshHintCompactItemEmissionReadyObjects = $quickbarHintCompactItemEmissionReadyObjects
        QuickbarItemRefreshHintCompactItemEmissionDeferredFeature25OnlyObjects = $quickbarHintCompactItemEmissionDeferredFeature25OnlyObjects
        QuickbarItemRefreshHintStreamProbeCompactItemEmissionReadyObjects = $quickbarHintStreamProbeCompactItemEmissionReadyObjects
        QuickbarItemRefreshHintStreamProbeCompactItemEmissionDeferredFeature25OnlyObjects = $quickbarHintStreamProbeCompactItemEmissionDeferredFeature25OnlyObjects
        QuickbarItemRefreshHintInventoryFeature25FirstItemRefs = $quickbarHintInventoryFeature25FirstItemRefs
        QuickbarItemRefreshHintInventoryFeature25FirstItemRefMentions = $quickbarHintInventoryFeature25FirstItemRefMentions
        QuickbarItemRefreshHintInventoryFeature25FirstMaterializedItemRefMentions = $quickbarHintInventoryFeature25FirstMaterializedItemRefMentions
        QuickbarItemRefreshHintInventoryFeature25FirstDeferredItemRefMentions = $quickbarHintInventoryFeature25FirstDeferredItemRefMentions
        QuickbarItemRefreshHintInventoryFeature25SecondItemRefs = $quickbarHintInventoryFeature25SecondItemRefs
        QuickbarItemRefreshHintInventoryFeature25SecondItemRefMentions = $quickbarHintInventoryFeature25SecondItemRefMentions
        QuickbarItemRefreshHintInventoryFeature25SecondMaterializedItemRefMentions = $quickbarHintInventoryFeature25SecondMaterializedItemRefMentions
        QuickbarItemRefreshHintInventoryFeature25SecondDeferredItemRefMentions = $quickbarHintInventoryFeature25SecondDeferredItemRefMentions
        QuickbarItemRefreshHintInventoryFeature25LegacyTailItemRefs = $quickbarHintInventoryFeature25LegacyTailItemRefs
        QuickbarItemRefreshHintInventoryFeature25LegacyTailItemRefMentions = $quickbarHintInventoryFeature25LegacyTailItemRefMentions
        QuickbarItemRefreshHintInventoryFeature25LegacyTailMaterializedItemRefMentions = $quickbarHintInventoryFeature25LegacyTailMaterializedItemRefMentions
        QuickbarItemRefreshHintInventoryFeature25LegacyTailDeferredItemRefMentions = $quickbarHintInventoryFeature25LegacyTailDeferredItemRefMentions
        QuickbarItemRefreshHintClearedInventoryItemObjectIds = $quickbarHintClearedInventoryItemObjectIds
        QuickbarItemRefreshHintQuickbarItemUseCountStateRows = $quickbarHintQuickbarItemUseCountStateRows
        QuickbarItemRefreshHintQuickbarItemUseCountUpdatesObserved = $quickbarHintQuickbarItemUseCountUpdatesObserved
        QuickbarItemRefreshHintCandidateQuickbarItemUseCountStateKnown = $quickbarHintCandidateQuickbarItemUseCountStateKnown
        QuickbarItemRefreshHintCandidateQuickbarItemUseCountStateSlotRelation = $quickbarHintCandidateQuickbarItemUseCountStateSlotRelation
        QuickbarItemRefreshHintCandidateQuickbarItemUseCountStateSlotMatchesFirstPreservedActiveItem = $quickbarHintCandidateQuickbarItemUseCountStateSlotMatchesFirstPreservedActiveItem
        QuickbarItemRefreshHintCandidateQuickbarItemUseCountStateSlot = $quickbarHintCandidateQuickbarItemUseCountStateSlot
        QuickbarItemRefreshHintCandidateQuickbarItemUseCountStateButtonType = $quickbarHintCandidateQuickbarItemUseCountStateButtonType
        QuickbarItemRefreshHintCandidateQuickbarItemUseCountStateObjectId = $quickbarHintCandidateQuickbarItemUseCountStateObjectId
        QuickbarItemRefreshHintCandidateQuickbarItemUseCountStateActivePropertyIndex = $quickbarHintCandidateQuickbarItemUseCountStateActivePropertyIndex
        QuickbarItemRefreshHintCandidateQuickbarItemUseCountStateUseCount = $quickbarHintCandidateQuickbarItemUseCountStateUseCount
        QuickbarItemRefreshHintFirstPreservedActiveItemQuickbarUseCountStateKnown = $quickbarHintFirstPreservedActiveItemQuickbarUseCountStateKnown
        QuickbarItemRefreshHintFirstPreservedActiveItemQuickbarUseCountStateSlotRelation = $quickbarHintFirstPreservedActiveItemQuickbarUseCountStateSlotRelation
        QuickbarItemRefreshHintFirstPreservedActiveItemQuickbarUseCountStateSlotMatchesFirstPreservedActiveItem = $quickbarHintFirstPreservedActiveItemQuickbarUseCountStateSlotMatchesFirstPreservedActiveItem
        QuickbarItemRefreshHintFirstPreservedActiveItemQuickbarUseCountStateSlot = $quickbarHintFirstPreservedActiveItemQuickbarUseCountStateSlot
        QuickbarItemRefreshHintFirstPreservedActiveItemQuickbarUseCountStateButtonType = $quickbarHintFirstPreservedActiveItemQuickbarUseCountStateButtonType
        QuickbarItemRefreshHintFirstPreservedActiveItemQuickbarUseCountStateObjectId = $quickbarHintFirstPreservedActiveItemQuickbarUseCountStateObjectId
        QuickbarItemRefreshHintFirstPreservedActiveItemQuickbarUseCountStateActivePropertyIndex = $quickbarHintFirstPreservedActiveItemQuickbarUseCountStateActivePropertyIndex
        QuickbarItemRefreshHintFirstPreservedActiveItemQuickbarUseCountStateUseCount = $quickbarHintFirstPreservedActiveItemQuickbarUseCountStateUseCount
        QuickbarItemRefreshHintFirstServerQuickbarItemUseCountCandidateRowKnown = $quickbarHintFirstServerQuickbarItemUseCountCandidateRowKnown
        QuickbarItemRefreshHintFirstServerQuickbarItemUseCountCandidateRowTiming = $quickbarHintFirstServerQuickbarItemUseCountCandidateRowTiming
        QuickbarItemRefreshHintFirstServerQuickbarItemUseCountCandidateRowSlotRelation = $quickbarHintFirstServerQuickbarItemUseCountCandidateRowSlotRelation
        QuickbarItemRefreshHintFirstServerQuickbarItemUseCountCandidateRowSlotMatchesFirstPreservedActiveItem = $quickbarHintFirstServerQuickbarItemUseCountCandidateRowSlotMatchesFirstPreservedActiveItem
        QuickbarItemRefreshHintFirstServerQuickbarItemUseCountCandidateRowSlot = $quickbarHintFirstServerQuickbarItemUseCountCandidateRowSlot
        QuickbarItemRefreshHintFirstServerQuickbarItemUseCountCandidateRowButtonType = $quickbarHintFirstServerQuickbarItemUseCountCandidateRowButtonType
        QuickbarItemRefreshHintFirstServerQuickbarItemUseCountCandidateRowObjectId = $quickbarHintFirstServerQuickbarItemUseCountCandidateRowObjectId
        QuickbarItemRefreshHintFirstServerQuickbarItemUseCountCandidateRowActivePropertyIndex = $quickbarHintFirstServerQuickbarItemUseCountCandidateRowActivePropertyIndex
        QuickbarItemRefreshHintFirstServerQuickbarItemUseCountCandidateRowUseCount = $quickbarHintFirstServerQuickbarItemUseCountCandidateRowUseCount
        QuickbarItemRefreshHintFirstServerQuickbarItemUseCountCandidateRowBeforeFirstClientActionKnown = $quickbarHintFirstServerQuickbarItemUseCountCandidateRowBeforeFirstClientActionKnown
        QuickbarItemRefreshHintFirstServerQuickbarItemUseCountCandidateRowBeforeFirstClientActionSlot = $quickbarHintFirstServerQuickbarItemUseCountCandidateRowBeforeFirstClientActionSlot
        QuickbarItemRefreshHintFirstServerQuickbarItemUseCountCandidateRowBeforeFirstClientActionButtonType = $quickbarHintFirstServerQuickbarItemUseCountCandidateRowBeforeFirstClientActionButtonType
        QuickbarItemRefreshHintFirstServerQuickbarItemUseCountCandidateRowBeforeFirstClientActionActivePropertyIndex = $quickbarHintFirstServerQuickbarItemUseCountCandidateRowBeforeFirstClientActionActivePropertyIndex
        QuickbarItemRefreshHintFirstServerQuickbarItemUseCountCandidateRowBeforeFirstClientActionUseCount = $quickbarHintFirstServerQuickbarItemUseCountCandidateRowBeforeFirstClientActionUseCount
        QuickbarItemRefreshHintFirstServerQuickbarItemUseCountCandidateRowAfterFirstClientActionKnown = $quickbarHintFirstServerQuickbarItemUseCountCandidateRowAfterFirstClientActionKnown
        QuickbarItemRefreshHintFirstServerQuickbarItemUseCountCandidateRowAfterFirstClientActionSlot = $quickbarHintFirstServerQuickbarItemUseCountCandidateRowAfterFirstClientActionSlot
        QuickbarItemRefreshHintFirstServerQuickbarItemUseCountCandidateRowAfterFirstClientActionButtonType = $quickbarHintFirstServerQuickbarItemUseCountCandidateRowAfterFirstClientActionButtonType
        QuickbarItemRefreshHintFirstServerQuickbarItemUseCountCandidateRowAfterFirstClientActionActivePropertyIndex = $quickbarHintFirstServerQuickbarItemUseCountCandidateRowAfterFirstClientActionActivePropertyIndex
        QuickbarItemRefreshHintFirstServerQuickbarItemUseCountCandidateRowAfterFirstClientActionUseCount = $quickbarHintFirstServerQuickbarItemUseCountCandidateRowAfterFirstClientActionUseCount
        QuickbarItemRefreshHintFirstClientActionTiming = $quickbarHintFirstClientActionTiming
        QuickbarItemRefreshHintFollowupEventsBeforeFirstClientAction = $quickbarHintFollowupEventsBeforeFirstClientAction
        QuickbarItemRefreshHintServerToClientEventsSincePendingRefresh = $quickbarHintServerToClientEventsSincePendingRefresh
        QuickbarItemRefreshHintClientToServerEventsSincePendingRefresh = $quickbarHintClientToServerEventsSincePendingRefresh
        QuickbarItemRefreshHintClientGuiEventEventsSincePendingRefresh = $quickbarHintClientGuiEventEventsSincePendingRefresh
        QuickbarItemRefreshHintServerQuickbarItemUseCountEventsSincePendingRefresh = $quickbarHintServerQuickbarItemUseCountEventsSincePendingRefresh
        QuickbarItemRefreshHintServerQuickbarItemUseCountRecordsSincePendingRefresh = $quickbarHintServerQuickbarItemUseCountRecordsSincePendingRefresh
        QuickbarItemRefreshHintServerQuickbarItemUseCountRowsSincePendingRefresh = $quickbarHintServerQuickbarItemUseCountRowsSincePendingRefresh
        QuickbarItemRefreshHintServerQuickbarItemUseCountCandidateRowsSincePendingRefresh = $quickbarHintServerQuickbarItemUseCountCandidateRowsSincePendingRefresh
        QuickbarItemRefreshHintServerActiveItemPropertyEventsSincePendingRefresh = $quickbarHintServerActiveItemPropertyEventsSincePendingRefresh
        QuickbarItemRefreshHintServerActiveItemPropertyUsesEventsSincePendingRefresh = $quickbarHintServerActiveItemPropertyUsesEventsSincePendingRefresh
        QuickbarItemRefreshHintServerActiveItemPropertyFullEventsSincePendingRefresh = $quickbarHintServerActiveItemPropertyFullEventsSincePendingRefresh
        QuickbarItemRefreshHintServerActiveItemPropertyCandidateEventsSincePendingRefresh = $quickbarHintServerActiveItemPropertyCandidateEventsSincePendingRefresh
        QuickbarItemRefreshHintServerActiveItemPropertyCandidateUsesEventsSincePendingRefresh = $quickbarHintServerActiveItemPropertyCandidateUsesEventsSincePendingRefresh
        QuickbarItemRefreshHintServerActiveItemPropertyCandidateFullEventsSincePendingRefresh = $quickbarHintServerActiveItemPropertyCandidateFullEventsSincePendingRefresh
        QuickbarItemRefreshHintServerActiveItemPropertyCandidateChangedUseCountRowsSincePendingRefresh = $quickbarHintServerActiveItemPropertyCandidateChangedUseCountRowsSincePendingRefresh
        QuickbarItemRefreshHintServerActiveItemPropertyCandidateFullPropertyRowsSincePendingRefresh = $quickbarHintServerActiveItemPropertyCandidateFullPropertyRowsSincePendingRefresh
        QuickbarItemRefreshHintFirstEventAfterClientAction = $quickbarHintFirstEventAfterClientAction
        QuickbarItemRefreshHintEventsAfterFirstClientAction = $quickbarHintEventsAfterFirstClientAction
        QuickbarItemRefreshHintServerToClientEventsAfterFirstClientAction = $quickbarHintServerToClientEventsAfterFirstClientAction
        QuickbarItemRefreshHintClientToServerEventsAfterFirstClientAction = $quickbarHintClientToServerEventsAfterFirstClientAction
        QuickbarItemRefreshHintLiveObjectEventsAfterFirstClientAction = $quickbarHintLiveObjectEventsAfterFirstClientAction
        QuickbarItemRefreshHintQuickbarEventsAfterFirstClientAction = $quickbarHintQuickbarEventsAfterFirstClientAction
        QuickbarItemRefreshHintServerQuickbarItemUseCountEventsAfterFirstClientAction = $quickbarHintServerQuickbarItemUseCountEventsAfterFirstClientAction
        QuickbarItemRefreshHintServerQuickbarItemUseCountRecordsAfterFirstClientAction = $quickbarHintServerQuickbarItemUseCountRecordsAfterFirstClientAction
        QuickbarItemRefreshHintServerQuickbarItemUseCountRowsAfterFirstClientAction = $quickbarHintServerQuickbarItemUseCountRowsAfterFirstClientAction
        QuickbarItemRefreshHintServerQuickbarItemUseCountCandidateRowsAfterFirstClientAction = $quickbarHintServerQuickbarItemUseCountCandidateRowsAfterFirstClientAction
        QuickbarItemRefreshHintServerActiveItemPropertyEventsAfterFirstClientAction = $quickbarHintServerActiveItemPropertyEventsAfterFirstClientAction
        QuickbarItemRefreshHintServerActiveItemPropertyUsesEventsAfterFirstClientAction = $quickbarHintServerActiveItemPropertyUsesEventsAfterFirstClientAction
        QuickbarItemRefreshHintServerActiveItemPropertyFullEventsAfterFirstClientAction = $quickbarHintServerActiveItemPropertyFullEventsAfterFirstClientAction
        QuickbarItemRefreshHintServerActiveItemPropertyCandidateEventsAfterFirstClientAction = $quickbarHintServerActiveItemPropertyCandidateEventsAfterFirstClientAction
        QuickbarItemRefreshHintServerActiveItemPropertyCandidateUsesEventsAfterFirstClientAction = $quickbarHintServerActiveItemPropertyCandidateUsesEventsAfterFirstClientAction
        QuickbarItemRefreshHintServerActiveItemPropertyCandidateFullEventsAfterFirstClientAction = $quickbarHintServerActiveItemPropertyCandidateFullEventsAfterFirstClientAction
        QuickbarItemRefreshHintServerActiveItemPropertyCandidateChangedUseCountRowsAfterFirstClientAction = $quickbarHintServerActiveItemPropertyCandidateChangedUseCountRowsAfterFirstClientAction
        QuickbarItemRefreshHintServerActiveItemPropertyCandidateFullPropertyRowsAfterFirstClientAction = $quickbarHintServerActiveItemPropertyCandidateFullPropertyRowsAfterFirstClientAction
        QuickbarItemRefreshHintInventoryEventsAfterFirstClientAction = $quickbarHintInventoryEventsAfterFirstClientAction
        QuickbarItemRefreshHintClientGuiEventEventsAfterFirstClientAction = $quickbarHintClientGuiEventEventsAfterFirstClientAction
        QuickbarItemRefreshHintOtherEventsAfterFirstClientAction = $quickbarHintOtherEventsAfterFirstClientAction
        QuarantineDirectory = $quarantineDir
        PacketFiles = $files.Count
        CapturePerspective = $capturePerspectiveResolved
        TimeoutSeconds = $TimeoutSeconds
        ProxyOutputWaitMilliseconds = $ProxyOutputWaitMilliseconds
        DrainReceiveTimeoutMilliseconds = $DrainReceiveTimeoutMilliseconds
        ProxyOutputWaitEvents = $proxyOutputWaitEvents
        ProxyOutputWaitTimeouts = $proxyOutputWaitTimeouts
        ClientPacketsSentToProxy = $clientPacketsSent
        ServerPacketsSentToProxy = $serverPacketsSent
        ServerPacketsSkippedBeforeEndpoint = $serverPacketsSkipped
        ProxyPacketsReceivedByDummyServer = $proxyPacketsReceivedByDummyServer
        ProxyPacketsReceivedByDummyClient = $proxyPacketsReceivedByDummyClient
        GeneratedClientAckControlFrames = $generatedClientAcks
        GeneratedClientAcksEnabled = $generateClientAcks
        SeedEeBnxiEnabled = $seedEeBnxiEnabled
        SeededEeBnxi = $seededEeBnxiSent
        SeededEeBnxiPlacement = $seedEeBnxiPlacement
        SeededEeBnxiBuild = if ($seedEeBnxiEnabled) { "$SeedEeBnxiMajor.$SeedEeBnxiMinor.$SeedEeBnxiRevision" } else { '<disabled>' }
        CapturedRecvMFrames = $capturedRecvMFrames
        CapturedRecvLiveObjectDirectFrames = $capturedRecvLiveObjectDirectFrames
        CapturedRecvAreaClientAreaDirectFrames = $capturedRecvAreaClientAreaDirectFrames
        ProxyLogLines = $proxyLogLineCount
        QuarantineFiles = Get-FileCount -Path $quarantineDir -Filter '*'
        QuarantineBinaryFiles = Get-FileCount -Path $quarantineDir -Filter '*.bin'
        QuarantineTextFiles = Get-FileCount -Path $quarantineDir -Filter '*.txt'
        QuarantineTsvFiles = Get-FileCount -Path $quarantineDir -Filter '*.tsv'
        StrictAllowDecisions = Get-TextMatchCount -Text $proxyLogText -Pattern 'strict translation decision .*action="allow"'
        StrictQuarantineDecisions = Get-TextMatchCount -Text $proxyLogText -Pattern 'strict translation decision .*action="quarantine"'
        SemanticQuarantineLogMatches = Get-TextMatchCount -Text $proxyLogText -Pattern 'semantic translation failed|quarantined'
        LiveObjectRewriteFailureMatches = Get-TextMatchCount -Text $proxyLogText -Pattern 'live_object_update_failure: Some|live-object item update cursor failure|live-object rewrite failure'
        LiveObjectExactShapeMatches = Get-TextMatchCount -Text $proxyLogText -Pattern 'live-object payload reached exact EE shape'
        LiveObjectExactRewriteMatches = Get-TextMatchCount -Text $proxyLogText -Pattern 'live-object payload reached exact EE shape'
        LiveObjectExactClaimMatches = Get-TextMatchCount -Text $proxyLogText -Pattern 'live-object payload accepted exact EE shape with lifecycle proof'
        QuickbarRewriteSummaryMatches = Get-QuickbarCommittedRewriteTraceCount -Text $proxyLogText
        QuickbarStreamProbeRewriteSummaryMatches = Get-QuickbarStreamProbeRewriteTraceCount -Text $proxyLogText
        QuickbarRegistryContextMatches = Get-QuickbarRegistryContextTraceCount -Text $proxyLogText
        QuickbarStreamProbeRegistryContextMatches = Get-QuickbarRegistryContextTraceCount -Text $proxyLogText -Committed $false
        QuickbarSemanticCommittedProfileMatches = Get-SemanticCommittedQuickbarProfileTraceCount -Text $proxyLogText
        QuickbarSemanticPriorItemContextKnown = Get-SemanticCommittedQuickbarProfileFlagCount -Text $proxyLogText -Field 'prior_item_context_known' -Value 'true'
        QuickbarSemanticBestItemContextKnown = Get-SemanticCommittedQuickbarProfileFlagCount -Text $proxyLogText -Field 'best_item_context_known' -Value 'true'
        QuickbarSemanticBestItemContextSourceCurrent = Get-SemanticCommittedQuickbarProfileStringFieldCount -Text $proxyLogText -Field 'best_item_context_source' -Value 'current'
        QuickbarSemanticBestItemContextSourcePrior = Get-SemanticCommittedQuickbarProfileStringFieldCount -Text $proxyLogText -Field 'best_item_context_source' -Value 'prior'
        QuickbarSemanticBestItemContextSourcePreviousPost = Get-SemanticCommittedQuickbarProfileStringFieldCount -Text $proxyLogText -Field 'best_item_context_source' -Value 'previous_post'
        QuickbarSemanticPendingItemRefreshBeforeCommit = Get-SemanticCommittedQuickbarProfileFlagCount -Text $proxyLogText -Field 'pending_item_refresh_before_commit' -Value 'true'
        QuickbarSemanticPendingItemRefreshBeforeCommitUpdates = Get-SemanticCommittedQuickbarProfileTraceFieldMax -Text $proxyLogText -Field 'pending_item_refresh_updates_before_commit'
        QuickbarSemanticPendingItemRefreshBeforeCommitEvents = Get-SemanticCommittedQuickbarProfileTraceFieldMax -Text $proxyLogText -Field 'pending_item_refresh_events_before_commit'
        QuickbarSemanticPendingItemRefreshBeforeCommitLiveObjectEvents = Get-SemanticCommittedQuickbarProfileTraceFieldMax -Text $proxyLogText -Field 'pending_item_refresh_live_object_events_before_commit'
        QuickbarSemanticPendingItemRefreshBeforeCommitQuickbarEvents = Get-SemanticCommittedQuickbarProfileTraceFieldMax -Text $proxyLogText -Field 'pending_item_refresh_quickbar_events_before_commit'
        QuickbarSemanticPendingItemRefreshBeforeCommitAreaEvents = Get-SemanticCommittedQuickbarProfileTraceFieldMax -Text $proxyLogText -Field 'pending_item_refresh_area_events_before_commit'
        QuickbarSemanticPendingItemRefreshBeforeCommitInventoryEvents = Get-SemanticCommittedQuickbarProfileTraceFieldMax -Text $proxyLogText -Field 'pending_item_refresh_inventory_events_before_commit'
        QuickbarSemanticPendingItemRefreshBeforeCommitClientInputEvents = Get-SemanticCommittedQuickbarProfileTraceFieldMax -Text $proxyLogText -Field 'pending_item_refresh_client_input_events_before_commit'
        QuickbarSemanticPendingItemRefreshBeforeCommitClientInputUseItemEvents = Get-SemanticCommittedQuickbarProfileTraceFieldMax -Text $proxyLogText -Field 'pending_item_refresh_client_input_use_item_events_before_commit'
        QuickbarSemanticPendingItemRefreshBeforeCommitClientInputUseObjectEvents = Get-SemanticCommittedQuickbarProfileTraceFieldMax -Text $proxyLogText -Field 'pending_item_refresh_client_input_use_object_events_before_commit'
        QuickbarSemanticPendingItemRefreshBeforeCommitClientInputChangeDoorStateEvents = Get-SemanticCommittedQuickbarProfileTraceFieldMax -Text $proxyLogText -Field 'pending_item_refresh_client_input_change_door_state_events_before_commit'
        QuickbarSemanticPendingItemRefreshBeforeCommitClientInputOtherEvents = Get-SemanticCommittedQuickbarProfileTraceFieldMax -Text $proxyLogText -Field 'pending_item_refresh_client_input_other_events_before_commit'
        QuickbarSemanticPendingItemRefreshBeforeCommitClientGuiEventEvents = Get-SemanticCommittedQuickbarProfileTraceFieldMax -Text $proxyLogText -Field 'pending_item_refresh_client_gui_event_events_before_commit'
        QuickbarSemanticPendingItemRefreshBeforeCommitClientGuiEventEventsAfterFirstClientAction = Get-SemanticCommittedQuickbarProfileTraceFieldMax -Text $proxyLogText -Field 'pending_item_refresh_client_gui_event_events_after_first_client_action_before_commit'
        QuickbarSemanticPendingItemRefreshBeforeCommitClientQuickbarEvents = Get-SemanticCommittedQuickbarProfileTraceFieldMax -Text $proxyLogText -Field 'pending_item_refresh_client_quickbar_events_before_commit'
        QuickbarSemanticPendingItemRefreshBeforeCommitClientQuickbarItemSetButtonEvents = Get-SemanticCommittedQuickbarProfileTraceFieldMax -Text $proxyLogText -Field 'pending_item_refresh_client_quickbar_item_set_button_events_before_commit'
        QuickbarSemanticPendingItemRefreshBeforeCommitClientQuickbarOtherSetButtonEvents = Get-SemanticCommittedQuickbarProfileTraceFieldMax -Text $proxyLogText -Field 'pending_item_refresh_client_quickbar_other_set_button_events_before_commit'
        QuickbarSemanticPendingItemRefreshBeforeCommitChatEvents = Get-SemanticCommittedQuickbarProfileTraceFieldMax -Text $proxyLogText -Field 'pending_item_refresh_chat_events_before_commit'
        QuickbarSemanticPendingItemRefreshBeforeCommitOtherEvents = Get-SemanticCommittedQuickbarProfileTraceFieldMax -Text $proxyLogText -Field 'pending_item_refresh_other_events_before_commit'
        QuickbarSemanticPendingItemRefreshProofClassNone = Get-SemanticCommittedQuickbarProfileStringFieldCount -Text $proxyLogText -Field 'pending_item_refresh_proof_class' -Value 'none'
        QuickbarSemanticPendingItemRefreshProofClassDirectOnly = Get-SemanticCommittedQuickbarProfileStringFieldCount -Text $proxyLogText -Field 'pending_item_refresh_proof_class' -Value 'direct_only'
        QuickbarSemanticPendingItemRefreshProofClassFeature25Only = Get-SemanticCommittedQuickbarProfileStringFieldCount -Text $proxyLogText -Field 'pending_item_refresh_proof_class' -Value 'feature25_only'
        QuickbarSemanticPendingItemRefreshProofClassShared = Get-SemanticCommittedQuickbarProfileStringFieldCount -Text $proxyLogText -Field 'pending_item_refresh_proof_class' -Value 'shared'
        QuickbarSemanticPendingItemRefreshProofClassMixed = Get-SemanticCommittedQuickbarProfileStringFieldCount -Text $proxyLogText -Field 'pending_item_refresh_proof_class' -Value 'mixed'
        QuickbarSemanticPendingItemRefreshOutcomeNoPending = Get-SemanticCommittedQuickbarProfileStringFieldCount -Text $proxyLogText -Field 'pending_item_refresh_outcome' -Value 'no_pending_refresh'
        QuickbarSemanticPendingItemRefreshOutcomeStillBlank = Get-SemanticCommittedQuickbarProfileStringFieldCount -Text $proxyLogText -Field 'pending_item_refresh_outcome' -Value 'pending_refresh_still_blank'
        QuickbarSemanticPendingItemRefreshOutcomeEmittedItemSlots = Get-SemanticCommittedQuickbarProfileStringFieldCount -Text $proxyLogText -Field 'pending_item_refresh_outcome' -Value 'pending_refresh_emitted_item_slots'
        QuickbarSemanticPendingItemRefreshOutcomeObservedUseCountRows = Get-SemanticCommittedQuickbarProfileStringFieldCount -Text $proxyLogText -Field 'pending_item_refresh_outcome' -Value 'pending_refresh_observed_use_count_rows'
        QuickbarSemanticPendingItemRefreshOutcomeResolvedByUseCountState = Get-SemanticCommittedQuickbarProfileStringFieldCount -Text $proxyLogText -Field 'pending_item_refresh_outcome' -Value 'pending_refresh_resolved_by_use_count_state'
        QuickbarSemanticPendingItemRefreshFirstFollowupLiveObject = Get-SemanticCommittedQuickbarProfileStringFieldCount -Text $proxyLogText -Field 'pending_item_refresh_first_followup_event' -Value 'live_object'
        QuickbarSemanticPendingItemRefreshFirstFollowupInventory = Get-SemanticCommittedQuickbarProfileStringFieldCount -Text $proxyLogText -Field 'pending_item_refresh_first_followup_event' -Value 'inventory'
        QuickbarSemanticPendingItemRefreshFirstFollowupClientInputUseItem = Get-SemanticCommittedQuickbarProfileStringFieldCount -Text $proxyLogText -Field 'pending_item_refresh_first_followup_event' -Value 'client_input_use_item'
        QuickbarSemanticPendingItemRefreshFirstFollowupClientInputOther = Get-SemanticCommittedQuickbarProfileStringFieldCount -Text $proxyLogText -Field 'pending_item_refresh_first_followup_event' -Value 'client_input_other'
        QuickbarSemanticPendingItemRefreshFirstFollowupClientGuiEventNotify = Get-SemanticCommittedQuickbarProfileStringFieldCount -Text $proxyLogText -Field 'pending_item_refresh_first_followup_event' -Value 'client_gui_event_notify'
        QuickbarSemanticPendingItemRefreshFirstFollowupClientQuickbarItemSetButton = Get-SemanticCommittedQuickbarProfileStringFieldCount -Text $proxyLogText -Field 'pending_item_refresh_first_followup_event' -Value 'client_quickbar_item_set_button'
        QuickbarSemanticPendingItemRefreshFirstFollowupClientQuickbarOtherSetButton = Get-SemanticCommittedQuickbarProfileStringFieldCount -Text $proxyLogText -Field 'pending_item_refresh_first_followup_event' -Value 'client_quickbar_other_set_button'
        QuickbarSemanticPendingItemRefreshFirstClientActionNone = Get-SemanticCommittedQuickbarProfileStringFieldCount -Text $proxyLogText -Field 'pending_item_refresh_first_client_action' -Value 'none'
        QuickbarSemanticPendingItemRefreshFirstClientActionUseItem = Get-SemanticCommittedQuickbarProfileStringFieldCount -Text $proxyLogText -Field 'pending_item_refresh_first_client_action' -Value 'client_input_use_item'
        QuickbarSemanticPendingItemRefreshFirstClientActionOtherInput = Get-SemanticCommittedQuickbarProfileStringFieldCount -Text $proxyLogText -Field 'pending_item_refresh_first_client_action' -Value 'client_input_other'
        QuickbarSemanticPendingItemRefreshFirstClientActionClientGuiEventNotify = Get-SemanticCommittedQuickbarProfileStringFieldCount -Text $proxyLogText -Field 'pending_item_refresh_first_client_action' -Value 'client_gui_event_notify'
        QuickbarSemanticPendingItemRefreshFirstClientActionItemSetButton = Get-SemanticCommittedQuickbarProfileStringFieldCount -Text $proxyLogText -Field 'pending_item_refresh_first_client_action' -Value 'client_quickbar_item_set_button'
        QuickbarSemanticPendingItemRefreshFirstClientActionOtherSetButton = Get-SemanticCommittedQuickbarProfileStringFieldCount -Text $proxyLogText -Field 'pending_item_refresh_first_client_action' -Value 'client_quickbar_other_set_button'
        QuickbarSemanticPendingItemRefreshFirstClientActionHasObjectId = Get-SemanticCommittedQuickbarProfileFlagCount -Text $proxyLogText -Field 'pending_item_refresh_first_client_action_has_object_id' -Value 'true'
        QuickbarSemanticPendingItemRefreshFirstClientActionObjectId = Get-SemanticCommittedQuickbarProfileTraceFieldMax -Text $proxyLogText -Field 'pending_item_refresh_first_client_action_object_id'
        QuickbarSemanticPendingItemRefreshFirstClientActionSlot = Get-SemanticCommittedQuickbarProfileTraceFieldMax -Text $proxyLogText -Field 'pending_item_refresh_first_client_action_slot'
        QuickbarSemanticPendingItemRefreshFirstClientActionButtonType = Get-SemanticCommittedQuickbarProfileTraceFieldMax -Text $proxyLogText -Field 'pending_item_refresh_first_client_action_button_type'
        QuickbarSemanticPendingItemRefreshFirstClientActionBodyKindNone = Get-SemanticCommittedQuickbarProfileStringFieldCount -Text $proxyLogText -Field 'pending_item_refresh_first_client_action_body_kind' -Value 'none'
        QuickbarSemanticPendingItemRefreshFirstClientActionBodyKindItem = Get-SemanticCommittedQuickbarProfileStringFieldCount -Text $proxyLogText -Field 'pending_item_refresh_first_client_action_body_kind' -Value 'item'
        QuickbarSemanticPendingItemRefreshFirstClientActionCandidateKnown = Get-SemanticCommittedQuickbarProfileFlagCount -Text $proxyLogText -Field 'pending_item_refresh_first_client_action_candidate_known' -Value 'true'
        QuickbarSemanticPendingItemRefreshFirstClientActionCandidateObjectId = Get-SemanticCommittedQuickbarProfileTraceFieldMax -Text $proxyLogText -Field 'pending_item_refresh_first_client_action_candidate_object_id'
        QuickbarSemanticPendingItemRefreshFirstClientActionMatchesCandidate = Get-SemanticCommittedQuickbarProfileFlagCount -Text $proxyLogText -Field 'pending_item_refresh_first_client_action_matches_candidate' -Value 'true'
        QuickbarSemanticPendingItemRefreshCandidateBeforeCommitKnown = Get-SemanticCommittedQuickbarProfileFlagCount -Text $proxyLogText -Field 'pending_item_refresh_candidate_known_before_commit' -Value 'true'
        QuickbarSemanticPendingItemRefreshCandidateBeforeCommitObjectId = Get-SemanticCommittedQuickbarProfileTraceFieldMax -Text $proxyLogText -Field 'pending_item_refresh_candidate_object_id_before_commit'
        QuickbarSemanticPendingItemRefreshCandidateBeforeCommitSourceDirectOnly = Get-SemanticCommittedQuickbarProfileStringFieldCount -Text $proxyLogText -Field 'pending_item_refresh_candidate_source_before_commit' -Value 'direct_only'
        QuickbarSemanticPendingItemRefreshCandidateBeforeCommitSourceShared = Get-SemanticCommittedQuickbarProfileStringFieldCount -Text $proxyLogText -Field 'pending_item_refresh_candidate_source_before_commit' -Value 'shared'
        QuickbarSemanticPendingItemRefreshCandidateBeforeCommitSourceFeature25Only = Get-SemanticCommittedQuickbarProfileStringFieldCount -Text $proxyLogText -Field 'pending_item_refresh_candidate_source_before_commit' -Value 'feature25_only'
        QuickbarSemanticPendingItemRefreshCandidateBeforeCommitProofActiveObject = Get-SemanticCommittedQuickbarProfileStringFieldCount -Text $proxyLogText -Field 'pending_item_refresh_candidate_proof_before_commit' -Value 'active_object'
        QuickbarSemanticPendingItemRefreshCandidateBeforeCommitProofFeature25FirstList = Get-SemanticCommittedQuickbarProfileStringFieldCount -Text $proxyLogText -Field 'pending_item_refresh_candidate_proof_before_commit' -Value 'feature25_first_list'
        QuickbarSemanticPendingItemRefreshCandidateBeforeCommitProofFeature25SecondList = Get-SemanticCommittedQuickbarProfileStringFieldCount -Text $proxyLogText -Field 'pending_item_refresh_candidate_proof_before_commit' -Value 'feature25_second_list'
        QuickbarSemanticPendingItemRefreshCandidateBeforeCommitProofFeature25LegacyTail = Get-SemanticCommittedQuickbarProfileStringFieldCount -Text $proxyLogText -Field 'pending_item_refresh_candidate_proof_before_commit' -Value 'feature25_legacy_tail'
        QuickbarSemanticPostItemContextMatches = Get-SemanticPostQuickbarItemContextTraceCount -Text $proxyLogText
        QuickbarSemanticPostItemRefreshPending = Get-SemanticPostQuickbarItemContextFlagCount -Text $proxyLogText -Field 'pending_item_refresh' -Value 'true'
        QuickbarSemanticPostItemRefreshPendingUpdates = Get-SemanticPostQuickbarItemContextTraceFieldMax -Text $proxyLogText -Field 'pending_item_refresh_updates'
        QuickbarSemanticPostItemRefreshPendingEvents = Get-SemanticPostQuickbarItemContextTraceFieldMax -Text $proxyLogText -Field 'pending_item_refresh_events'
        QuickbarSemanticPostItemRefreshPendingServerToClientEvents = Get-SemanticPostQuickbarItemContextTraceFieldMax -Text $proxyLogText -Field 'pending_item_refresh_server_to_client_events'
        QuickbarSemanticPostItemRefreshPendingClientToServerEvents = Get-SemanticPostQuickbarItemContextTraceFieldMax -Text $proxyLogText -Field 'pending_item_refresh_client_to_server_events'
        QuickbarSemanticPostItemRefreshPendingLiveObjectEvents = Get-SemanticPostQuickbarItemContextTraceFieldMax -Text $proxyLogText -Field 'pending_item_refresh_live_object_events'
        QuickbarSemanticPostItemRefreshPendingQuickbarEvents = Get-SemanticPostQuickbarItemContextTraceFieldMax -Text $proxyLogText -Field 'pending_item_refresh_quickbar_events'
        QuickbarSemanticPostItemRefreshPendingAreaEvents = Get-SemanticPostQuickbarItemContextTraceFieldMax -Text $proxyLogText -Field 'pending_item_refresh_area_events'
        QuickbarSemanticPostItemRefreshPendingInventoryEvents = Get-SemanticPostQuickbarItemContextTraceFieldMax -Text $proxyLogText -Field 'pending_item_refresh_inventory_events'
        QuickbarSemanticPostItemRefreshPendingClientInputEvents = Get-SemanticPostQuickbarItemContextTraceFieldMax -Text $proxyLogText -Field 'pending_item_refresh_client_input_events'
        QuickbarSemanticPostItemRefreshPendingClientInputUseItemEvents = Get-SemanticPostQuickbarItemContextTraceFieldMax -Text $proxyLogText -Field 'pending_item_refresh_client_input_use_item_events'
        QuickbarSemanticPostItemRefreshPendingClientInputUseObjectEvents = Get-SemanticPostQuickbarItemContextTraceFieldMax -Text $proxyLogText -Field 'pending_item_refresh_client_input_use_object_events'
        QuickbarSemanticPostItemRefreshPendingClientInputChangeDoorStateEvents = Get-SemanticPostQuickbarItemContextTraceFieldMax -Text $proxyLogText -Field 'pending_item_refresh_client_input_change_door_state_events'
        QuickbarSemanticPostItemRefreshPendingClientInputOtherEvents = Get-SemanticPostQuickbarItemContextTraceFieldMax -Text $proxyLogText -Field 'pending_item_refresh_client_input_other_events'
        QuickbarSemanticPostItemRefreshPendingClientGuiEventEvents = Get-SemanticPostQuickbarItemContextTraceFieldMax -Text $proxyLogText -Field 'pending_item_refresh_client_gui_event_events'
        QuickbarSemanticPostItemRefreshPendingClientQuickbarEvents = Get-SemanticPostQuickbarItemContextTraceFieldMax -Text $proxyLogText -Field 'pending_item_refresh_client_quickbar_events'
        QuickbarSemanticPostItemRefreshPendingClientQuickbarItemSetButtonEvents = Get-SemanticPostQuickbarItemContextTraceFieldMax -Text $proxyLogText -Field 'pending_item_refresh_client_quickbar_item_set_button_events'
        QuickbarSemanticPostItemRefreshPendingClientQuickbarOtherSetButtonEvents = Get-SemanticPostQuickbarItemContextTraceFieldMax -Text $proxyLogText -Field 'pending_item_refresh_client_quickbar_other_set_button_events'
        QuickbarSemanticPostItemRefreshPendingChatEvents = Get-SemanticPostQuickbarItemContextTraceFieldMax -Text $proxyLogText -Field 'pending_item_refresh_chat_events'
        QuickbarSemanticPostItemRefreshPendingOtherEvents = Get-SemanticPostQuickbarItemContextTraceFieldMax -Text $proxyLogText -Field 'pending_item_refresh_other_events'
        QuickbarSemanticPostItemRefreshProofClassNone = Get-SemanticPostQuickbarItemContextStringFieldCount -Text $proxyLogText -Field 'pending_item_refresh_proof_class' -Value 'none'
        QuickbarSemanticPostItemRefreshProofClassDirectOnly = Get-SemanticPostQuickbarItemContextStringFieldCount -Text $proxyLogText -Field 'pending_item_refresh_proof_class' -Value 'direct_only'
        QuickbarSemanticPostItemRefreshProofClassFeature25Only = Get-SemanticPostQuickbarItemContextStringFieldCount -Text $proxyLogText -Field 'pending_item_refresh_proof_class' -Value 'feature25_only'
        QuickbarSemanticPostItemRefreshProofClassShared = Get-SemanticPostQuickbarItemContextStringFieldCount -Text $proxyLogText -Field 'pending_item_refresh_proof_class' -Value 'shared'
        QuickbarSemanticPostItemRefreshProofClassMixed = Get-SemanticPostQuickbarItemContextStringFieldCount -Text $proxyLogText -Field 'pending_item_refresh_proof_class' -Value 'mixed'
        QuickbarSemanticPostItemRefreshFirstFollowupLiveObject = Get-SemanticPostQuickbarItemContextStringFieldCount -Text $proxyLogText -Field 'pending_item_refresh_first_followup_event' -Value 'live_object'
        QuickbarSemanticPostItemRefreshFirstFollowupInventory = Get-SemanticPostQuickbarItemContextStringFieldCount -Text $proxyLogText -Field 'pending_item_refresh_first_followup_event' -Value 'inventory'
        QuickbarSemanticPostItemRefreshFirstFollowupClientInputUseItem = Get-SemanticPostQuickbarItemContextStringFieldCount -Text $proxyLogText -Field 'pending_item_refresh_first_followup_event' -Value 'client_input_use_item'
        QuickbarSemanticPostItemRefreshFirstFollowupClientInputOther = Get-SemanticPostQuickbarItemContextStringFieldCount -Text $proxyLogText -Field 'pending_item_refresh_first_followup_event' -Value 'client_input_other'
        QuickbarSemanticPostItemRefreshFirstFollowupClientGuiEventNotify = Get-SemanticPostQuickbarItemContextStringFieldCount -Text $proxyLogText -Field 'pending_item_refresh_first_followup_event' -Value 'client_gui_event_notify'
        QuickbarSemanticPostItemRefreshFirstFollowupClientQuickbarItemSetButton = Get-SemanticPostQuickbarItemContextStringFieldCount -Text $proxyLogText -Field 'pending_item_refresh_first_followup_event' -Value 'client_quickbar_item_set_button'
        QuickbarSemanticPostItemRefreshFirstFollowupClientQuickbarOtherSetButton = Get-SemanticPostQuickbarItemContextStringFieldCount -Text $proxyLogText -Field 'pending_item_refresh_first_followup_event' -Value 'client_quickbar_other_set_button'
        QuickbarSemanticPostItemRefreshFirstClientActionNone = Get-SemanticPostQuickbarItemContextStringFieldCount -Text $proxyLogText -Field 'pending_item_refresh_first_client_action' -Value 'none'
        QuickbarSemanticPostItemRefreshFirstClientActionUseItem = Get-SemanticPostQuickbarItemContextStringFieldCount -Text $proxyLogText -Field 'pending_item_refresh_first_client_action' -Value 'client_input_use_item'
        QuickbarSemanticPostItemRefreshFirstClientActionOtherInput = Get-SemanticPostQuickbarItemContextStringFieldCount -Text $proxyLogText -Field 'pending_item_refresh_first_client_action' -Value 'client_input_other'
        QuickbarSemanticPostItemRefreshFirstClientActionClientGuiEventNotify = Get-SemanticPostQuickbarItemContextStringFieldCount -Text $proxyLogText -Field 'pending_item_refresh_first_client_action' -Value 'client_gui_event_notify'
        QuickbarSemanticPostItemRefreshFirstClientActionItemSetButton = Get-SemanticPostQuickbarItemContextStringFieldCount -Text $proxyLogText -Field 'pending_item_refresh_first_client_action' -Value 'client_quickbar_item_set_button'
        QuickbarSemanticPostItemRefreshFirstClientActionOtherSetButton = Get-SemanticPostQuickbarItemContextStringFieldCount -Text $proxyLogText -Field 'pending_item_refresh_first_client_action' -Value 'client_quickbar_other_set_button'
        QuickbarSemanticPostItemRefreshFirstClientActionHasObjectId = Get-SemanticPostQuickbarItemContextFlagCount -Text $proxyLogText -Field 'pending_item_refresh_first_client_action_has_object_id' -Value 'true'
        QuickbarSemanticPostItemRefreshFirstClientActionObjectId = Get-SemanticPostQuickbarItemContextTraceFieldMax -Text $proxyLogText -Field 'pending_item_refresh_first_client_action_object_id'
        QuickbarSemanticPostItemRefreshFirstClientActionSlot = Get-SemanticPostQuickbarItemContextTraceFieldMax -Text $proxyLogText -Field 'pending_item_refresh_first_client_action_slot'
        QuickbarSemanticPostItemRefreshFirstClientActionButtonType = Get-SemanticPostQuickbarItemContextTraceFieldMax -Text $proxyLogText -Field 'pending_item_refresh_first_client_action_button_type'
        QuickbarSemanticPostItemRefreshFirstClientActionBodyKindNone = Get-SemanticPostQuickbarItemContextStringFieldCount -Text $proxyLogText -Field 'pending_item_refresh_first_client_action_body_kind' -Value 'none'
        QuickbarSemanticPostItemRefreshFirstClientActionBodyKindItem = Get-SemanticPostQuickbarItemContextStringFieldCount -Text $proxyLogText -Field 'pending_item_refresh_first_client_action_body_kind' -Value 'item'
        QuickbarSemanticPostItemRefreshFirstClientActionCandidateKnown = Get-SemanticPostQuickbarItemContextFlagCount -Text $proxyLogText -Field 'pending_item_refresh_first_client_action_candidate_known' -Value 'true'
        QuickbarSemanticPostItemRefreshFirstClientActionCandidateObjectId = Get-SemanticPostQuickbarItemContextTraceFieldMax -Text $proxyLogText -Field 'pending_item_refresh_first_client_action_candidate_object_id'
        QuickbarSemanticPostItemRefreshFirstClientActionMatchesCandidate = Get-SemanticPostQuickbarItemContextFlagCount -Text $proxyLogText -Field 'pending_item_refresh_first_client_action_matches_candidate' -Value 'true'
        QuickbarSemanticUnresolvedPendingItemRefresh = Get-SemanticUnresolvedQuickbarItemRefreshTraceCount -Text $proxyLogText
        QuickbarSemanticUnresolvedPendingItemRefreshUpdates = Get-SemanticUnresolvedQuickbarItemRefreshTraceFieldMax -Text $proxyLogText -Field 'updates_since_committed_quickbar'
        QuickbarSemanticUnresolvedPendingItemRefreshEvents = Get-SemanticUnresolvedQuickbarItemRefreshTraceFieldMax -Text $proxyLogText -Field 'events_since_pending_refresh'
        QuickbarSemanticUnresolvedPendingItemRefreshServerToClientEvents = Get-SemanticUnresolvedQuickbarItemRefreshTraceFieldMax -Text $proxyLogText -Field 'server_to_client_events_since_pending_refresh'
        QuickbarSemanticUnresolvedPendingItemRefreshClientToServerEvents = Get-SemanticUnresolvedQuickbarItemRefreshTraceFieldMax -Text $proxyLogText -Field 'client_to_server_events_since_pending_refresh'
        QuickbarSemanticUnresolvedPendingItemRefreshLiveObjectEvents = Get-SemanticUnresolvedQuickbarItemRefreshTraceFieldMax -Text $proxyLogText -Field 'live_object_events_since_pending_refresh'
        QuickbarSemanticUnresolvedPendingItemRefreshQuickbarEvents = Get-SemanticUnresolvedQuickbarItemRefreshTraceFieldMax -Text $proxyLogText -Field 'quickbar_events_since_pending_refresh'
        QuickbarSemanticUnresolvedPendingItemRefreshServerQuickbarItemUseCountEvents = Get-SemanticUnresolvedQuickbarItemRefreshTraceFieldMax -Text $proxyLogText -Field 'server_quickbar_item_use_count_events_since_pending_refresh'
        QuickbarSemanticUnresolvedPendingItemRefreshServerQuickbarItemUseCountRecords = Get-SemanticUnresolvedQuickbarItemRefreshTraceFieldMax -Text $proxyLogText -Field 'server_quickbar_item_use_count_records_since_pending_refresh'
        QuickbarSemanticUnresolvedPendingItemRefreshServerQuickbarItemUseCountRows = Get-SemanticUnresolvedQuickbarItemRefreshTraceFieldMax -Text $proxyLogText -Field 'server_quickbar_item_use_count_rows_since_pending_refresh'
        QuickbarSemanticUnresolvedPendingItemRefreshServerQuickbarItemUseCountCandidateRows = Get-SemanticUnresolvedQuickbarItemRefreshTraceFieldMax -Text $proxyLogText -Field 'server_quickbar_item_use_count_candidate_rows_since_pending_refresh'
        QuickbarSemanticUnresolvedPendingItemRefreshAreaEvents = Get-SemanticUnresolvedQuickbarItemRefreshTraceFieldMax -Text $proxyLogText -Field 'area_events_since_pending_refresh'
        QuickbarSemanticUnresolvedPendingItemRefreshInventoryEvents = Get-SemanticUnresolvedQuickbarItemRefreshTraceFieldMax -Text $proxyLogText -Field 'inventory_events_since_pending_refresh'
        QuickbarSemanticUnresolvedPendingItemRefreshClientInputEvents = Get-SemanticUnresolvedQuickbarItemRefreshTraceFieldMax -Text $proxyLogText -Field 'client_input_events_since_pending_refresh'
        QuickbarSemanticUnresolvedPendingItemRefreshClientInputUseItemEvents = Get-SemanticUnresolvedQuickbarItemRefreshTraceFieldMax -Text $proxyLogText -Field 'client_input_use_item_events_since_pending_refresh'
        QuickbarSemanticUnresolvedPendingItemRefreshClientInputUseObjectEvents = Get-SemanticUnresolvedQuickbarItemRefreshTraceFieldMax -Text $proxyLogText -Field 'client_input_use_object_events_since_pending_refresh'
        QuickbarSemanticUnresolvedPendingItemRefreshClientInputChangeDoorStateEvents = Get-SemanticUnresolvedQuickbarItemRefreshTraceFieldMax -Text $proxyLogText -Field 'client_input_change_door_state_events_since_pending_refresh'
        QuickbarSemanticUnresolvedPendingItemRefreshClientInputOtherEvents = Get-SemanticUnresolvedQuickbarItemRefreshTraceFieldMax -Text $proxyLogText -Field 'client_input_other_events_since_pending_refresh'
        QuickbarSemanticUnresolvedPendingItemRefreshClientGuiEventEvents = Get-SemanticUnresolvedQuickbarItemRefreshTraceFieldMax -Text $proxyLogText -Field 'client_gui_event_events_since_pending_refresh'
        QuickbarSemanticUnresolvedPendingItemRefreshClientQuickbarEvents = Get-SemanticUnresolvedQuickbarItemRefreshTraceFieldMax -Text $proxyLogText -Field 'client_quickbar_events_since_pending_refresh'
        QuickbarSemanticUnresolvedPendingItemRefreshClientQuickbarItemSetButtonEvents = Get-SemanticUnresolvedQuickbarItemRefreshTraceFieldMax -Text $proxyLogText -Field 'client_quickbar_item_set_button_events_since_pending_refresh'
        QuickbarSemanticUnresolvedPendingItemRefreshClientQuickbarOtherSetButtonEvents = Get-SemanticUnresolvedQuickbarItemRefreshTraceFieldMax -Text $proxyLogText -Field 'client_quickbar_other_set_button_events_since_pending_refresh'
        QuickbarSemanticUnresolvedPendingItemRefreshChatEvents = Get-SemanticUnresolvedQuickbarItemRefreshTraceFieldMax -Text $proxyLogText -Field 'chat_events_since_pending_refresh'
        QuickbarSemanticUnresolvedPendingItemRefreshOtherEvents = Get-SemanticUnresolvedQuickbarItemRefreshTraceFieldMax -Text $proxyLogText -Field 'other_events_since_pending_refresh'
        QuickbarSemanticUnresolvedPendingItemRefreshProofClassDirectOnly = Get-SemanticUnresolvedQuickbarItemRefreshStringFieldCount -Text $proxyLogText -Field 'pending_item_refresh_proof_class' -Value 'direct_only'
        QuickbarSemanticUnresolvedPendingItemRefreshProofClassFeature25Only = Get-SemanticUnresolvedQuickbarItemRefreshStringFieldCount -Text $proxyLogText -Field 'pending_item_refresh_proof_class' -Value 'feature25_only'
        QuickbarSemanticUnresolvedPendingItemRefreshProofClassShared = Get-SemanticUnresolvedQuickbarItemRefreshStringFieldCount -Text $proxyLogText -Field 'pending_item_refresh_proof_class' -Value 'shared'
        QuickbarSemanticUnresolvedPendingItemRefreshProofClassMixed = Get-SemanticUnresolvedQuickbarItemRefreshStringFieldCount -Text $proxyLogText -Field 'pending_item_refresh_proof_class' -Value 'mixed'
        QuickbarSemanticUnresolvedPendingItemRefreshCandidateKnown = Get-SemanticUnresolvedQuickbarItemRefreshFlagCount -Text $proxyLogText -Field 'compact_item_emission_candidate_known' -Value 'true'
        QuickbarSemanticUnresolvedPendingItemRefreshCandidateObjectId = Get-SemanticUnresolvedQuickbarItemRefreshTraceFieldMax -Text $proxyLogText -Field 'compact_item_emission_candidate_object_id'
        QuickbarSemanticUnresolvedPendingItemRefreshCandidateSourceDirectOnly = Get-SemanticUnresolvedQuickbarItemRefreshStringFieldCount -Text $proxyLogText -Field 'compact_item_emission_candidate_source' -Value 'direct_only'
        QuickbarSemanticUnresolvedPendingItemRefreshCandidateSourceShared = Get-SemanticUnresolvedQuickbarItemRefreshStringFieldCount -Text $proxyLogText -Field 'compact_item_emission_candidate_source' -Value 'shared'
        QuickbarSemanticUnresolvedPendingItemRefreshCandidateSourceFeature25Only = Get-SemanticUnresolvedQuickbarItemRefreshStringFieldCount -Text $proxyLogText -Field 'compact_item_emission_candidate_source' -Value 'feature25_only'
        QuickbarSemanticUnresolvedPendingItemRefreshCandidateProofActiveObject = Get-SemanticUnresolvedQuickbarItemRefreshStringFieldCount -Text $proxyLogText -Field 'compact_item_emission_candidate_proof' -Value 'active_object'
        QuickbarSemanticUnresolvedPendingItemRefreshCandidateProofFeature25FirstList = Get-SemanticUnresolvedQuickbarItemRefreshStringFieldCount -Text $proxyLogText -Field 'compact_item_emission_candidate_proof' -Value 'feature25_first_list'
        QuickbarSemanticUnresolvedPendingItemRefreshCandidateProofFeature25SecondList = Get-SemanticUnresolvedQuickbarItemRefreshStringFieldCount -Text $proxyLogText -Field 'compact_item_emission_candidate_proof' -Value 'feature25_second_list'
        QuickbarSemanticUnresolvedPendingItemRefreshCandidateProofFeature25LegacyTail = Get-SemanticUnresolvedQuickbarItemRefreshStringFieldCount -Text $proxyLogText -Field 'compact_item_emission_candidate_proof' -Value 'feature25_legacy_tail'
        QuickbarSemanticUnresolvedPendingItemRefreshFirstFollowupLiveObject = Get-SemanticUnresolvedQuickbarItemRefreshStringFieldCount -Text $proxyLogText -Field 'first_followup_event' -Value 'live_object'
        QuickbarSemanticUnresolvedPendingItemRefreshFirstFollowupInventory = Get-SemanticUnresolvedQuickbarItemRefreshStringFieldCount -Text $proxyLogText -Field 'first_followup_event' -Value 'inventory'
        QuickbarSemanticUnresolvedPendingItemRefreshFirstFollowupClientInputUseItem = Get-SemanticUnresolvedQuickbarItemRefreshStringFieldCount -Text $proxyLogText -Field 'first_followup_event' -Value 'client_input_use_item'
        QuickbarSemanticUnresolvedPendingItemRefreshFirstFollowupClientInputOther = Get-SemanticUnresolvedQuickbarItemRefreshStringFieldCount -Text $proxyLogText -Field 'first_followup_event' -Value 'client_input_other'
        QuickbarSemanticUnresolvedPendingItemRefreshFirstFollowupClientGuiEventNotify = Get-SemanticUnresolvedQuickbarItemRefreshStringFieldCount -Text $proxyLogText -Field 'first_followup_event' -Value 'client_gui_event_notify'
        QuickbarSemanticUnresolvedPendingItemRefreshFirstFollowupClientQuickbarItemSetButton = Get-SemanticUnresolvedQuickbarItemRefreshStringFieldCount -Text $proxyLogText -Field 'first_followup_event' -Value 'client_quickbar_item_set_button'
        QuickbarSemanticUnresolvedPendingItemRefreshFirstFollowupClientQuickbarOtherSetButton = Get-SemanticUnresolvedQuickbarItemRefreshStringFieldCount -Text $proxyLogText -Field 'first_followup_event' -Value 'client_quickbar_other_set_button'
        QuickbarSemanticUnresolvedPendingItemRefreshFirstClientActionNone = Get-SemanticUnresolvedQuickbarItemRefreshStringFieldCount -Text $proxyLogText -Field 'first_client_action' -Value 'none'
        QuickbarSemanticUnresolvedPendingItemRefreshFirstClientActionUseItem = Get-SemanticUnresolvedQuickbarItemRefreshStringFieldCount -Text $proxyLogText -Field 'first_client_action' -Value 'client_input_use_item'
        QuickbarSemanticUnresolvedPendingItemRefreshFirstClientActionOtherInput = Get-SemanticUnresolvedQuickbarItemRefreshStringFieldCount -Text $proxyLogText -Field 'first_client_action' -Value 'client_input_other'
        QuickbarSemanticUnresolvedPendingItemRefreshFirstClientActionClientGuiEventNotify = Get-SemanticUnresolvedQuickbarItemRefreshStringFieldCount -Text $proxyLogText -Field 'first_client_action' -Value 'client_gui_event_notify'
        QuickbarSemanticUnresolvedPendingItemRefreshFirstClientActionItemSetButton = Get-SemanticUnresolvedQuickbarItemRefreshStringFieldCount -Text $proxyLogText -Field 'first_client_action' -Value 'client_quickbar_item_set_button'
        QuickbarSemanticUnresolvedPendingItemRefreshFirstClientActionOtherSetButton = Get-SemanticUnresolvedQuickbarItemRefreshStringFieldCount -Text $proxyLogText -Field 'first_client_action' -Value 'client_quickbar_other_set_button'
        QuickbarSemanticUnresolvedPendingItemRefreshFirstClientActionHasObjectId = Get-SemanticUnresolvedQuickbarItemRefreshFlagCount -Text $proxyLogText -Field 'first_client_action_has_object_id' -Value 'true'
        QuickbarSemanticUnresolvedPendingItemRefreshFirstClientActionObjectId = Get-SemanticUnresolvedQuickbarItemRefreshTraceFieldMax -Text $proxyLogText -Field 'first_client_action_object_id'
        QuickbarSemanticUnresolvedPendingItemRefreshFirstClientActionSlot = Get-SemanticUnresolvedQuickbarItemRefreshTraceFieldMax -Text $proxyLogText -Field 'first_client_action_slot'
        QuickbarSemanticUnresolvedPendingItemRefreshFirstClientActionButtonType = Get-SemanticUnresolvedQuickbarItemRefreshTraceFieldMax -Text $proxyLogText -Field 'first_client_action_button_type'
        QuickbarSemanticUnresolvedPendingItemRefreshFirstClientActionBodyKindNone = Get-SemanticUnresolvedQuickbarItemRefreshStringFieldCount -Text $proxyLogText -Field 'first_client_action_body_kind' -Value 'none'
        QuickbarSemanticUnresolvedPendingItemRefreshFirstClientActionBodyKindItem = Get-SemanticUnresolvedQuickbarItemRefreshStringFieldCount -Text $proxyLogText -Field 'first_client_action_body_kind' -Value 'item'
        QuickbarSemanticUnresolvedPendingItemRefreshFirstClientActionCandidateKnown = Get-SemanticUnresolvedQuickbarItemRefreshFlagCount -Text $proxyLogText -Field 'first_client_action_candidate_known' -Value 'true'
        QuickbarSemanticUnresolvedPendingItemRefreshFirstClientActionCandidateObjectId = Get-SemanticUnresolvedQuickbarItemRefreshTraceFieldMax -Text $proxyLogText -Field 'first_client_action_candidate_object_id'
        QuickbarSemanticUnresolvedPendingItemRefreshFirstClientActionMatchesCandidate = Get-SemanticUnresolvedQuickbarItemRefreshFlagCount -Text $proxyLogText -Field 'first_client_action_matches_candidate' -Value 'true'
        QuickbarSemanticUnresolvedPendingItemRefreshFirstEventAfterClientActionLiveObject = Get-SemanticUnresolvedQuickbarItemRefreshStringFieldCount -Text $proxyLogText -Field 'first_event_after_client_action' -Value 'live_object'
        QuickbarSemanticUnresolvedPendingItemRefreshFirstEventAfterClientActionQuickbar = Get-SemanticUnresolvedQuickbarItemRefreshStringFieldCount -Text $proxyLogText -Field 'first_event_after_client_action' -Value 'server_quickbar'
        QuickbarSemanticUnresolvedPendingItemRefreshFirstEventAfterClientActionServerQuickbarItemUseCount = Get-SemanticUnresolvedQuickbarItemRefreshStringFieldCount -Text $proxyLogText -Field 'first_event_after_client_action' -Value 'server_quickbar_item_use_count'
        QuickbarSemanticUnresolvedPendingItemRefreshFirstEventAfterClientActionInventory = Get-SemanticUnresolvedQuickbarItemRefreshStringFieldCount -Text $proxyLogText -Field 'first_event_after_client_action' -Value 'inventory'
        QuickbarSemanticUnresolvedPendingItemRefreshFirstEventAfterClientActionOther = Get-SemanticUnresolvedQuickbarItemRefreshStringFieldCount -Text $proxyLogText -Field 'first_event_after_client_action' -Value 'other'
        QuickbarSemanticUnresolvedPendingItemRefreshEventsAfterFirstClientAction = Get-SemanticUnresolvedQuickbarItemRefreshTraceFieldMax -Text $proxyLogText -Field 'events_after_first_client_action'
        QuickbarSemanticUnresolvedPendingItemRefreshServerToClientEventsAfterFirstClientAction = Get-SemanticUnresolvedQuickbarItemRefreshTraceFieldMax -Text $proxyLogText -Field 'server_to_client_events_after_first_client_action'
        QuickbarSemanticUnresolvedPendingItemRefreshClientToServerEventsAfterFirstClientAction = Get-SemanticUnresolvedQuickbarItemRefreshTraceFieldMax -Text $proxyLogText -Field 'client_to_server_events_after_first_client_action'
        QuickbarSemanticUnresolvedPendingItemRefreshLiveObjectEventsAfterFirstClientAction = Get-SemanticUnresolvedQuickbarItemRefreshTraceFieldMax -Text $proxyLogText -Field 'live_object_events_after_first_client_action'
        QuickbarSemanticUnresolvedPendingItemRefreshQuickbarEventsAfterFirstClientAction = Get-SemanticUnresolvedQuickbarItemRefreshTraceFieldMax -Text $proxyLogText -Field 'quickbar_events_after_first_client_action'
        QuickbarSemanticUnresolvedPendingItemRefreshServerQuickbarItemUseCountEventsAfterFirstClientAction = Get-SemanticUnresolvedQuickbarItemRefreshTraceFieldMax -Text $proxyLogText -Field 'server_quickbar_item_use_count_events_after_first_client_action'
        QuickbarSemanticUnresolvedPendingItemRefreshServerQuickbarItemUseCountRecordsAfterFirstClientAction = Get-SemanticUnresolvedQuickbarItemRefreshTraceFieldMax -Text $proxyLogText -Field 'server_quickbar_item_use_count_records_after_first_client_action'
        QuickbarSemanticUnresolvedPendingItemRefreshServerQuickbarItemUseCountRowsAfterFirstClientAction = Get-SemanticUnresolvedQuickbarItemRefreshTraceFieldMax -Text $proxyLogText -Field 'server_quickbar_item_use_count_rows_after_first_client_action'
        QuickbarSemanticUnresolvedPendingItemRefreshServerQuickbarItemUseCountCandidateRowsAfterFirstClientAction = Get-SemanticUnresolvedQuickbarItemRefreshTraceFieldMax -Text $proxyLogText -Field 'server_quickbar_item_use_count_candidate_rows_after_first_client_action'
        QuickbarSemanticUnresolvedPendingItemRefreshInventoryEventsAfterFirstClientAction = Get-SemanticUnresolvedQuickbarItemRefreshTraceFieldMax -Text $proxyLogText -Field 'inventory_events_after_first_client_action'
        QuickbarSemanticUnresolvedPendingItemRefreshClientGuiEventEventsAfterFirstClientAction = Get-SemanticUnresolvedQuickbarItemRefreshTraceFieldMax -Text $proxyLogText -Field 'client_gui_event_events_after_first_client_action'
        QuickbarSemanticUnresolvedPendingItemRefreshOtherEventsAfterFirstClientAction = Get-SemanticUnresolvedQuickbarItemRefreshTraceFieldMax -Text $proxyLogText -Field 'other_events_after_first_client_action'
        QuickbarItemDecisionTraceMatches = Get-QuickbarCommittedItemDecisionTraceCount -Text $proxyLogText
        QuickbarStreamProbeItemDecisionTraceMatches = Get-QuickbarStreamProbeItemDecisionTraceCount -Text $proxyLogText
        QuickbarItemDecisionsAccepted = Get-QuickbarCommittedItemDecisionFlagCount -Text $proxyLogText -Field 'accepted' -Value 'true'
        QuickbarItemDecisionsRejected = Get-QuickbarCommittedItemDecisionFlagCount -Text $proxyLogText -Field 'accepted' -Value 'false'
        QuickbarStreamProbeItemDecisionsAccepted = Get-QuickbarStreamProbeItemDecisionFlagCount -Text $proxyLogText -Field 'accepted' -Value 'true'
        QuickbarStreamProbeItemDecisionsRejected = Get-QuickbarStreamProbeItemDecisionFlagCount -Text $proxyLogText -Field 'accepted' -Value 'false'
        QuickbarSlotRecordsOwned = Get-QuickbarRewriteTraceFieldSum -Text $proxyLogText -Field 'slot_records_owned'
        QuickbarStreamProbeMaxSlotRecordsOwned = Get-QuickbarRewriteTraceFieldMax -Text $proxyLogText -Field 'slot_records_owned' -Committed $false
        QuickbarStreamProbeItemButtonsSeen = Get-QuickbarRewriteTraceFieldSum -Text $proxyLogText -Field 'item_buttons_seen' -Committed $false
        QuickbarItemButtonsSeen = Get-QuickbarRewriteTraceFieldSum -Text $proxyLogText -Field 'item_buttons_seen'
        QuickbarItemButtonsSourceExplicit = Get-QuickbarRewriteTraceFieldSum -Text $proxyLogText -Field 'item_buttons_source_explicit'
        QuickbarItemButtonsSourceCompact = Get-QuickbarRewriteTraceFieldSum -Text $proxyLogText -Field 'item_buttons_source_compact'
        QuickbarItemButtonsSourceRecovered = Get-QuickbarRewriteTraceFieldSum -Text $proxyLogText -Field 'item_buttons_source_recovered'
        QuickbarItemButtonsPreserved = Get-QuickbarRewriteTraceFieldSum -Text $proxyLogText -Field 'item_buttons_preserved'
        QuickbarBlankButtonsSeen = Get-QuickbarRewriteTraceFieldSum -Text $proxyLogText -Field 'blank_buttons_seen'
        QuickbarSpellsPreserved = Get-QuickbarRewriteTraceFieldSum -Text $proxyLogText -Field 'spells_preserved'
        QuickbarGeneralButtonsPreserved = Get-QuickbarRewriteTraceFieldSum -Text $proxyLogText -Field 'general_buttons_preserved'
        QuickbarGeneralButtonsBlanked = Get-QuickbarRewriteTraceFieldSum -Text $proxyLogText -Field 'general_buttons_blanked'
        QuickbarItemButtonsBlanked = Get-QuickbarRewriteTraceFieldSum -Text $proxyLogText -Field 'item_buttons_blanked'
        QuickbarItemButtonsBlankedCandidate = Get-QuickbarRewriteTraceFieldSum -Text $proxyLogText -Field 'item_buttons_blanked_candidate'
        QuickbarUnsupportedButtonsBlanked = Get-QuickbarRewriteTraceFieldSum -Text $proxyLogText -Field 'unsupported_buttons_blanked'
        QuickbarItemButtonsRejectedRecoveredTypeTag = Get-QuickbarRewriteTraceFieldSum -Text $proxyLogText -Field 'item_buttons_rejected_recovered_type_tag'
        QuickbarItemButtonsRejectedMissingTypeSource = Get-QuickbarRewriteTraceFieldSum -Text $proxyLogText -Field 'item_buttons_rejected_missing_type_source'
        QuickbarItemButtonsRejectedNoPresentItem = Get-QuickbarRewriteTraceFieldSum -Text $proxyLogText -Field 'item_buttons_rejected_no_present_item'
        QuickbarItemButtonsRejectedInvalidObjectId = Get-QuickbarRewriteTraceFieldSum -Text $proxyLogText -Field 'item_buttons_rejected_invalid_object_id'
        QuickbarItemButtonsRejectedMissingActiveProperties = Get-QuickbarRewriteTraceFieldSum -Text $proxyLogText -Field 'item_buttons_rejected_missing_active_properties'
        QuickbarItemButtonsRejectedUnsupportedAppearanceType = Get-QuickbarRewriteTraceFieldSum -Text $proxyLogText -Field 'item_buttons_rejected_unsupported_appearance_type'
        QuickbarItemButtonsRejectedAppearanceShape = Get-QuickbarRewriteTraceFieldSum -Text $proxyLogText -Field 'item_buttons_rejected_appearance_shape'
        QuickbarItemButtonsRejectedMissingStateProof = Get-QuickbarRewriteTraceFieldSum -Text $proxyLogText -Field 'item_buttons_rejected_missing_state_proof'
        QuickbarItemButtonsRejectedMissingStateUnknown = Get-QuickbarRewriteTraceFieldSum -Text $proxyLogText -Field 'item_buttons_rejected_missing_state_unknown'
        QuickbarItemButtonsRejectedMissingStateClearedDelete = Get-QuickbarRewriteTraceFieldSum -Text $proxyLogText -Field 'item_buttons_rejected_missing_state_cleared_delete'
        QuickbarItemButtonsRejectedMissingStateClearedAreaReset = Get-QuickbarRewriteTraceFieldSum -Text $proxyLogText -Field 'item_buttons_rejected_missing_state_cleared_area_reset'
        QuickbarItemObjectsRejectedMissingStateProven = Get-QuickbarRewriteTraceFieldSum -Text $proxyLogText -Field 'item_objects_rejected_missing_state_proven'
        QuickbarItemObjectsRejectedMissingStateActive = Get-QuickbarRewriteTraceFieldSum -Text $proxyLogText -Field 'item_objects_rejected_missing_state_active'
        QuickbarItemObjectsRejectedMissingStateFeature25First = Get-QuickbarRewriteTraceFieldSum -Text $proxyLogText -Field 'item_objects_rejected_missing_state_feature25_first'
        QuickbarItemObjectsRejectedMissingStateFeature25Second = Get-QuickbarRewriteTraceFieldSum -Text $proxyLogText -Field 'item_objects_rejected_missing_state_feature25_second'
        QuickbarItemObjectsRejectedMissingStateFeature25LegacyTail = Get-QuickbarRewriteTraceFieldSum -Text $proxyLogText -Field 'item_objects_rejected_missing_state_feature25_legacy_tail'
        QuickbarItemObjectsRejectedMissingStateUnknown = Get-QuickbarRewriteTraceFieldSum -Text $proxyLogText -Field 'item_objects_rejected_missing_state_unknown'
        QuickbarItemObjectsRejectedMissingStateClearedDelete = Get-QuickbarRewriteTraceFieldSum -Text $proxyLogText -Field 'item_objects_rejected_missing_state_cleared_delete'
        QuickbarItemObjectsRejectedMissingStateClearedAreaReset = Get-QuickbarRewriteTraceFieldSum -Text $proxyLogText -Field 'item_objects_rejected_missing_state_cleared_area_reset'
        QuickbarItemObjectsPreservedByExplicitSelfMaterialization = Get-QuickbarRewriteTraceFieldSum -Text $proxyLogText -Field 'item_objects_preserved_by_explicit_self_materialization'
        QuickbarItemObjectsPreservedByActiveState = Get-QuickbarRewriteTraceFieldSum -Text $proxyLogText -Field 'item_objects_preserved_by_active_state'
        QuickbarItemObjectsPreservedByFeature25First = Get-QuickbarRewriteTraceFieldSum -Text $proxyLogText -Field 'item_objects_preserved_by_feature25_first'
        QuickbarItemObjectsPreservedByFeature25Second = Get-QuickbarRewriteTraceFieldSum -Text $proxyLogText -Field 'item_objects_preserved_by_feature25_second'
        QuickbarItemObjectsPreservedByFeature25LegacyTail = Get-QuickbarRewriteTraceFieldSum -Text $proxyLogText -Field 'item_objects_preserved_by_feature25_legacy_tail'
        QuickbarRegistryActiveItemObjects = Get-QuickbarRegistryContextTraceFieldMax -Text $proxyLogText -Field 'active_item_objects'
        QuickbarRegistryMaterializedItemObjects = Get-QuickbarRegistryContextTraceFieldMax -Text $proxyLogText -Field 'materialized_item_objects'
        QuickbarRegistryDirectItemProofObjects = Get-QuickbarRegistryContextTraceFieldMax -Text $proxyLogText -Field 'direct_item_proof_objects'
        QuickbarRegistryFeature25ItemProofObjects = Get-QuickbarRegistryContextTraceFieldMax -Text $proxyLogText -Field 'feature25_item_proof_objects'
        QuickbarRegistryCompactItemEmissionProofObjects = Get-QuickbarRegistryContextTraceFieldMax -Text $proxyLogText -Field 'compact_item_emission_proof_objects'
        QuickbarRegistryCompactItemEmissionReadyObjects = Get-QuickbarRegistryContextTraceFieldMax -Text $proxyLogText -Field 'compact_item_emission_ready_objects'
        QuickbarRegistryCompactItemEmissionDirectOnlyProofObjects = Get-QuickbarRegistryContextTraceFieldMax -Text $proxyLogText -Field 'compact_item_emission_direct_only_proof_objects'
        QuickbarRegistryCompactItemEmissionFeature25OnlyProofObjects = Get-QuickbarRegistryContextTraceFieldMax -Text $proxyLogText -Field 'compact_item_emission_feature25_only_proof_objects'
        QuickbarRegistryCompactItemEmissionSharedProofObjects = Get-QuickbarRegistryContextTraceFieldMax -Text $proxyLogText -Field 'compact_item_emission_shared_proof_objects'
        QuickbarRegistryCompactItemEmissionDeferredFeature25OnlyObjects = Get-QuickbarRegistryContextTraceFieldMax -Text $proxyLogText -Field 'compact_item_emission_deferred_feature25_only_objects'
        QuickbarRegistryFeature25FirstItemRefs = Get-QuickbarRegistryContextTraceFieldMax -Text $proxyLogText -Field 'inventory_feature25_first_item_refs'
        QuickbarRegistryFeature25SecondItemRefs = Get-QuickbarRegistryContextTraceFieldMax -Text $proxyLogText -Field 'inventory_feature25_second_item_refs'
        QuickbarRegistryFeature25LegacyTailItemRefs = Get-QuickbarRegistryContextTraceFieldMax -Text $proxyLogText -Field 'inventory_feature25_legacy_tail_item_refs'
        QuickbarRegistryClearedInventoryItemObjectIds = Get-QuickbarRegistryContextTraceFieldMax -Text $proxyLogText -Field 'cleared_inventory_item_object_ids'
        QuickbarRegistryFeature25ReferenceRecords = Get-QuickbarRegistryContextTraceFieldMax -Text $proxyLogText -Field 'inventory_feature25_reference_records'
        QuickbarRegistryFeature25FirstItemRefMentions = Get-QuickbarRegistryContextTraceFieldMax -Text $proxyLogText -Field 'inventory_feature25_first_item_ref_mentions'
        QuickbarRegistryFeature25SecondItemRefMentions = Get-QuickbarRegistryContextTraceFieldMax -Text $proxyLogText -Field 'inventory_feature25_second_item_ref_mentions'
        QuickbarRegistryFeature25LegacyTailItemRefMentions = Get-QuickbarRegistryContextTraceFieldMax -Text $proxyLogText -Field 'inventory_feature25_legacy_tail_item_ref_mentions'
        QuickbarRegistryFeature25FirstMaterializedItemRefMentions = Get-QuickbarRegistryContextTraceFieldMax -Text $proxyLogText -Field 'inventory_feature25_first_materialized_item_ref_mentions'
        QuickbarRegistryFeature25FirstDeferredItemRefMentions = Get-QuickbarRegistryContextTraceFieldMax -Text $proxyLogText -Field 'inventory_feature25_first_deferred_item_ref_mentions'
        QuickbarRegistryFeature25SecondMaterializedItemRefMentions = Get-QuickbarRegistryContextTraceFieldMax -Text $proxyLogText -Field 'inventory_feature25_second_materialized_item_ref_mentions'
        QuickbarRegistryFeature25SecondDeferredItemRefMentions = Get-QuickbarRegistryContextTraceFieldMax -Text $proxyLogText -Field 'inventory_feature25_second_deferred_item_ref_mentions'
        QuickbarRegistryFeature25LegacyTailMaterializedItemRefMentions = Get-QuickbarRegistryContextTraceFieldMax -Text $proxyLogText -Field 'inventory_feature25_legacy_tail_materialized_item_ref_mentions'
        QuickbarRegistryFeature25LegacyTailDeferredItemRefMentions = Get-QuickbarRegistryContextTraceFieldMax -Text $proxyLogText -Field 'inventory_feature25_legacy_tail_deferred_item_ref_mentions'
        QuickbarSemanticPriorDirectItemProofObjects = Get-SemanticCommittedQuickbarProfileTraceFieldMax -Text $proxyLogText -Field 'prior_direct_item_proof_objects'
        QuickbarSemanticPriorFeature25ItemProofObjects = Get-SemanticCommittedQuickbarProfileTraceFieldMax -Text $proxyLogText -Field 'prior_feature25_item_proof_objects'
        QuickbarSemanticPriorCompactItemEmissionProofObjects = Get-SemanticCommittedQuickbarProfileTraceFieldMax -Text $proxyLogText -Field 'prior_compact_item_emission_proof_objects'
        QuickbarSemanticPriorCompactItemEmissionDirectOnlyProofObjects = Get-SemanticCommittedQuickbarProfileTraceFieldMax -Text $proxyLogText -Field 'prior_compact_item_emission_direct_only_proof_objects'
        QuickbarSemanticPriorCompactItemEmissionFeature25OnlyProofObjects = Get-SemanticCommittedQuickbarProfileTraceFieldMax -Text $proxyLogText -Field 'prior_compact_item_emission_feature25_only_proof_objects'
        QuickbarSemanticPriorCompactItemEmissionSharedProofObjects = Get-SemanticCommittedQuickbarProfileTraceFieldMax -Text $proxyLogText -Field 'prior_compact_item_emission_shared_proof_objects'
        QuickbarSemanticPriorFeature25FirstItemRefs = Get-SemanticCommittedQuickbarProfileTraceFieldMax -Text $proxyLogText -Field 'prior_inventory_feature25_first_item_refs'
        QuickbarSemanticPriorFeature25SecondItemRefs = Get-SemanticCommittedQuickbarProfileTraceFieldMax -Text $proxyLogText -Field 'prior_inventory_feature25_second_item_refs'
        QuickbarSemanticPriorFeature25LegacyTailItemRefs = Get-SemanticCommittedQuickbarProfileTraceFieldMax -Text $proxyLogText -Field 'prior_inventory_feature25_legacy_tail_item_refs'
        QuickbarSemanticPriorClearedInventoryItemObjectIds = Get-SemanticCommittedQuickbarProfileTraceFieldMax -Text $proxyLogText -Field 'prior_cleared_inventory_item_object_ids'
        QuickbarSemanticPreviousPostItemContextKnown = Get-SemanticCommittedQuickbarProfileFlagCount -Text $proxyLogText -Field 'previous_post_item_context_known' -Value 'true'
        QuickbarSemanticPreviousPostContextUpdates = Get-SemanticCommittedQuickbarProfileTraceFieldMax -Text $proxyLogText -Field 'previous_post_context_updates'
        QuickbarSemanticPreviousPostDirectItemProofObjects = Get-SemanticCommittedQuickbarProfileTraceFieldMax -Text $proxyLogText -Field 'previous_post_direct_item_proof_objects'
        QuickbarSemanticPreviousPostFeature25ItemProofObjects = Get-SemanticCommittedQuickbarProfileTraceFieldMax -Text $proxyLogText -Field 'previous_post_feature25_item_proof_objects'
        QuickbarSemanticPreviousPostCompactItemEmissionProofObjects = Get-SemanticCommittedQuickbarProfileTraceFieldMax -Text $proxyLogText -Field 'previous_post_compact_item_emission_proof_objects'
        QuickbarSemanticPreviousPostCompactItemEmissionDirectOnlyProofObjects = Get-SemanticCommittedQuickbarProfileTraceFieldMax -Text $proxyLogText -Field 'previous_post_compact_item_emission_direct_only_proof_objects'
        QuickbarSemanticPreviousPostCompactItemEmissionFeature25OnlyProofObjects = Get-SemanticCommittedQuickbarProfileTraceFieldMax -Text $proxyLogText -Field 'previous_post_compact_item_emission_feature25_only_proof_objects'
        QuickbarSemanticPreviousPostCompactItemEmissionSharedProofObjects = Get-SemanticCommittedQuickbarProfileTraceFieldMax -Text $proxyLogText -Field 'previous_post_compact_item_emission_shared_proof_objects'
        QuickbarSemanticPreviousPostFeature25FirstItemRefs = Get-SemanticCommittedQuickbarProfileTraceFieldMax -Text $proxyLogText -Field 'previous_post_inventory_feature25_first_item_refs'
        QuickbarSemanticPreviousPostFeature25SecondItemRefs = Get-SemanticCommittedQuickbarProfileTraceFieldMax -Text $proxyLogText -Field 'previous_post_inventory_feature25_second_item_refs'
        QuickbarSemanticPreviousPostFeature25LegacyTailItemRefs = Get-SemanticCommittedQuickbarProfileTraceFieldMax -Text $proxyLogText -Field 'previous_post_inventory_feature25_legacy_tail_item_refs'
        QuickbarSemanticPreviousPostClearedInventoryItemObjectIds = Get-SemanticCommittedQuickbarProfileTraceFieldMax -Text $proxyLogText -Field 'previous_post_cleared_inventory_item_object_ids'
        QuickbarSemanticBestDirectItemProofObjects = Get-SemanticCommittedQuickbarProfileTraceFieldMax -Text $proxyLogText -Field 'best_direct_item_proof_objects'
        QuickbarSemanticBestFeature25ItemProofObjects = Get-SemanticCommittedQuickbarProfileTraceFieldMax -Text $proxyLogText -Field 'best_feature25_item_proof_objects'
        QuickbarSemanticBestCompactItemEmissionProofObjects = Get-SemanticCommittedQuickbarProfileTraceFieldMax -Text $proxyLogText -Field 'best_compact_item_emission_proof_objects'
        QuickbarSemanticBestCompactItemEmissionDirectOnlyProofObjects = Get-SemanticCommittedQuickbarProfileTraceFieldMax -Text $proxyLogText -Field 'best_compact_item_emission_direct_only_proof_objects'
        QuickbarSemanticBestCompactItemEmissionFeature25OnlyProofObjects = Get-SemanticCommittedQuickbarProfileTraceFieldMax -Text $proxyLogText -Field 'best_compact_item_emission_feature25_only_proof_objects'
        QuickbarSemanticBestCompactItemEmissionSharedProofObjects = Get-SemanticCommittedQuickbarProfileTraceFieldMax -Text $proxyLogText -Field 'best_compact_item_emission_shared_proof_objects'
        QuickbarSemanticBestFeature25FirstItemRefs = Get-SemanticCommittedQuickbarProfileTraceFieldMax -Text $proxyLogText -Field 'best_inventory_feature25_first_item_refs'
        QuickbarSemanticBestFeature25SecondItemRefs = Get-SemanticCommittedQuickbarProfileTraceFieldMax -Text $proxyLogText -Field 'best_inventory_feature25_second_item_refs'
        QuickbarSemanticBestFeature25LegacyTailItemRefs = Get-SemanticCommittedQuickbarProfileTraceFieldMax -Text $proxyLogText -Field 'best_inventory_feature25_legacy_tail_item_refs'
        QuickbarSemanticBestClearedInventoryItemObjectIds = Get-SemanticCommittedQuickbarProfileTraceFieldMax -Text $proxyLogText -Field 'best_cleared_inventory_item_object_ids'
        QuickbarSemanticPostContextUpdates = Get-SemanticPostQuickbarItemContextTraceFieldMax -Text $proxyLogText -Field 'updates_since_committed_quickbar'
        QuickbarSemanticPostDirectItemProofObjects = Get-SemanticPostQuickbarItemContextTraceFieldMax -Text $proxyLogText -Field 'direct_item_proof_objects'
        QuickbarSemanticPostFeature25ItemProofObjects = Get-SemanticPostQuickbarItemContextTraceFieldMax -Text $proxyLogText -Field 'feature25_item_proof_objects'
        QuickbarSemanticPostCompactItemEmissionProofObjects = Get-SemanticPostQuickbarItemContextTraceFieldMax -Text $proxyLogText -Field 'compact_item_emission_proof_objects'
        QuickbarSemanticPostCompactItemEmissionReadyObjects = Get-SemanticPostQuickbarItemContextTraceFieldMax -Text $proxyLogText -Field 'compact_item_emission_ready_objects'
        QuickbarSemanticPostCompactItemEmissionCandidateKnown = Get-SemanticPostQuickbarItemContextFlagCount -Text $proxyLogText -Field 'compact_item_emission_candidate_known' -Value 'true'
        QuickbarSemanticPostCompactItemEmissionCandidateObjectId = Get-SemanticPostQuickbarItemContextTraceFieldMax -Text $proxyLogText -Field 'compact_item_emission_candidate_object_id'
        QuickbarSemanticPostCompactItemEmissionCandidateSourceDirectOnly = Get-SemanticPostQuickbarItemContextStringFieldCount -Text $proxyLogText -Field 'compact_item_emission_candidate_source' -Value 'direct_only'
        QuickbarSemanticPostCompactItemEmissionCandidateSourceShared = Get-SemanticPostQuickbarItemContextStringFieldCount -Text $proxyLogText -Field 'compact_item_emission_candidate_source' -Value 'shared'
        QuickbarSemanticPostCompactItemEmissionCandidateSourceFeature25Only = Get-SemanticPostQuickbarItemContextStringFieldCount -Text $proxyLogText -Field 'compact_item_emission_candidate_source' -Value 'feature25_only'
        QuickbarSemanticPostCompactItemEmissionCandidateProofActiveObject = Get-SemanticPostQuickbarItemContextStringFieldCount -Text $proxyLogText -Field 'compact_item_emission_candidate_proof' -Value 'active_object'
        QuickbarSemanticPostCompactItemEmissionCandidateProofFeature25FirstList = Get-SemanticPostQuickbarItemContextStringFieldCount -Text $proxyLogText -Field 'compact_item_emission_candidate_proof' -Value 'feature25_first_list'
        QuickbarSemanticPostCompactItemEmissionCandidateProofFeature25SecondList = Get-SemanticPostQuickbarItemContextStringFieldCount -Text $proxyLogText -Field 'compact_item_emission_candidate_proof' -Value 'feature25_second_list'
        QuickbarSemanticPostCompactItemEmissionCandidateProofFeature25LegacyTail = Get-SemanticPostQuickbarItemContextStringFieldCount -Text $proxyLogText -Field 'compact_item_emission_candidate_proof' -Value 'feature25_legacy_tail'
        QuickbarSemanticPostCompactItemEmissionDirectOnlyProofObjects = Get-SemanticPostQuickbarItemContextTraceFieldMax -Text $proxyLogText -Field 'compact_item_emission_direct_only_proof_objects'
        QuickbarSemanticPostCompactItemEmissionFeature25OnlyProofObjects = Get-SemanticPostQuickbarItemContextTraceFieldMax -Text $proxyLogText -Field 'compact_item_emission_feature25_only_proof_objects'
        QuickbarSemanticPostCompactItemEmissionSharedProofObjects = Get-SemanticPostQuickbarItemContextTraceFieldMax -Text $proxyLogText -Field 'compact_item_emission_shared_proof_objects'
        QuickbarSemanticPostCompactItemEmissionDeferredFeature25OnlyObjects = Get-SemanticPostQuickbarItemContextTraceFieldMax -Text $proxyLogText -Field 'compact_item_emission_deferred_feature25_only_objects'
        QuickbarSemanticPostFeature25FirstItemRefs = Get-SemanticPostQuickbarItemContextTraceFieldMax -Text $proxyLogText -Field 'inventory_feature25_first_item_refs'
        QuickbarSemanticPostFeature25SecondItemRefs = Get-SemanticPostQuickbarItemContextTraceFieldMax -Text $proxyLogText -Field 'inventory_feature25_second_item_refs'
        QuickbarSemanticPostFeature25LegacyTailItemRefs = Get-SemanticPostQuickbarItemContextTraceFieldMax -Text $proxyLogText -Field 'inventory_feature25_legacy_tail_item_refs'
        QuickbarSemanticPostClearedInventoryItemObjectIds = Get-SemanticPostQuickbarItemContextTraceFieldMax -Text $proxyLogText -Field 'cleared_inventory_item_object_ids'
        QuickbarStreamProbeRegistryDirectItemProofObjects = Get-QuickbarRegistryContextTraceFieldMax -Text $proxyLogText -Field 'direct_item_proof_objects' -Committed $false
        QuickbarStreamProbeRegistryFeature25ItemProofObjects = Get-QuickbarRegistryContextTraceFieldMax -Text $proxyLogText -Field 'feature25_item_proof_objects' -Committed $false
        QuickbarStreamProbeRegistryCompactItemEmissionProofObjects = Get-QuickbarRegistryContextTraceFieldMax -Text $proxyLogText -Field 'compact_item_emission_proof_objects' -Committed $false
        QuickbarStreamProbeRegistryCompactItemEmissionReadyObjects = Get-QuickbarRegistryContextTraceFieldMax -Text $proxyLogText -Field 'compact_item_emission_ready_objects' -Committed $false
        QuickbarStreamProbeRegistryCompactItemEmissionDirectOnlyProofObjects = Get-QuickbarRegistryContextTraceFieldMax -Text $proxyLogText -Field 'compact_item_emission_direct_only_proof_objects' -Committed $false
        QuickbarStreamProbeRegistryCompactItemEmissionFeature25OnlyProofObjects = Get-QuickbarRegistryContextTraceFieldMax -Text $proxyLogText -Field 'compact_item_emission_feature25_only_proof_objects' -Committed $false
        QuickbarStreamProbeRegistryCompactItemEmissionSharedProofObjects = Get-QuickbarRegistryContextTraceFieldMax -Text $proxyLogText -Field 'compact_item_emission_shared_proof_objects' -Committed $false
        QuickbarStreamProbeRegistryCompactItemEmissionDeferredFeature25OnlyObjects = Get-QuickbarRegistryContextTraceFieldMax -Text $proxyLogText -Field 'compact_item_emission_deferred_feature25_only_objects' -Committed $false
        QuickbarStreamProbeRegistryFeature25FirstItemRefs = Get-QuickbarRegistryContextTraceFieldMax -Text $proxyLogText -Field 'inventory_feature25_first_item_refs' -Committed $false
        QuickbarStreamProbeRegistryFeature25SecondItemRefs = Get-QuickbarRegistryContextTraceFieldMax -Text $proxyLogText -Field 'inventory_feature25_second_item_refs' -Committed $false
        LiveObjectExactClaimCreatureMentions = Get-LiveObjectExactClaimTraceFieldSum -Text $proxyLogText -Field 'creature_mentions'
        LiveObjectExactClaimCreatureUpdateMentions = Get-LiveObjectExactClaimTraceFieldSum -Text $proxyLogText -Field 'creature_update_mentions'
        LiveObjectExactClaimCreaturePositionMentions = Get-LiveObjectExactClaimTraceFieldSum -Text $proxyLogText -Field 'creature_position_mentions'
        LiveObjectExactClaimCreatureOrientationMentions = Get-LiveObjectExactClaimTraceFieldSum -Text $proxyLogText -Field 'creature_orientation_mentions'
        LiveObjectExactClaimCreatureUpdateClaimMentions = Get-LiveObjectExactClaimTraceFieldSum -Text $proxyLogText -Field 'creature_update_claim_mentions'
        LiveObjectExactClaimCreatureUpdateClaimPositionMentions = Get-LiveObjectExactClaimTraceFieldSum -Text $proxyLogText -Field 'creature_update_claim_position_mentions'
        LiveObjectExactClaimCreatureUpdateClaimScalarOrientationMentions = Get-LiveObjectExactClaimTraceFieldSum -Text $proxyLogText -Field 'creature_update_claim_scalar_orientation_mentions'
        LiveObjectExactClaimCreatureUpdateClaimVectorOrientationMentions = Get-LiveObjectExactClaimTraceFieldSum -Text $proxyLogText -Field 'creature_update_claim_vector_orientation_mentions'
        LiveObjectExactClaimItemMentions = Get-LiveObjectExactClaimTraceFieldSum -Text $proxyLogText -Field 'item_mentions'
        LiveObjectExactClaimTriggerMentions = Get-LiveObjectExactClaimTraceFieldSum -Text $proxyLogText -Field 'trigger_mentions'
        LiveObjectExactClaimPlaceableMentions = Get-LiveObjectExactClaimTraceFieldSum -Text $proxyLogText -Field 'placeable_mentions'
        LiveObjectExactClaimPlaceableAddMentions = Get-LiveObjectExactClaimTraceFieldSum -Text $proxyLogText -Field 'placeable_add_mentions'
        LiveObjectExactClaimPlaceableUpdateMentions = Get-LiveObjectExactClaimTraceFieldSum -Text $proxyLogText -Field 'placeable_update_mentions'
        LiveObjectExactClaimPlaceableDeleteMentions = Get-LiveObjectExactClaimTraceFieldSum -Text $proxyLogText -Field 'placeable_delete_mentions'
        LiveObjectExactClaimPlaceablePositionMentions = Get-LiveObjectExactClaimTraceFieldSum -Text $proxyLogText -Field 'placeable_position_mentions'
        LiveObjectExactClaimPlaceableOrientationMentions = Get-LiveObjectExactClaimTraceFieldSum -Text $proxyLogText -Field 'placeable_orientation_mentions'
        LiveObjectExactClaimPlaceableScalarOrientationMentions = Get-LiveObjectExactClaimTraceFieldSum -Text $proxyLogText -Field 'placeable_scalar_orientation_mentions'
        LiveObjectExactClaimPlaceableVectorOrientationMentions = Get-LiveObjectExactClaimTraceFieldSum -Text $proxyLogText -Field 'placeable_vector_orientation_mentions'
        LiveObjectExactClaimPlaceableNormalAppearanceMentions = Get-LiveObjectExactClaimTraceFieldSum -Text $proxyLogText -Field 'placeable_normal_appearance_mentions'
        LiveObjectExactClaimPlaceableCustomAppearanceMentions = Get-LiveObjectExactClaimTraceFieldSum -Text $proxyLogText -Field 'placeable_custom_appearance_mentions'
        LiveObjectExactClaimPlaceableAppearanceClaimMentions = Get-LiveObjectExactClaimTraceFieldSum -Text $proxyLogText -Field 'placeable_appearance_claim_mentions'
        LiveObjectExactClaimPlaceableFullStateMentions = Get-LiveObjectExactClaimTraceFieldSum -Text $proxyLogText -Field 'placeable_full_state_mentions'
        LiveObjectExactClaimPlaceablePartialStateMentions = Get-LiveObjectExactClaimTraceFieldSum -Text $proxyLogText -Field 'placeable_partial_state_mentions'
        LiveObjectExactClaimDoorMentions = Get-LiveObjectExactClaimTraceFieldSum -Text $proxyLogText -Field 'door_mentions'
        LiveObjectExactClaimUntypedMentions = Get-LiveObjectExactClaimTraceFieldSum -Text $proxyLogText -Field 'untyped_mentions'
        LiveObjectExactClaimInventoryOwnerClaimMentions = Get-LiveObjectExactClaimTraceFieldSum -Text $proxyLogText -Field 'inventory_owner_claim_mentions'
        LiveObjectExactClaimInventoryOwnerClaimFragmentBits = Get-LiveObjectExactClaimTraceFieldSum -Text $proxyLogText -Field 'inventory_owner_claim_fragment_bits'
        LiveObjectExactClaimInventoryOwnerSentinelMentions = Get-LiveObjectExactClaimTraceFieldSum -Text $proxyLogText -Field 'inventory_owner_sentinel_mentions'
        LiveObjectExactClaimInventoryOwnerCompactMentions = Get-LiveObjectExactClaimTraceFieldSum -Text $proxyLogText -Field 'inventory_owner_compact_mentions'
        LiveObjectExactClaimInventoryOwnerExternalMentions = Get-LiveObjectExactClaimTraceFieldSum -Text $proxyLogText -Field 'inventory_owner_external_mentions'
        LiveObjectExactClaimInventoryOwnerMask2a00Mentions = Get-LiveObjectExactClaimTraceFieldSum -Text $proxyLogText -Field 'inventory_owner_mask_2a00_mentions'
        LiveObjectExactClaimInventoryOwnerMask2e00Mentions = Get-LiveObjectExactClaimTraceFieldSum -Text $proxyLogText -Field 'inventory_owner_mask_2e00_mentions'
        LiveObjectExactClaimInventoryOwnerMask2e01Mentions = Get-LiveObjectExactClaimTraceFieldSum -Text $proxyLogText -Field 'inventory_owner_mask_2e01_mentions'
        LiveObjectExactClaimInventoryOwnerMask2000Mentions = Get-LiveObjectExactClaimTraceFieldSum -Text $proxyLogText -Field 'inventory_owner_mask_2000_mentions'
        LiveObjectExactClaimInventoryOwnerMaskD5ffMentions = Get-LiveObjectExactClaimTraceFieldSum -Text $proxyLogText -Field 'inventory_owner_mask_d5ff_mentions'
        LiveObjectExactClaimInventoryOwnerMaskOtherMentions = Get-LiveObjectExactClaimTraceFieldSum -Text $proxyLogText -Field 'inventory_owner_mask_other_mentions'
        LiveObjectExactClaimInventoryOwnerBranch0001Mentions = Get-LiveObjectExactClaimTraceFieldSum -Text $proxyLogText -Field 'inventory_owner_branch_0001_mentions'
        LiveObjectExactClaimInventoryOwnerBranch0002Mentions = Get-LiveObjectExactClaimTraceFieldSum -Text $proxyLogText -Field 'inventory_owner_branch_0002_mentions'
        LiveObjectExactClaimInventoryOwnerBranch0004IconListMentions = Get-LiveObjectExactClaimTraceFieldSum -Text $proxyLogText -Field 'inventory_owner_branch_0004_icon_list_mentions'
        LiveObjectExactClaimInventoryOwnerBranch0008Mentions = Get-LiveObjectExactClaimTraceFieldSum -Text $proxyLogText -Field 'inventory_owner_branch_0008_mentions'
        LiveObjectExactClaimInventoryOwnerBranch0010SimpleCategoryMentions = Get-LiveObjectExactClaimTraceFieldSum -Text $proxyLogText -Field 'inventory_owner_branch_0010_simple_category_mentions'
        LiveObjectExactClaimInventoryOwnerBranch0020RichCategoryMentions = Get-LiveObjectExactClaimTraceFieldSum -Text $proxyLogText -Field 'inventory_owner_branch_0020_rich_category_mentions'
        LiveObjectExactClaimInventoryOwnerBranch0040TenBitGroupMentions = Get-LiveObjectExactClaimTraceFieldSum -Text $proxyLogText -Field 'inventory_owner_branch_0040_ten_bit_group_mentions'
        LiveObjectExactClaimInventoryOwnerBranch0080TenBitGroupMentions = Get-LiveObjectExactClaimTraceFieldSum -Text $proxyLogText -Field 'inventory_owner_branch_0080_ten_bit_group_mentions'
        LiveObjectExactClaimInventoryOwnerBranch0100OpcodeStreamMentions = Get-LiveObjectExactClaimTraceFieldSum -Text $proxyLogText -Field 'inventory_owner_branch_0100_opcode_stream_mentions'
        LiveObjectExactClaimInventoryOwnerBranch0200Mentions = Get-LiveObjectExactClaimTraceFieldSum -Text $proxyLogText -Field 'inventory_owner_branch_0200_mentions'
        LiveObjectExactClaimInventoryOwnerBranch0400EquipmentDeltaMentions = Get-LiveObjectExactClaimTraceFieldSum -Text $proxyLogText -Field 'inventory_owner_branch_0400_equipment_delta_mentions'
        LiveObjectExactClaimInventoryOwnerBranch0800TailSelectorMentions = Get-LiveObjectExactClaimTraceFieldSum -Text $proxyLogText -Field 'inventory_owner_branch_0800_tail_selector_mentions'
        LiveObjectExactClaimInventoryOwnerBranch1000UiClearMentions = Get-LiveObjectExactClaimTraceFieldSum -Text $proxyLogText -Field 'inventory_owner_branch_1000_ui_clear_mentions'
        LiveObjectExactClaimInventoryOwnerBranch2000Feature25Mentions = Get-LiveObjectExactClaimTraceFieldSum -Text $proxyLogText -Field 'inventory_owner_branch_2000_feature25_mentions'
        LiveObjectExactClaimInventoryOwnerFeature25ClaimMentions = Get-LiveObjectExactClaimTraceFieldSum -Text $proxyLogText -Field 'inventory_owner_feature25_claim_mentions'
        LiveObjectExactClaimInventoryOwnerFeature25OwnerSentinelMentions = Get-LiveObjectExactClaimTraceFieldSum -Text $proxyLogText -Field 'inventory_owner_feature25_owner_sentinel_mentions'
        LiveObjectExactClaimInventoryOwnerFeature25OwnerCompactMentions = Get-LiveObjectExactClaimTraceFieldSum -Text $proxyLogText -Field 'inventory_owner_feature25_owner_compact_mentions'
        LiveObjectExactClaimInventoryOwnerFeature25OwnerExternalMentions = Get-LiveObjectExactClaimTraceFieldSum -Text $proxyLogText -Field 'inventory_owner_feature25_owner_external_mentions'
        LiveObjectExactClaimInventoryOwnerFeature25Mask2000Mentions = Get-LiveObjectExactClaimTraceFieldSum -Text $proxyLogText -Field 'inventory_owner_feature25_mask_2000_mentions'
        LiveObjectExactClaimInventoryOwnerFeature25Mask2a00Mentions = Get-LiveObjectExactClaimTraceFieldSum -Text $proxyLogText -Field 'inventory_owner_feature25_mask_2a00_mentions'
        LiveObjectExactClaimInventoryOwnerFeature25Mask2e00Mentions = Get-LiveObjectExactClaimTraceFieldSum -Text $proxyLogText -Field 'inventory_owner_feature25_mask_2e00_mentions'
        LiveObjectExactClaimInventoryOwnerFeature25Mask2e01Mentions = Get-LiveObjectExactClaimTraceFieldSum -Text $proxyLogText -Field 'inventory_owner_feature25_mask_2e01_mentions'
        LiveObjectExactClaimInventoryOwnerFeature25MaskOtherMentions = Get-LiveObjectExactClaimTraceFieldSum -Text $proxyLogText -Field 'inventory_owner_feature25_mask_other_mentions'
        LiveObjectExactClaimInventoryOwnerFeature25MaterializedOwnerMentions = Get-LiveObjectExactClaimTraceFieldSum -Text $proxyLogText -Field 'inventory_owner_feature25_materialized_owner_mentions'
        LiveObjectExactClaimInventoryOwnerFeature25UnmaterializedOwnerMentions = Get-LiveObjectExactClaimTraceFieldSum -Text $proxyLogText -Field 'inventory_owner_feature25_unmaterialized_owner_mentions'
        LiveObjectExactClaimInventoryOwnerFeature25FirstObjectRefs = Get-LiveObjectExactClaimTraceFieldSum -Text $proxyLogText -Field 'inventory_owner_feature25_first_object_refs'
        LiveObjectExactClaimInventoryOwnerFeature25FirstMaterializedObjectRefs = Get-LiveObjectExactClaimTraceFieldSum -Text $proxyLogText -Field 'inventory_owner_feature25_first_materialized_object_refs'
        LiveObjectExactClaimInventoryOwnerFeature25FirstUnmaterializedObjectRefs = Get-LiveObjectExactClaimTraceFieldSum -Text $proxyLogText -Field 'inventory_owner_feature25_first_unmaterialized_object_refs'
        LiveObjectExactClaimInventoryOwnerFeature25SecondObjectRefs = Get-LiveObjectExactClaimTraceFieldSum -Text $proxyLogText -Field 'inventory_owner_feature25_second_object_refs'
        LiveObjectExactClaimInventoryOwnerFeature25SecondMaterializedObjectRefs = Get-LiveObjectExactClaimTraceFieldSum -Text $proxyLogText -Field 'inventory_owner_feature25_second_materialized_object_refs'
        LiveObjectExactClaimInventoryOwnerFeature25SecondUnmaterializedObjectRefs = Get-LiveObjectExactClaimTraceFieldSum -Text $proxyLogText -Field 'inventory_owner_feature25_second_unmaterialized_object_refs'
        LiveObjectExactClaimInventoryOwnerFeature25SecondFragmentBits = Get-LiveObjectExactClaimTraceFieldSum -Text $proxyLogText -Field 'inventory_owner_feature25_second_fragment_bits'
        LiveObjectExactClaimInventoryOwnerFeature25LegacyTailObjectRefs = Get-LiveObjectExactClaimTraceFieldSum -Text $proxyLogText -Field 'inventory_owner_feature25_legacy_tail_object_refs'
        LiveObjectExactClaimInventoryOwnerFeature25LegacyTailMaterializedObjectRefs = Get-LiveObjectExactClaimTraceFieldSum -Text $proxyLogText -Field 'inventory_owner_feature25_legacy_tail_materialized_object_refs'
        LiveObjectExactClaimInventoryOwnerFeature25LegacyTailUnmaterializedObjectRefs = Get-LiveObjectExactClaimTraceFieldSum -Text $proxyLogText -Field 'inventory_owner_feature25_legacy_tail_unmaterialized_object_refs'
        LiveObjectExactClaimInventoryOwnerBranch4000StateStreamMentions = Get-LiveObjectExactClaimTraceFieldSum -Text $proxyLogText -Field 'inventory_owner_branch_4000_state_stream_mentions'
        LiveObjectExactClaimInventoryOwnerBranch8000FixedScalarMentions = Get-LiveObjectExactClaimTraceFieldSum -Text $proxyLogText -Field 'inventory_owner_branch_8000_fixed_scalar_mentions'
        LiveObjectExactClaimOtherObjectTypeMentions = Get-LiveObjectExactClaimTraceFieldSum -Text $proxyLogText -Field 'other_object_type_mentions'
        LiveObjectExactClaimScalarOrientationMentions = Get-LiveObjectExactClaimTraceFieldSum -Text $proxyLogText -Field 'scalar_orientation_mentions'
        LiveObjectExactClaimVectorOrientationMentions = Get-LiveObjectExactClaimTraceFieldSum -Text $proxyLogText -Field 'vector_orientation_mentions'
        GameObjUpdateLogMatches = Get-TextMatchCount -Text $proxyLogText -Pattern 'GameObjUpdate_LiveObject'
        AreaClientAreaLogMatches = Get-TextMatchCount -Text $proxyLogText -Pattern 'Area_ClientArea'
        AreaClientAreaRewriteMatches = Get-TextMatchCount -Text $proxyLogText -Pattern 'Area_ClientArea named compatibility rewrite applied'
        AreaClientAreaModuleContextChecks = Get-TextMatchCount -Text $proxyLogText -Pattern 'Area_ClientArea translator checking runtime Module_Info context'
        AreaClientAreaModuleContextObserved = Get-TextMatchCount -Text $proxyLogText -Pattern 'Area_ClientArea translator checking runtime Module_Info context has_observed_module_context=true'
        FixedWidthCarrierSummaryMatches = Get-TextMatchCount -Text $proxyLogText -Pattern 'exact_placeable_add_module_custom_fixed_width_unproven_carrier'
        FixedWidthCarrierResidualPresentMatches = Get-TextMatchCount -Text $proxyLogText -Pattern 'remaining_source_provenance_after_source_trusted_present=true'
        LiveObjectTerminalResidualMatches = $terminalResidualSummary.Count
        LiveObjectTerminalResidualFirstOffset = $terminalResidualSummary.FirstOffset
        LiveObjectTerminalResidualFirstRecordEnd = $terminalResidualSummary.FirstRecordEnd
        LiveObjectTerminalResidualFirstBitCursor = $terminalResidualSummary.FirstBitCursor
        LiveObjectTerminalResidualLastOffset = $terminalResidualSummary.LastOffset
        LiveObjectTerminalResidualLastRecordEnd = $terminalResidualSummary.LastRecordEnd
        LiveObjectTerminalResidualLastBitCursor = $terminalResidualSummary.LastBitCursor
        LiveObjectTerminalResidualReport = $terminalResidualReport.Path
        LiveObjectTerminalResidualPayload = $terminalResidualReport.PayloadCopy
        ProxyServerEndpoint = if ($proxyServerEndpoint) { $proxyServerEndpoint.ToString() } else { '<none>' }
        StrictTranslate = -not [bool]$NoStrictTranslate
        NWSyncDisabled = -not [bool]$EnableNwsync
        DebugLiveClaim = [bool]$DebugLiveClaim
    }
    $summary | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $summaryPath -Encoding UTF8
    $summary | Format-List
} finally {
    if ($client) {
        $client.Dispose()
    }
    if ($server) {
        $server.Dispose()
    }
    if ($proxy -and -not $proxy.HasExited) {
        Stop-Process -Id $proxy.Id -Force
        $proxy.WaitForExit(5000) | Out-Null
    }
    if ($null -eq $previousDebugLiveClaim) {
        Remove-Item Env:\HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM -ErrorAction SilentlyContinue
    } else {
        $env:HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM = $previousDebugLiveClaim
    }
}
