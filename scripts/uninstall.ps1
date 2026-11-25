<#
.SYNOPSIS
    곁(gyeot)을 지웁니다. 데이터(토큰·설정·이벤트 기록·학습 기록)는 -PurgeData 를 줄 때만 지웁니다.

.DESCRIPTION
    실행 중인 곁(gyeotd·gyeot-shell)을 멈추고, 로그인 시작 등록(작업 스케줄러 gyeot-daemon,
    HKCU\...\Run 의 gyeotd·gyeot-shell 값 가운데 이 설치를 가리키는 것)을 지우고,
    설치 폴더의 bin\ · scripts\ · snippets\ 를 지웁니다. 관리자 권한이 필요 없습니다.

.PARAMETER InstallRoot
    설치·데이터 폴더. 기본값은 $env:GYEOT_HOME, 없으면 %LOCALAPPDATA%\gyeot.

.PARAMETER PurgeData
    데이터까지 모두(설치 폴더 전체) 지웁니다. 되돌릴 수 없습니다.

.PARAMETER RemoveClaudeHooks
    ~/.claude/settings.json 을 백업한 뒤 곁의 훅 항목만 뺍니다(다른 훅은 그대로).

.PARAMETER ClaudeSettingsPath
    Claude Code 사용자 설정 파일. 기본값 ~/.claude/settings.json.

.PARAMETER DryRun
    아무것도 바꾸지 않고 할 일만 보여 줍니다.

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File "$env:LOCALAPPDATA\gyeot\scripts\uninstall.ps1" -RemoveClaudeHooks
#>
[CmdletBinding()]
param(
    [string]$InstallRoot,
    [switch]$PurgeData,
    [switch]$RemoveClaudeHooks,
    [string]$ClaudeSettingsPath,
    [switch]$DryRun
)

$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'gyeot-common.ps1')

$restoreEncoding = $null
if ([Console]::IsOutputRedirected) {
    $restoreEncoding = [Console]::OutputEncoding
    [Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false)
}

function Invoke-GyeotChange {
    param([string]$Description, [scriptblock]$Action)
    if ($DryRun) { Write-GyeotPlan $Description; return }
    & $Action
    Write-GyeotOk $Description
}

try {
    $mode = ''
    if ($DryRun) { $mode = ' (미리보기: 아무것도 바꾸지 않아요)' }
    Write-Host ''
    Write-Host "곁(gyeot) 지우기$mode" -ForegroundColor White
    Write-Host ''

    if (-not $InstallRoot) { $InstallRoot = Get-GyeotDefaultRoot }
    $InstallRoot = Get-GyeotFullPath $InstallRoot
    Write-GyeotInfo "설치 폴더: $InstallRoot"
    if (-not (Test-Path -LiteralPath $InstallRoot)) {
        Write-GyeotWarn '설치 폴더가 없어요. 등록만 정리합니다.'
    }

    # ── 1. 멈추기 ──
    Write-GyeotStep '실행 중인 곁 멈추기'
    $running = Get-GyeotProcessesUnder @('gyeot-shell', 'gyeotd') $InstallRoot
    foreach ($p in $running) {
        $pidText = "$($p.ProcessName) (pid $($p.Id))"
        Invoke-GyeotChange "$pidText 멈춤" { Stop-Process -Id $p.Id -Force }
    }
    if ($running.Count -eq 0) { Write-GyeotInfo '이 설치에서 실행 중인 곁이 없어요.' }
    elseif (-not $DryRun) { Start-Sleep -Milliseconds 500 }

    # ── 2. 로그인 시작 등록 지우기 ──
    Write-GyeotStep '로그인 시 시작 등록 지우기'
    $task = $null
    if (Get-Command Get-ScheduledTask -ErrorAction SilentlyContinue) {
        $task = Get-ScheduledTask -TaskName $script:GyeotTaskName -ErrorAction SilentlyContinue
    }
    if ($task) {
        Invoke-GyeotChange "작업 스케줄러 '$script:GyeotTaskName' 지움" {
            Unregister-ScheduledTask -TaskName $script:GyeotTaskName -Confirm:$false
        }
    } else {
        Write-GyeotInfo "작업 스케줄러에 '$script:GyeotTaskName' 이 없어요."
    }
    foreach ($name in @($script:GyeotRunValueDaemon, $script:GyeotRunValueShell)) {
        $value = Get-GyeotRunValue $name
        if (-not $value) { continue }
        # 따옴표를 벗긴 실행 파일 경로가 이 설치 안을 가리킬 때만 지운다.
        $exe = $value
        if ($exe.StartsWith('"')) { $exe = $exe.Substring(1, [Math]::Max(0, $exe.IndexOf('"', 1) - 1)) }
        $pointsHere = (Test-GyeotPathUnder $exe $InstallRoot) -or ($value.IndexOf($InstallRoot, [StringComparison]::OrdinalIgnoreCase) -ge 0)
        if ($pointsHere) {
            Invoke-GyeotChange "Run 값 '$name' 지움" {
                Remove-ItemProperty -LiteralPath $script:GyeotRunKey -Name $name -Force
            }
        } else {
            Write-GyeotInfo "Run 값 '$name' 은 다른 곳($value)을 가리켜서 그대로 둬요."
        }
    }

    # ── 3. Claude Code 훅(선택) ──
    $claudeSettings = $ClaudeSettingsPath
    if (-not $claudeSettings) { $claudeSettings = Join-Path (Get-GyeotHomeDir) '.claude\settings.json' }
    $claudeSettings = Get-GyeotFullPath $claudeSettings
    if ($RemoveClaudeHooks) {
        Write-GyeotStep "Claude Code 훅에서 곁 항목 빼기 → $claudeSettings"
        if (Test-Path -LiteralPath $claudeSettings) {
            $res = Update-GyeotJsonFile -Path $claudeSettings -DryRun:$DryRun -Mutate {
                param($tree)
                Remove-GyeotClaudeHook -Settings $tree
            }
            if (-not $res.Changed) { Write-GyeotInfo '곁 항목이 없어요.' }
            elseif ($DryRun) { Write-GyeotPlan "백업 후 곁 항목 $($res.Detail)개 뺌" }
            else {
                if ($res.Backup) { Write-GyeotOk "백업: $($res.Backup)" }
                Write-GyeotOk "곁 항목 $($res.Detail)개를 뺐어요. 다른 훅은 그대로예요."
            }
        } else {
            Write-GyeotInfo '설정 파일이 없어요.'
        }
    } else {
        Write-GyeotStep 'Claude Code 훅'
        Write-GyeotInfo "settings.json 의 곁 훅은 그대로 둬요(비동기라 막지는 않지만 실행 실패 기록이 남을 수 있어요)."
        Write-GyeotInfo '빼려면 -RemoveClaudeHooks 를 붙여 다시 실행하세요.'
    }
    Write-GyeotInfo 'Codex 를 연결했다면 ~/.codex/hooks.json 의 gyeot-hook 항목과 config.toml 의 notify 줄도 지우세요.'
    Write-GyeotInfo 'channel 을 등록했다면: claude mcp remove gyeot (또는 .mcp.json 의 "gyeot" 항목 삭제)'

    # ── 4. 파일 지우기 ──
    Write-GyeotStep '파일 지우기'
    if ($PurgeData) {
        if (Test-Path -LiteralPath $InstallRoot) {
            Invoke-GyeotChange "설치 폴더 전체(데이터 포함) 지움: $InstallRoot" {
                Remove-Item -LiteralPath $InstallRoot -Recurse -Force
            }
        }
    } else {
        foreach ($sub in @('bin', 'scripts', 'snippets')) {
            $dir = Join-Path $InstallRoot $sub
            if (Test-Path -LiteralPath $dir) {
                Invoke-GyeotChange "$sub\ 지움" { Remove-Item -LiteralPath $dir -Recurse -Force }
            }
        }
        if (Test-Path -LiteralPath $InstallRoot) {
            Write-GyeotInfo "데이터는 남겨 뒀어요(token·config.toml·events\·learn.json). 모두 지우려면 -PurgeData."
        }
    }

    Write-Host ''
    if ($DryRun) { Write-Host '미리보기를 마쳤어요.' -ForegroundColor White }
    else { Write-Host '곁을 지웠어요.' -ForegroundColor White }
    $exitCode = 0
} catch {
    Write-Host ''
    Write-Host "지우지 못했어요: $($_.Exception.Message)" -ForegroundColor Red
    $exitCode = 1
} finally {
    if ($null -ne $restoreEncoding) { [Console]::OutputEncoding = $restoreEncoding }
}
exit $exitCode
