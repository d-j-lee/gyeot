<#
.SYNOPSIS
    곁(gyeot)을 설치합니다. 관리자 권한이 필요 없습니다.

.DESCRIPTION
    gyeotd.exe · gyeot-hook.exe · gyeot-shell.exe(있으면) · gyeot-channel.mjs 를
    %LOCALAPPDATA%\gyeot\bin 에, 얼굴(face)을 %LOCALAPPDATA%\gyeot\bin\face 에 복사하고
    `gyeotd --init` 을 실행한 뒤, Claude Code 훅 · Codex 훅/notify · .mcp.json(channel)
    조각을 절대 경로로 채워서 출력합니다. 기본값은 출력만 하고 다른 프로그램의 설정은 바꾸지 않습니다.

    Claude Code 훅 조각에는 기본으로 PostToolBatch 가 들어 있습니다. 사람이 터미널에서 승인한 뒤
    곁의 승인 대기를 닫는 신호입니다(-NoActivityHook 으로 뺄 수 있음).

    얼굴은 셸 트레이의 "곁 열기" 또는 `gyeotd --open`(기본 브라우저)으로 엽니다. 얼굴 주소에는
    토큰이 없고, 얼굴이 일회용 부트 코드를 세션 열쇠로 바꿔 데몬에 붙습니다.

    설치 폴더나 데이터 폴더(GYEOT_HOME)가 %LOCALAPPDATA% 밖이면(-InstallRoot C:\gyeot 등) 그 폴더의 상속을
    끊고 지금 사용자와 SYSTEM만 모든 권한을 갖게 좁힌 뒤(icacls) 결과를 보여 줍니다. 다른 사용자가 token을
    읽거나 gyeot-hook.exe를 바꿔치지 못하게 하려는 것입니다. 폴더 주인이 다른 사용자면 설치하지 않습니다.

    zip 배포본(bin\, face\, scripts\, examples\)에서도, 빌드한 저장소(target\release 등)에서도 돌아갑니다.

.PARAMETER SourceDir
    설치할 파일이 있는 폴더. 기본값은 이 스크립트의 상위 폴더(zip의 맨 위).

.PARAMETER InstallRoot
    설치·데이터 폴더. 기본값은 $env:GYEOT_HOME, 없으면 %LOCALAPPDATA%\gyeot.
    %LOCALAPPDATA% 밖이면 그 사용자와 SYSTEM만 쓸 수 있게 권한을 좁힙니다.

.PARAMETER DryRun
    아무것도 바꾸지 않고 할 일과 조각만 보여 줍니다.

.PARAMETER ApplyClaudeHooks
    ~/.claude/settings.json 을 settings.json.bak-<시각> 으로 백업한 뒤 곁의 훅(PostToolBatch 포함)을 더합니다.
    기존 훅은 지우지 않고, 이미 있으면 다시 더하지 않습니다(여러 번 실행해도 같음).

.PARAMETER NoActivityHook
    PostToolBatch 훅을 넣지 않습니다(기본은 넣음). -ApplyClaudeHooks 와 함께 주면 이미 넣은 곁의
    PostToolBatch 항목만 뺍니다(다른 훅은 그대로). 빼면 터미널에서 승인해도 곁의 승인 대기가 턴이
    끝날 때까지 남을 수 있고, 곁은 그 세션의 승인 대기를 말풍선까지만 알립니다.

.PARAMETER ClaudeSettingsPath
    Claude Code 사용자 설정 파일. 기본값 ~/.claude/settings.json.

.PARAMETER RegisterStartup
    로그인할 때 gyeotd를 창 없이 띄우는 작업 스케줄러 작업(gyeot-daemon)을 등록합니다.
    작업 스케줄러가 거절하면 HKCU\...\Run 값으로 대신 등록합니다.

.PARAMETER NoStart
    -RegisterStartup 뒤에 데몬을 바로 띄우지 않습니다.

.PARAMETER NoInit
    gyeotd --init 을 건너뜁니다.

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File .\scripts\install.ps1
.EXAMPLE
    powershell -ExecutionPolicy Bypass -File .\scripts\install.ps1 -ApplyClaudeHooks -RegisterStartup
.EXAMPLE
    powershell -ExecutionPolicy Bypass -File .\scripts\install.ps1 -DryRun
.EXAMPLE
    powershell -ExecutionPolicy Bypass -File .\scripts\install.ps1 -ApplyClaudeHooks -NoActivityHook
#>
[CmdletBinding()]
param(
    [string]$SourceDir,
    [string]$InstallRoot,
    [switch]$DryRun,
    [switch]$ApplyClaudeHooks,
    [switch]$NoActivityHook,
    [string]$ClaudeSettingsPath,
    [switch]$RegisterStartup,
    [switch]$NoStart,
    [switch]$NoInit
)

$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'gyeot-common.ps1')

$restoreEncoding = $null
if ([Console]::IsOutputRedirected) {
    # CI 로그 등으로 넘길 때 한글이 깨지지 않게(끝나면 되돌린다).
    $restoreEncoding = [Console]::OutputEncoding
    [Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false)
}

function Find-GyeotSource {
    param([string]$Root, [string[]]$Candidates)
    foreach ($c in $Candidates) {
        $p = Join-Path $Root $c
        if (Test-Path -LiteralPath $p) { return (Get-GyeotFullPath $p) }
    }
    return $null
}

function Copy-GyeotFile {
    param([string]$From, [string]$To)
    for ($i = 1; $i -le 5; $i++) {
        try {
            Copy-Item -LiteralPath $From -Destination $To -Force
            return
        } catch {
            if ($i -eq 5) { throw "복사하지 못했어요: $From → $To ($($_.Exception.Message))" }
            Start-Sleep -Milliseconds (300 * $i)
        }
    }
}

function Invoke-GyeotInit {
    param([string]$DaemonExe, [string]$Root)
    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = $DaemonExe
    $psi.Arguments = '--init'
    $psi.WorkingDirectory = Split-Path -Parent $DaemonExe
    $psi.UseShellExecute = $false
    $psi.CreateNoWindow = $true
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError = $true
    $psi.StandardOutputEncoding = [System.Text.Encoding]::UTF8
    $psi.StandardErrorEncoding = [System.Text.Encoding]::UTF8
    $psi.EnvironmentVariables['GYEOT_HOME'] = $Root
    $proc = [System.Diagnostics.Process]::Start($psi)
    $stdout = $proc.StandardOutput.ReadToEndAsync()
    $stderr = $proc.StandardError.ReadToEndAsync()
    if (-not $proc.WaitForExit(30000)) {
        try { $proc.Kill() } catch { Write-Verbose $_.Exception.Message }
        throw 'gyeotd --init 이 30초 안에 끝나지 않았어요.'
    }
    $proc.WaitForExit()
    foreach ($line in (($stdout.Result + $stderr.Result) -split "`r?`n")) {
        if ($line.Trim()) { Write-GyeotInfo "  gyeotd: $line" }
    }
    if ($proc.ExitCode -ne 0) { throw "gyeotd --init 이 종료 코드 $($proc.ExitCode)로 끝났어요." }
}

function Start-GyeotDaemonHidden {
    param([string]$DaemonExe)
    Start-Process -FilePath $DaemonExe -WorkingDirectory (Split-Path -Parent $DaemonExe) -WindowStyle Hidden
}

try {
    $mode = ''
    if ($DryRun) { $mode = ' (미리보기: 아무것도 바꾸지 않아요)' }
    Write-Host ''
    Write-Host "곁(gyeot) 설치$mode" -ForegroundColor White
    Write-Host ''

    # ── 1. 원본 찾기 ──
    if (-not $SourceDir) { $SourceDir = Split-Path -Parent $PSScriptRoot }
    $SourceDir = Get-GyeotFullPath $SourceDir
    if (-not $InstallRoot) {
        $InstallRoot = Get-GyeotDefaultRoot
        $customRoot = [bool]$env:GYEOT_HOME
    } else {
        $customRoot = $true
    }
    $InstallRoot = Get-GyeotFullPath $InstallRoot
    $BinDir = Join-Path $InstallRoot 'bin'
    $FaceDir = Join-Path $BinDir 'face'
    $ScriptsDir = Join-Path $InstallRoot 'scripts'
    $SnippetDir = Join-Path $InstallRoot 'snippets'

    Write-GyeotStep '설치할 파일 찾기'
    Write-GyeotInfo "원본: $SourceDir"
    Write-GyeotInfo "설치: $InstallRoot"
    $src = @{
        Daemon  = Find-GyeotSource $SourceDir @('bin\gyeotd.exe', 'target\release\gyeotd.exe', 'target\x86_64-pc-windows-msvc\release\gyeotd.exe')
        Hook    = Find-GyeotSource $SourceDir @('bin\gyeot-hook.exe', 'target\release\gyeot-hook.exe', 'target\x86_64-pc-windows-msvc\release\gyeot-hook.exe')
        Shell   = Find-GyeotSource $SourceDir @('bin\gyeot-shell.exe', 'shell\src-tauri\target\release\gyeot-shell.exe')
        Channel = Find-GyeotSource $SourceDir @('bin\gyeot-channel.mjs', 'channel\dist\gyeot-channel.mjs')
        Face    = $null
    }
    foreach ($candidate in @('face', 'face\dist')) {
        $p = Join-Path $SourceDir $candidate
        if (Test-Path -LiteralPath (Join-Path $p 'index.html')) { $src.Face = Get-GyeotFullPath $p; break }
    }
    $missing = @()
    foreach ($k in @('Daemon', 'Hook', 'Face')) { if (-not $src[$k]) { $missing += $k } }
    $labels = @{ Daemon = 'gyeotd.exe'; Hook = 'gyeot-hook.exe'; Shell = 'gyeot-shell.exe'; Channel = 'gyeot-channel.mjs'; Face = 'face\index.html' }
    foreach ($k in @('Daemon', 'Hook', 'Shell', 'Channel', 'Face')) {
        if ($src[$k]) { Write-GyeotOk "$($labels[$k]): $($src[$k])" }
        elseif ($k -eq 'Shell') { Write-GyeotWarn 'gyeot-shell.exe 가 없어요. 셸 없이 gyeotd --open 으로 브라우저에서 얼굴을 열 수 있어요.' }
        elseif ($k -eq 'Channel') { Write-GyeotWarn 'gyeot-channel.mjs 가 없어요. Claude Code 세션과 대화(channel)는 쓸 수 없어요.' }
        else { Write-GyeotWarn "$($labels[$k]) 를 찾지 못했어요." }
    }
    if ($missing.Count -gt 0) {
        throw "필요한 파일이 없어요: $(($missing | ForEach-Object { $labels[$_] }) -join ', '). zip을 다시 풀었는지, 또는 저장소에서 빌드했는지 확인하세요."
    }
    $zoneMarked = $false
    try { $zoneMarked = [bool](Get-Item -LiteralPath $src.Daemon -Stream 'Zone.Identifier' -ErrorAction Stop) } catch { $zoneMarked = $false }
    if ($zoneMarked) {
        Write-GyeotWarn '내려받은 파일 표시(Zone.Identifier)가 있어요. SmartScreen이 막으면 zip을 풀기 전에 속성에서 "차단 해제"를 고르거나 Unblock-File 을 쓰세요.'
    }

    $dst = @{
        Daemon  = Join-Path $BinDir 'gyeotd.exe'
        Hook    = Join-Path $BinDir 'gyeot-hook.exe'
        Shell   = Join-Path $BinDir 'gyeot-shell.exe'
        Channel = Join-Path $BinDir 'gyeot-channel.mjs'
    }

    # ── 2. 실행 중인 곁 멈추기(덮어쓸 파일이 잠겨 있지 않게) ──
    Write-GyeotStep '실행 중인 곁 확인'
    $running = Get-GyeotProcessesUnder @('gyeotd', 'gyeot-shell') $BinDir
    $wasDaemon = $false
    $wasShell = $false
    foreach ($p in $running) {
        if ($p.ProcessName -eq 'gyeotd') { $wasDaemon = $true } else { $wasShell = $true }
        if ($DryRun) { Write-GyeotPlan "$($p.ProcessName) (pid $($p.Id)) 를 잠시 멈춤" }
        else {
            Write-GyeotInfo "$($p.ProcessName) (pid $($p.Id)) 를 잠시 멈춰요"
            Stop-Process -Id $p.Id -Force
        }
    }
    if ($running.Count -eq 0) { Write-GyeotInfo '설치 폴더에서 실행 중인 곁이 없어요.' }
    elseif (-not $DryRun) { Start-Sleep -Milliseconds 500 }

    # ── 3. 폴더 권한(Windows, docs/contracts.md §13.4) ──
    # 복사하기 전에 좁혀서 새로 만드는 bin\ · token 등이 좁힌 권한을 이어받게 한다.
    Write-GyeotStep '폴더 권한 확인(설치·데이터 폴더)'
    $privatePlan = @(Get-GyeotPrivateFolderPlan @($InstallRoot, $env:GYEOT_HOME))
    if ($privatePlan.Count -eq 0) {
        Write-GyeotInfo '설치·데이터 폴더가 %LOCALAPPDATA% 안이라 그 사용자 전용 권한을 이어받아요(바꾸지 않음).'
    } elseif (-not (Test-GyeotWindowsHost)) {
        foreach ($f in $privatePlan) { Write-GyeotInfo "Windows가 아니라 권한을 바꾸지 않아요: $f" }
    } else {
        $userSid = Get-GyeotCurrentUserSid
        $userName = Get-GyeotSidName $userSid
        foreach ($f in $privatePlan) {
            Write-GyeotInfo "%LOCALAPPDATA% 밖이에요: $f → 상속을 끊고 $userName 와 SYSTEM만 남겨요."
            if (Test-Path -LiteralPath $f) {
                $owner = (Get-GyeotAclSummary $f).Owner
                if (-not (Test-GyeotTrustedOwner $owner $userSid)) {
                    throw "$f 의 주인이 다른 사용자($(Get-GyeotSidName $owner))예요. 그 사용자는 권한을 다시 넓힐 수 있으니 새 폴더를 고르거나 %LOCALAPPDATA% 에 설치하세요."
                }
            }
            $icaclsLine = (Get-GyeotIcaclsArgument $f $userSid | ForEach-Object { if ($_ -match '[\s()]') { "`"$_`"" } else { $_ } }) -join ' '
            if ($DryRun) {
                Write-GyeotPlan "icacls $icaclsLine"
                Write-GyeotPlan '(남은 다른 주체의 직접 항목은 빼고, 이미 있던 하위 항목은 이 폴더를 이어받게 다시 맞춤)'
                continue
            }
            if (-not (Test-Path -LiteralPath $f)) { New-Item -ItemType Directory -Path $f -Force | Out-Null }
            Write-GyeotInfo "icacls $icaclsLine"
            $summary = Set-GyeotFolderPrivate -Folder $f -UserSid $userSid
            $problems = @(Get-GyeotAclProblem -Summary $summary -UserSid $userSid)
            if ($problems.Count -gt 0) {
                throw "$f 의 권한을 좁히지 못했어요: $($problems -join '; ') (지금: $(Format-GyeotAclSummary $summary))"
            }
            Write-GyeotOk "권한을 좁혔어요: $f — $(Format-GyeotAclSummary $summary)"
        }
    }

    # ── 4. 복사 ──
    Write-GyeotStep "파일 복사 → $BinDir"
    $plan = @()
    foreach ($k in @('Daemon', 'Hook', 'Shell', 'Channel')) {
        if ($src[$k]) { $plan += , @($src[$k], $dst[$k]) }
    }
    if ($DryRun) {
        foreach ($pair in $plan) { Write-GyeotPlan "$($pair[0]) → $($pair[1])" }
        Write-GyeotPlan "$($src.Face)\* → $FaceDir\ (기존 face 폴더는 비우고 새로 복사)"
        Write-GyeotPlan "uninstall.ps1, doctor.ps1, bootstrap.ps1, gyeot-common.ps1 → $ScriptsDir\"
    } else {
        foreach ($d in @($InstallRoot, $BinDir, $ScriptsDir)) {
            if (-not (Test-Path -LiteralPath $d)) { New-Item -ItemType Directory -Path $d -Force | Out-Null }
        }
        foreach ($pair in $plan) {
            Copy-GyeotFile -From $pair[0] -To $pair[1]
            Write-GyeotOk (Split-Path -Leaf $pair[1])
        }
        if (Test-Path -LiteralPath $FaceDir) { Remove-Item -LiteralPath $FaceDir -Recurse -Force }
        New-Item -ItemType Directory -Path $FaceDir -Force | Out-Null
        Copy-Item -Path (Join-Path $src.Face '*') -Destination $FaceDir -Recurse -Force
        Write-GyeotOk "face\ ($(@(Get-ChildItem -LiteralPath $FaceDir -Recurse -File).Count)개 파일)"
        foreach ($f in @('uninstall.ps1', 'doctor.ps1', 'bootstrap.ps1', 'gyeot-common.ps1')) {
            $from = Join-Path $PSScriptRoot $f
            if (Test-Path -LiteralPath $from) { Copy-GyeotFile -From $from -To (Join-Path $ScriptsDir $f) }
        }
        Write-GyeotOk "scripts\uninstall.ps1 (지울 때) · doctor.ps1 (점검) · bootstrap.ps1 (업데이트)"
    }

    # ── 5. gyeotd --init ──
    if ($NoInit) {
        Write-GyeotStep 'gyeotd --init 건너뜀(-NoInit)'
    } elseif ($DryRun) {
        Write-GyeotStep 'gyeotd --init'
        Write-GyeotPlan "GYEOT_HOME=$InstallRoot $($dst.Daemon) --init"
    } else {
        Write-GyeotStep 'gyeotd --init (데이터 폴더·토큰 준비)'
        Invoke-GyeotInit -DaemonExe $dst.Daemon -Root $InstallRoot
        if (Test-Path -LiteralPath (Join-Path $InstallRoot 'token')) { Write-GyeotOk '토큰 준비됨' }
        else { Write-GyeotWarn "토큰 파일이 아직 없어요($InstallRoot\token). 데몬이 처음 뜰 때 만들어요." }
    }

    # ── 6. 연결 조각 ──
    $hookExe = $dst.Hook
    $channelJs = $dst.Channel
    $noActivity = [bool]$NoActivityHook
    $claudeSnippet = ConvertTo-GyeotJson (New-GyeotClaudeHooksSnippet -HookExe $hookExe -NoActivity:$noActivity)

    $codexHooks = New-GyeotDict
    $codexCmd = (Get-GyeotShortPath $hookExe)
    if ($codexCmd -match '\s') { $codexCmd = '"' + $codexCmd + '"' }
    $codexCmd = "$codexCmd codex"
    foreach ($ev in $script:GyeotCodexEvents) {
        $h = New-GyeotDict
        $h['type'] = 'command'
        $h['command'] = $codexCmd
        $h['async'] = $true
        $h['timeout'] = 5
        $g = New-GyeotDict
        $g['hooks'] = (New-GyeotList @(, $h))
        $codexHooks[$ev] = (New-GyeotList @(, $g))
    }
    $codexRoot = New-GyeotDict
    $codexRoot['hooks'] = $codexHooks
    $codexSnippet = ConvertTo-GyeotJson $codexRoot

    $notifySnippet = "notify = [$(ConvertTo-GyeotTomlString $hookExe), 'codex-notify']"

    $server = New-GyeotDict
    $server['command'] = 'node'
    $server['args'] = (New-GyeotList @($channelJs))
    if ($customRoot) {
        $envDict = New-GyeotDict
        $envDict['GYEOT_HOME'] = $InstallRoot
        $server['env'] = $envDict
    }
    $servers = New-GyeotDict
    $servers['gyeot'] = $server
    $mcpRoot = New-GyeotDict
    $mcpRoot['mcpServers'] = $servers
    $mcpSnippet = ConvertTo-GyeotJson $mcpRoot

    $claudeSettings = $ClaudeSettingsPath
    if (-not $claudeSettings) { $claudeSettings = Join-Path (Get-GyeotHomeDir) '.claude\settings.json' }
    $claudeSettings = Get-GyeotFullPath $claudeSettings
    $codexHome = $env:CODEX_HOME
    if (-not $codexHome) { $codexHome = Join-Path (Get-GyeotHomeDir) '.codex' }

    Write-GyeotStep '연결 조각(절대 경로를 채웠어요)'
    if ($ApplyClaudeHooks) {
        Write-GyeotInfo "Claude Code 훅은 아래 -ApplyClaudeHooks 단계에서 $claudeSettings 에 더해요."
    } else {
        Write-GyeotInfo "Claude Code 훅: $claudeSettings 의 `"hooks`" 에 합치세요(또는 -ApplyClaudeHooks 로 다시 실행)."
    }
    if ($noActivity) {
        Write-GyeotWarn 'PostToolBatch 훅을 넣지 않아요(-NoActivityHook). 터미널에서 승인해도 곁의 승인 대기가 턴이 끝날 때까지 남을 수 있어요.'
    } else {
        Write-GyeotInfo 'PostToolBatch 훅이 들어 있어요. 터미널에서 승인한 뒤 곁의 승인 대기를 닫아요(빼려면 -NoActivityHook).'
    }
    Write-GyeotBlock 'Claude Code 훅 (~/.claude/settings.json)' $claudeSnippet
    Write-GyeotInfo "Codex 훅: $(Join-Path $codexHome 'hooks.json') 에 합치세요(입력 모양은 실측 전 F3)."
    Write-GyeotBlock 'Codex 훅 (~/.codex/hooks.json)' $codexSnippet
    Write-GyeotInfo "Codex notify: $(Join-Path $codexHome 'config.toml') 의 맨 위(다른 [표]보다 앞)에 넣으세요(실측 전 F4)."
    Write-GyeotBlock 'Codex notify (~/.codex/config.toml)' $notifySnippet
    if ($src.Channel) {
        Write-GyeotInfo '곁 channel: 프로젝트의 .mcp.json 에 넣거나, 사용자 범위로 등록하세요:'
        Write-GyeotInfo "  claude mcp add --scope user gyeot -- node `"$channelJs`""
        Write-GyeotBlock 'channel (.mcp.json)' $mcpSnippet
        Write-GyeotInfo '세션 시작: claude --dangerously-load-development-channels server:gyeot'
        if (-not (Get-Command node -ErrorAction SilentlyContinue)) {
            Write-GyeotWarn 'node 가 PATH에 없어요. channel에는 Node.js 20 이상이 필요해요.'
        }
    }
    if ($customRoot) {
        Write-GyeotWarn "기본이 아닌 폴더($InstallRoot)예요. 훅·데몬·channel이 같은 폴더를 보도록 사용자 환경 변수 GYEOT_HOME 을 이 경로로 두세요:"
        Write-GyeotInfo "  [Environment]::SetEnvironmentVariable('GYEOT_HOME', '$InstallRoot', 'User')"
    }

    if ($DryRun) {
        Write-GyeotPlan "위 조각을 $SnippetDir 에 파일로도 저장"
    } else {
        if (-not (Test-Path -LiteralPath $SnippetDir)) { New-Item -ItemType Directory -Path $SnippetDir -Force | Out-Null }
        Write-GyeotTextFile -Path (Join-Path $SnippetDir 'claude-settings.hooks.json') -Text ($claudeSnippet + "`n")
        Write-GyeotTextFile -Path (Join-Path $SnippetDir 'codex-hooks.json') -Text ($codexSnippet + "`n")
        Write-GyeotTextFile -Path (Join-Path $SnippetDir 'codex-config.notify.toml') -Text ($notifySnippet + "`n")
        Write-GyeotTextFile -Path (Join-Path $SnippetDir 'mcp.json') -Text ($mcpSnippet + "`n")
        Write-GyeotOk "조각을 파일로도 저장했어요: $SnippetDir"
    }

    # ── 7. Claude Code 훅 적용(선택) ──
    if ($ApplyClaudeHooks) {
        Write-GyeotStep "Claude Code 훅 적용 → $claudeSettings"
        $res = Update-GyeotJsonFile -Path $claudeSettings -DryRun:$DryRun -Mutate {
            param($tree)
            Merge-GyeotClaudeHook -Settings $tree -HookExe $hookExe -NoActivity:$noActivity
        }
        $d = $res.Detail
        $removedNote = ''
        if ($d.Removed -gt 0) { $removedNote = ", 곁의 PostToolBatch $($d.Removed)개 뺌" }
        if (-not $res.Changed) {
            Write-GyeotOk "이미 들어 있어요(그대로 둠, 곁 항목 $($d.Kept)개)."
        } elseif ($DryRun) {
            Write-GyeotPlan "백업 후 곁 항목 $($d.Added)개 추가, $($d.Updated)개 경로 갱신$removedNote"
        } else {
            if ($res.Backup) { Write-GyeotOk "백업: $($res.Backup)" }
            Write-GyeotOk "곁 항목 $($d.Added)개 추가, $($d.Updated)개 경로 갱신$removedNote, 기존 훅은 그대로"
            Write-GyeotInfo '이미 열려 있는 Claude Code 세션은 /hooks 를 열거나 다시 시작해야 반영돼요.'
        }
    }

    # ── 8. 로그인 시 데몬 시작(선택) ──
    if ($RegisterStartup) {
        Write-GyeotStep "로그인 시 곁 데몬 시작 등록(작업 이름 $script:GyeotTaskName)"
        $probe = $dst.Daemon
        if ($DryRun -and -not (Test-Path -LiteralPath $probe)) { $probe = $src.Daemon }
        $launch = Get-GyeotDaemonLaunch -DaemonExe $dst.Daemon -ProbePath $probe
        if ($launch.Wrapped) { Write-GyeotInfo 'gyeotd가 콘솔 프로그램이라 숨긴 PowerShell로 띄워요(로그인 때 잠깐 깜박일 수 있어요).' }
        if ($DryRun) {
            Write-GyeotPlan "작업 스케줄러: 로그인 시 `"$($launch.Execute)`" $($launch.Arguments)"
        } else {
            $user = [System.Security.Principal.WindowsIdentity]::GetCurrent().Name
            $registered = $false
            try {
                if ($launch.Arguments) {
                    $action = New-ScheduledTaskAction -Execute $launch.Execute -Argument $launch.Arguments -WorkingDirectory $launch.WorkingDirectory
                } else {
                    $action = New-ScheduledTaskAction -Execute $launch.Execute -WorkingDirectory $launch.WorkingDirectory
                }
                $trigger = New-ScheduledTaskTrigger -AtLogOn -User $user
                $principal = New-ScheduledTaskPrincipal -UserId $user -LogonType Interactive -RunLevel Limited
                $settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries `
                    -ExecutionTimeLimit ([TimeSpan]::Zero) -MultipleInstances IgnoreNew -StartWhenAvailable
                Register-ScheduledTask -TaskName $script:GyeotTaskName -Action $action -Trigger $trigger -Principal $principal `
                    -Settings $settings -Description '곁 데몬(gyeotd)을 로그인할 때 창 없이 띄웁니다.' -Force | Out-Null
                $registered = $true
                Write-GyeotOk '작업 스케줄러에 등록했어요.'
            } catch {
                Write-GyeotWarn "작업 스케줄러가 거절했어요: $($_.Exception.Message)"
                Write-GyeotInfo '대신 HKCU\...\Run 값으로 등록해요.'
                $cmdLine = "`"$($launch.Execute)`""
                if ($launch.Arguments) { $cmdLine = "$cmdLine $($launch.Arguments)" }
                New-ItemProperty -Path $script:GyeotRunKey -Name $script:GyeotRunValueDaemon -Value $cmdLine -PropertyType String -Force | Out-Null
                Write-GyeotOk "Run 값 '$script:GyeotRunValueDaemon' 을 등록했어요."
            }
            if (-not $NoStart -and -not (Get-GyeotProcessesUnder @('gyeotd') $BinDir).Count) {
                if ($registered) { Start-ScheduledTask -TaskName $script:GyeotTaskName }
                else { Start-GyeotDaemonHidden $dst.Daemon }
                $wasDaemon = $false
                Write-GyeotOk '곁 데몬을 띄웠어요.'
            }
        }
    }

    # ── 9. 멈췄던 것 다시 띄우기 ──
    if (-not $DryRun) {
        if ($wasDaemon -and -not (Get-GyeotProcessesUnder @('gyeotd') $BinDir).Count) {
            Start-GyeotDaemonHidden $dst.Daemon
            Write-GyeotOk '멈췄던 곁 데몬을 다시 띄웠어요.'
        }
        if ($wasShell -and $src.Shell) {
            Start-Process -FilePath $dst.Shell -WorkingDirectory $BinDir
            Write-GyeotOk '멈췄던 곁 셸을 다시 띄웠어요.'
        }
    }

    # ── 끝 ──
    Write-Host ''
    if ($DryRun) {
        Write-Host '미리보기를 마쳤어요. -DryRun 없이 다시 실행하면 위 일을 합니다.' -ForegroundColor White
    } else {
        Write-Host '설치를 마쳤어요.' -ForegroundColor White
        if ($src.Shell) {
            Write-GyeotInfo "곁 셸 실행: `"$($dst.Shell)`"  (Ctrl+Alt+Space 로 보이기/숨기기)"
            Write-GyeotInfo "얼굴 열기: 트레이 아이콘의 '곁 열기', 또는 브라우저로 `"$($dst.Daemon)`" --open"
        } else {
            Write-GyeotInfo "얼굴 열기: `"$($dst.Daemon)`" --open  (기본 브라우저로 열려요)"
            Write-GyeotInfo "곁 셸(설치본 gyeot-shell-setup)을 쓰면 트레이 아이콘의 '곁 열기'로도 열 수 있어요."
        }
        Write-GyeotInfo '얼굴 주소에는 토큰이 없어요. 주소만 복사해 열면 안내 쪽이 보이니 위 방법으로 여세요.'
        Write-GyeotInfo "점검: powershell -NoProfile -ExecutionPolicy Bypass -File `"$(Join-Path $ScriptsDir 'doctor.ps1')`""
        Write-GyeotInfo "지우기: powershell -ExecutionPolicy Bypass -File `"$(Join-Path $ScriptsDir 'uninstall.ps1')`""
    }
    $exitCode = 0
} catch {
    Write-Host ''
    Write-Host "설치하지 못했어요: $($_.Exception.Message)" -ForegroundColor Red
    $exitCode = 1
} finally {
    if ($null -ne $restoreEncoding) { [Console]::OutputEncoding = $restoreEncoding }
}
exit $exitCode
