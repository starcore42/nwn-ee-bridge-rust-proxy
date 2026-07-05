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
    $quickbarHintFirstActionMatchesCandidate = $false
    $quickbarHintFirstActionMatchesPreservedActiveItem = $false
    $quickbarHintFirstActionMatchClass = ''
    $quickbarHintRecommendedActionOutcome = ''
    $quickbarHintFirstClientActionTiming = ''
    $quickbarHintFollowupEventsBeforeFirstClientAction = 0
    $quickbarHintServerToClientEventsSincePendingRefresh = 0
    $quickbarHintClientToServerEventsSincePendingRefresh = 0
    $quickbarHintClientGuiEventEventsSincePendingRefresh = 0
    $quickbarHintServerQuickbarItemUseCountEventsSincePendingRefresh = 0
    $quickbarHintServerQuickbarItemUseCountRecordsSincePendingRefresh = 0
    $quickbarHintServerQuickbarItemUseCountRowsSincePendingRefresh = 0
    $quickbarHintServerQuickbarItemUseCountCandidateRowsSincePendingRefresh = 0
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
    $quickbarHintInventoryEventsAfterFirstClientAction = 0
    $quickbarHintClientGuiEventEventsAfterFirstClientAction = 0
    $quickbarHintOtherEventsAfterFirstClientAction = 0
    if ($null -ne $quickbarHintJson) {
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
        $matchClassProp = $quickbarHintJson.PSObject.Properties['first_client_action_match_class']
        if ($null -ne $matchClassProp -and $null -ne $matchClassProp.Value) {
            $quickbarHintFirstActionMatchClass = [string]$matchClassProp.Value
        }
        $recommendedActionOutcomeProp = $quickbarHintJson.PSObject.Properties['pending_item_refresh_recommended_action_outcome']
        if ($null -ne $recommendedActionOutcomeProp -and $null -ne $recommendedActionOutcomeProp.Value) {
            $quickbarHintRecommendedActionOutcome = [string]$recommendedActionOutcomeProp.Value
        }
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
        QuickbarItemRefreshHintFirstActionMatchesCandidate = $quickbarHintFirstActionMatchesCandidate
        QuickbarItemRefreshHintFirstActionMatchesPreservedActiveItem = $quickbarHintFirstActionMatchesPreservedActiveItem
        QuickbarItemRefreshHintFirstActionMatchClass = $quickbarHintFirstActionMatchClass
        QuickbarItemRefreshHintRecommendedActionOutcome = $quickbarHintRecommendedActionOutcome
        QuickbarItemRefreshHintFirstClientActionTiming = $quickbarHintFirstClientActionTiming
        QuickbarItemRefreshHintFollowupEventsBeforeFirstClientAction = $quickbarHintFollowupEventsBeforeFirstClientAction
        QuickbarItemRefreshHintServerToClientEventsSincePendingRefresh = $quickbarHintServerToClientEventsSincePendingRefresh
        QuickbarItemRefreshHintClientToServerEventsSincePendingRefresh = $quickbarHintClientToServerEventsSincePendingRefresh
        QuickbarItemRefreshHintClientGuiEventEventsSincePendingRefresh = $quickbarHintClientGuiEventEventsSincePendingRefresh
        QuickbarItemRefreshHintServerQuickbarItemUseCountEventsSincePendingRefresh = $quickbarHintServerQuickbarItemUseCountEventsSincePendingRefresh
        QuickbarItemRefreshHintServerQuickbarItemUseCountRecordsSincePendingRefresh = $quickbarHintServerQuickbarItemUseCountRecordsSincePendingRefresh
        QuickbarItemRefreshHintServerQuickbarItemUseCountRowsSincePendingRefresh = $quickbarHintServerQuickbarItemUseCountRowsSincePendingRefresh
        QuickbarItemRefreshHintServerQuickbarItemUseCountCandidateRowsSincePendingRefresh = $quickbarHintServerQuickbarItemUseCountCandidateRowsSincePendingRefresh
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
        QuickbarRegistryCompactItemEmissionDirectOnlyProofObjects = Get-QuickbarRegistryContextTraceFieldMax -Text $proxyLogText -Field 'compact_item_emission_direct_only_proof_objects'
        QuickbarRegistryCompactItemEmissionFeature25OnlyProofObjects = Get-QuickbarRegistryContextTraceFieldMax -Text $proxyLogText -Field 'compact_item_emission_feature25_only_proof_objects'
        QuickbarRegistryCompactItemEmissionSharedProofObjects = Get-QuickbarRegistryContextTraceFieldMax -Text $proxyLogText -Field 'compact_item_emission_shared_proof_objects'
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
        QuickbarSemanticPostFeature25FirstItemRefs = Get-SemanticPostQuickbarItemContextTraceFieldMax -Text $proxyLogText -Field 'inventory_feature25_first_item_refs'
        QuickbarSemanticPostFeature25SecondItemRefs = Get-SemanticPostQuickbarItemContextTraceFieldMax -Text $proxyLogText -Field 'inventory_feature25_second_item_refs'
        QuickbarSemanticPostFeature25LegacyTailItemRefs = Get-SemanticPostQuickbarItemContextTraceFieldMax -Text $proxyLogText -Field 'inventory_feature25_legacy_tail_item_refs'
        QuickbarSemanticPostClearedInventoryItemObjectIds = Get-SemanticPostQuickbarItemContextTraceFieldMax -Text $proxyLogText -Field 'cleared_inventory_item_object_ids'
        QuickbarStreamProbeRegistryDirectItemProofObjects = Get-QuickbarRegistryContextTraceFieldMax -Text $proxyLogText -Field 'direct_item_proof_objects' -Committed $false
        QuickbarStreamProbeRegistryFeature25ItemProofObjects = Get-QuickbarRegistryContextTraceFieldMax -Text $proxyLogText -Field 'feature25_item_proof_objects' -Committed $false
        QuickbarStreamProbeRegistryCompactItemEmissionProofObjects = Get-QuickbarRegistryContextTraceFieldMax -Text $proxyLogText -Field 'compact_item_emission_proof_objects' -Committed $false
        QuickbarStreamProbeRegistryCompactItemEmissionDirectOnlyProofObjects = Get-QuickbarRegistryContextTraceFieldMax -Text $proxyLogText -Field 'compact_item_emission_direct_only_proof_objects' -Committed $false
        QuickbarStreamProbeRegistryCompactItemEmissionFeature25OnlyProofObjects = Get-QuickbarRegistryContextTraceFieldMax -Text $proxyLogText -Field 'compact_item_emission_feature25_only_proof_objects' -Committed $false
        QuickbarStreamProbeRegistryCompactItemEmissionSharedProofObjects = Get-QuickbarRegistryContextTraceFieldMax -Text $proxyLogText -Field 'compact_item_emission_shared_proof_objects' -Committed $false
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
