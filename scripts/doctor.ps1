<#
.SYNOPSIS
    곁(gyeot)이 제대로 설치되고 돌고 있는지 점검합니다. 아무것도 바꾸지 않습니다.
.DESCRIPTION
    파일·판·토큰, 데몬이 "내" 곁 데몬인지(계약 §13.1의 hello 증명), 얼굴 열기 코드,
    Claude Code 훅, 훅 심 실행(stdout 없음·exit 0), channel(Node·claude mcp), 시작 등록,
    폴더 권한을 차례로 봅니다. 실패가 하나라도 있으면 종료 코드 1입니다.
    데몬에 보내는 것은 인증 없는 hello와, 토큰으로 받는 일회용 열기 코드(120초) 하나뿐입니다.
.PARAMETER InstallRoot
    설치 폴더. 기본값은 이 스크립트가 <설치 폴더>\scripts 에 있으면 그 위, 아니면 $env:GYEOT_HOME, %LOCALAPPDATA%\gyeot.
.PARAMETER ClaudeSettingsPath
    Claude Code 사용자 설정 파일. 기본값은 ~/.claude/settings.json.
.PARAMETER NoActivityHook
    install.ps1 -NoActivityHook 으로 설치했다면 주세요(PostToolBatch를 기대하지 않음).
.PARAMETER NoHooks
    훅을 넣지 않은 설치라 훅 점검을 건너뜁니다.
.EXAMPLE
    powershell -NoProfile -ExecutionPolicy Bypass -File "$env:LOCALAPPDATA\gyeot\scripts\doctor.ps1"
#>
[CmdletBinding()]
param(
    [string]$InstallRoot,
    [string]$ClaudeSettingsPath,
    [switch]$NoActivityHook,
    [switch]$NoHooks
)

$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'gyeot-common.ps1')

$restoreEncoding = $null
if ([Console]::IsOutputRedirected) {
    $restoreEncoding = [Console]::OutputEncoding
    [Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false)
}

# 실행 파일을 돌려 종료 코드·출력을 받는다. stdin을 줄 수 있다. 시간 한도를 넘기면 죽이고 실패로 본다.
function Invoke-GyeotTool {
    param([string]$Exe, [string[]]$Arguments, [string]$StdIn, [int]$TimeoutMs = 15000, [hashtable]$Env)
    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = $Exe
    $psi.Arguments = (($Arguments | ForEach-Object { if ($_ -match '[\s"]') { '"' + ($_ -replace '"', '\"') + '"' } else { $_ } }) -join ' ')
    $psi.UseShellExecute = $false
    $psi.CreateNoWindow = $true
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError = $true
    $psi.RedirectStandardInput = $true
    $psi.StandardOutputEncoding = [System.Text.Encoding]::UTF8
    $psi.StandardErrorEncoding = [System.Text.Encoding]::UTF8
    if ($Env) { foreach ($k in $Env.Keys) { $psi.EnvironmentVariables[$k] = $Env[$k] } }
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    $p = [System.Diagnostics.Process]::Start($psi)
    $out = $p.StandardOutput.ReadToEndAsync()
    $err = $p.StandardError.ReadToEndAsync()
    try {
        if ($StdIn) {
            $bytes = (New-Object System.Text.UTF8Encoding($false)).GetBytes($StdIn)
            $p.StandardInput.BaseStream.Write($bytes, 0, $bytes.Length)
        }
        $p.StandardInput.Close()
    } catch { Write-Verbose $_.Exception.Message }
    if (-not $p.WaitForExit($TimeoutMs)) {
        try { $p.Kill() } catch { Write-Verbose $_.Exception.Message }
        return [pscustomobject]@{ Code = -1; Out = ''; Err = "시간 한도($TimeoutMs ms)를 넘겼어요"; Ms = $sw.ElapsedMilliseconds }
    }
    $p.WaitForExit()
    return [pscustomobject]@{ Code = $p.ExitCode; Out = $out.Result; Err = $err.Result; Ms = $sw.ElapsedMilliseconds }
}

function Get-GyeotDoctorRoot([string]$Given) {
    if ($Given) { return (Get-GyeotFullPath $Given) }
    $parent = Split-Path -Parent $PSScriptRoot
    if ((Split-Path -Leaf $PSScriptRoot) -eq 'scripts' -and (Test-Path -LiteralPath (Join-Path $parent 'bin\gyeotd.exe'))) {
        return $parent
    }
    return (Get-GyeotDefaultRoot)
}

$checks = New-Object System.Collections.ArrayList
function Add-GyeotCheck([string]$Name, [string]$Status, [string]$Detail) {
    $c = New-GyeotCheck $Name $Status $Detail
    [void]$checks.Add($c)
    Write-GyeotCheck $c
}

try {
    $root = Get-GyeotDoctorRoot $InstallRoot
    $bin = Join-Path $root 'bin'
    $daemon = Join-Path $bin 'gyeotd.exe'
    $hook = Join-Path $bin 'gyeot-hook.exe'
    $shell = Join-Path $bin 'gyeot-shell.exe'
    $channel = Join-Path $bin 'gyeot-channel.mjs'
    Write-GyeotStep "곁 점검 — 설치 폴더 $root"

    # 1. 파일
    $missing = @()
    foreach ($f in @($daemon, $hook, (Join-Path $bin 'face\index.html'))) {
        if (-not (Test-Path -LiteralPath $f)) { $missing += $f }
    }
    if ($missing.Count -gt 0) { Add-GyeotCheck '파일' 'fail' "없어요: $($missing -join ', ') — bootstrap.ps1 -Install 로 다시 설치하세요" }
    else { Add-GyeotCheck '파일' 'ok' 'gyeotd.exe · gyeot-hook.exe · bin\face\index.html' }
    if (Test-Path -LiteralPath $shell) { Add-GyeotCheck '셸' 'ok' 'gyeot-shell.exe 있음' }
    else { Add-GyeotCheck '셸' 'warn' 'gyeot-shell.exe 가 없어요 — 트레이 아바타 없이 브라우저(gyeotd --open)로만 써요' }
    if (-not (Test-Path -LiteralPath $daemon)) { throw '데몬 실행 파일이 없어 나머지를 점검할 수 없어요.' }

    # 2. 판
    $v = Invoke-GyeotTool $daemon @('--version')
    if ($v.Code -eq 0 -and $v.Out -match 'gyeotd\s+(\S+)') { Add-GyeotCheck '판' 'ok' "gyeotd $($Matches[1])" }
    else { Add-GyeotCheck '판' 'fail' "gyeotd --version 이 실패했어요(코드 $($v.Code)) $($v.Err.Trim())" }

    # 3. 설정·토큰(데몬과 같은 해석)
    $pc = Invoke-GyeotTool $daemon @('--print-config')
    $cfg = ConvertFrom-GyeotPrintConfig ($pc.Out + "`n" + $pc.Err)
    $token = $null
    if ($pc.Code -ne 0) {
        Add-GyeotCheck '설정' 'fail' "gyeotd --print-config 가 실패했어요: $($pc.Err.Trim())"
    } else {
        Add-GyeotCheck '설정' 'ok' "데이터 폴더 $($cfg.Home) · 포트 $($cfg.Port)"
        if ($cfg.Token -and (Test-Path -LiteralPath $cfg.Token)) {
            $token = ([System.IO.File]::ReadAllText($cfg.Token)).Trim()
            if ($token.Length -ge 32) { Add-GyeotCheck '토큰' 'ok' $cfg.Token }
            else { Add-GyeotCheck '토큰' 'fail' "토큰 파일이 비었거나 짧아요: $($cfg.Token)"; $token = $null }
        } else {
            Add-GyeotCheck '토큰' 'fail' "토큰 파일이 없어요($($cfg.Token)) — install.ps1 이 gyeotd --init 을 하지 않았어요"
        }
        if ($cfg.Face -and -not (Test-Path -LiteralPath (Join-Path $cfg.Face 'index.html'))) {
            Add-GyeotCheck '얼굴 폴더' 'warn' "데몬이 보는 얼굴 폴더에 index.html 이 없어요: $($cfg.Face)"
        }
    }

    # 4. 데몬이 "내" 곁 데몬인가(§13.1)
    $mine = $false
    if ($token) {
        $n = New-GyeotNonce
        $resp = $null
        $helloErr = ''
        try { $resp = Get-GyeotLoopbackText -Port $cfg.Port -PathAndQuery "/v1/hello?n=$n" } catch { $resp = $null; $helloErr = $_.Exception.Message }
        $hello = $null
        if ($resp -and $resp.Status -eq 200) { try { $hello = $resp.Body | ConvertFrom-Json } catch { $hello = $null } }
        $proof = $null
        if ($hello -and ($hello.PSObject.Properties.Name -contains 'proof')) { $proof = [string]$hello.proof }
        if ($null -eq $resp) {
            Add-GyeotCheck '데몬' 'fail' "127.0.0.1:$($cfg.Port) 에 닿지 않아요(아무것도 듣고 있지 않음) — 셸을 띄우거나 `"$daemon`" 를 실행하세요 ($helloErr)"
        } elseif (-not $proof) {
            # 연결은 됐는데 곁 hello 가 아니다: 다른 프로그램이 포트를 잡았다. 곁이 그 포트에 토큰을 보내면 안 된다.
            Add-GyeotCheck '데몬' 'fail' "127.0.0.1:$($cfg.Port) 을 다른 프로그램이 쓰고 있어요(곁 hello 응답이 아님, HTTP $($resp.Status)) — 그 프로그램을 끄거나 config.toml 의 port 를 바꾸세요"
        } elseif (Test-GyeotSameHex $proof (Get-GyeotHelloProof $token $cfg.Port $n)) {
            $mine = $true; Add-GyeotCheck '데몬' 'ok' "127.0.0.1:$($cfg.Port) 은 내 곁 데몬(hello 증명 일치)"
        } else {
            Add-GyeotCheck '데몬' 'fail' "127.0.0.1:$($cfg.Port) 의 곁 데몬이 내 토큰을 몰라요 — 다른 사용자나 다른 설치의 데몬이 포트를 잡았을 수 있어요"
        }
    } else {
        Add-GyeotCheck '데몬' 'skip' '토큰이 없어 확인하지 않았어요'
    }

    # 5. 얼굴 열기 코드
    if ($mine) {
        $o = Invoke-GyeotTool $daemon @('--open', '--print')
        if ($o.Code -eq 0 -and $o.Out -match "^http://127\.0\.0\.1:$($cfg.Port)/\?boot=[0-9a-f]{64}") { Add-GyeotCheck '얼굴 열기' 'ok' '일회용 열기 코드를 받았어요(120초 뒤 사라짐)' }
        else { Add-GyeotCheck '얼굴 열기' 'fail' "gyeotd --open --print 가 실패했어요: $($o.Err.Trim())" }
    } else {
        Add-GyeotCheck '얼굴 열기' 'skip' '데몬 확인이 안 돼 건너뛰었어요'
    }

    # 6. Claude Code 훅
    # 곁 처리기가 하나도 없으면 훅 없이 설치한 것으로 보고 경고만 한다. 있으면 이벤트마다 이 설치의
    # gyeot-hook 를 불러야 하고, 없는 파일을 부르는 처리기가 있으면 실패(그 훅은 매번 실패한다).
    $hookAdvice = 'bootstrap.ps1 -ApplyClaudeHooks -Install 로 다시 설치하세요(처음에 준 다른 선택도 함께)'
    if ($NoHooks) {
        Add-GyeotCheck 'Claude 훅' 'skip' '-NoHooks'
    } else {
        if (-not $ClaudeSettingsPath) { $ClaudeSettingsPath = Join-Path (Join-Path (Get-GyeotHomeDir) '.claude') 'settings.json' }
        $handlers = @()
        if (Test-Path -LiteralPath $ClaudeSettingsPath) {
            $tree = ConvertFrom-GyeotJsonText (Read-GyeotTextFile $ClaudeSettingsPath)
            $handlers = @(Get-GyeotHookHandlerList $tree)
        }
        if ($handlers.Count -eq 0) {
            Add-GyeotCheck 'Claude 훅' 'warn' "곁 훅이 없어요($ClaudeSettingsPath) — 훅 없이 설치했다면 그대로 두세요. 넣으려면 $hookAdvice"
        } else {
            $broken = @()
            $other = @()
            foreach ($h in $handlers) {
                $exe = $h.Exe
                if (Test-GyeotSamePath $exe $hook) { continue }
                $exists = $false
                if ($exe -match '[\\/]') { try { $exists = Test-Path -LiteralPath $exe -PathType Leaf } catch { $exists = $false } }
                else { $exists = [bool](Get-Command $exe -ErrorAction SilentlyContinue) }
                if ($exists) { if ($other -notcontains $exe) { $other += $exe } }
                elseif ($broken -notcontains $exe) { $broken += $exe }
            }
            $lack = @()
            foreach ($ev in (Get-GyeotClaudeEventList -NoActivity:$NoActivityHook)) {
                $mineHere = @($handlers | Where-Object { $_.Event -ceq $ev.Name -and (Test-GyeotSamePath $_.Exe $hook) })
                if ($mineHere.Count -lt 1) { $lack += $ev.Name }
            }
            $expected = @(Get-GyeotClaudeEventList -NoActivity:$NoActivityHook).Count
            $activity = $script:GyeotClaudeActivityEvent.Name
            if ($lack.Count -eq 1 -and $lack[0] -ceq $activity) {
                # -NoActivityHook 으로 설치한 모양: 활동 훅만 없다.
                Add-GyeotCheck 'Claude 훅' 'warn' "활동 훅($activity)이 없어요 — -NoActivityHook 으로 설치했다면 정상이에요(그때는 doctor.ps1 -NoActivityHook)"
                $lack = @()
                $expected--
            }
            if ($broken.Count -gt 0) { Add-GyeotCheck 'Claude 훅' 'fail' "없는 파일을 부르는 곁 훅이 있어요: $($broken -join ', ') — $hookAdvice. 손으로 넣은 줄이면 settings.json 에서 지우세요" }
            if ($lack.Count -gt 0) { Add-GyeotCheck 'Claude 훅' 'fail' "이 설치의 gyeot-hook 를 부르지 않는 이벤트: $($lack -join ', ') — $hookAdvice" }
            if ($other.Count -gt 0) { Add-GyeotCheck 'Claude 훅' 'warn' "다른 설치의 gyeot-hook 도 불러요(이벤트가 두 번 갈 수 있어요): $($other -join ', ')" }
            if ($broken.Count -eq 0 -and $lack.Count -eq 0) { Add-GyeotCheck 'Claude 훅' 'ok' "$expected 개 이벤트가 $hook 를 불러요" }
        }
    }

    # 7. 훅 심 실행(불변식: stdout 없음, exit 0, 오래 걸리지 않음)
    if (Test-Path -LiteralPath $hook) {
        $h = Invoke-GyeotTool $hook @('claude-code') -StdIn '{}' -TimeoutMs 5000
        if ($h.Code -eq 0 -and $h.Out.Length -eq 0 -and $h.Ms -lt 2000) { Add-GyeotCheck '훅 심' 'ok' "exit 0 · stdout 없음 · $($h.Ms) ms" }
        else { Add-GyeotCheck '훅 심' 'fail' "exit $($h.Code) · stdout $($h.Out.Length)바이트 · $($h.Ms) ms" }
    }

    # 8. channel(대화, 선택)
    $node = Get-Command node -ErrorAction SilentlyContinue
    if (-not (Test-Path -LiteralPath $channel)) {
        Add-GyeotCheck 'channel' 'warn' 'gyeot-channel.mjs 가 없어 대화 기능은 못 써요'
    } elseif (-not $node) {
        Add-GyeotCheck 'channel' 'warn' 'Node.js 가 없어요 — 대화(channel)를 쓰려면 Node 22 이상을 설치하세요'
    } else {
        $nv = Invoke-GyeotTool $node.Source @('--version')
        $major = Get-GyeotNodeMajor $nv.Out
        if ($major -lt 22) { Add-GyeotCheck 'channel' 'warn' "Node $($nv.Out.Trim()) — 22 이상이 필요해요" }
        else {
            $claudeCmd = Get-Command claude -ErrorAction SilentlyContinue
            if (-not $claudeCmd) { Add-GyeotCheck 'channel' 'warn' "Node $($nv.Out.Trim()) · claude CLI를 찾지 못해 MCP 등록은 확인하지 못했어요" }
            else {
                # claude 는 claude.exe 일 수도 npm 의 claude.cmd 일 수도 있어 cmd 를 거친다(Windows).
                if ($env:ComSpec) { $g = Invoke-GyeotTool $env:ComSpec @('/d', '/c', 'claude', 'mcp', 'get', 'gyeot') -TimeoutMs 30000 }
                else { $g = Invoke-GyeotTool $claudeCmd.Source @('mcp', 'get', 'gyeot') -TimeoutMs 30000 }
                if ($g.Code -eq 0 -and ($g.Out -match [regex]::Escape('gyeot-channel.mjs'))) { Add-GyeotCheck 'channel' 'ok' "Node $($nv.Out.Trim()) · claude mcp 에 gyeot 등록됨" }
                else { Add-GyeotCheck 'channel' 'warn' "claude mcp 에 gyeot 가 없어요 — bootstrap.ps1 -RegisterChannel 또는: claude mcp add --transport stdio --scope user gyeot -- node `"$channel`"" }
            }
        }
    }

    # 9. 로그인할 때 시작(정보)
    if (Test-GyeotWindowsHost) {
        $task = $null
        try { $task = Get-ScheduledTask -TaskName $script:GyeotTaskName -ErrorAction Stop } catch { $task = $null }
        $runD = Get-GyeotRunValue $script:GyeotRunValueDaemon
        $runS = Get-GyeotRunValue $script:GyeotRunValueShell
        $parts = @()
        if ($task) { $parts += '데몬(작업 스케줄러)' } elseif ($runD) { $parts += '데몬(Run 값)' }
        if ($runS) { $parts += '셸(트레이의 로그인 시 시작)' }
        if ($parts.Count -gt 0) { Add-GyeotCheck '로그인 시 시작' 'ok' ($parts -join ' · ') }
        else { Add-GyeotCheck '로그인 시 시작' 'skip' '등록하지 않았어요(install.ps1 -RegisterStartup, 또는 셸 트레이의 "로그인 시 시작")' }
    } else {
        Add-GyeotCheck '로그인 시 시작' 'skip' 'Windows가 아니에요'
    }

    # 10. 폴더 권한(§13.4)
    $folders = @($root)
    if ($cfg.Home) { $folders += $cfg.Home }
    $plan = @(Get-GyeotPrivateFolderPlan $folders)
    if ($plan.Count -eq 0) {
        Add-GyeotCheck '폴더 권한' 'ok' '%LOCALAPPDATA% 안(그 사용자 전용 권한을 이어받음)'
    } elseif (-not (Test-GyeotWindowsHost)) {
        Add-GyeotCheck '폴더 권한' 'skip' 'Windows가 아니에요'
    } else {
        $userSid = Get-GyeotCurrentUserSid
        $bad = @()
        foreach ($f in $plan) {
            if (-not (Test-Path -LiteralPath $f)) { continue }
            $problems = @(Get-GyeotAclProblem -Summary (Get-GyeotAclSummary $f) -UserSid $userSid)
            if ($problems.Count -gt 0) { $bad += "$f($($problems -join '; '))" }
        }
        if ($bad.Count -gt 0) { Add-GyeotCheck '폴더 권한' 'fail' "다른 사용자가 읽거나 바꿀 수 있어요: $($bad -join ', ') — install.ps1 을 다시 하세요" }
        else { Add-GyeotCheck '폴더 권한' 'ok' "$($plan -join ', ') 은 그 사용자와 SYSTEM만" }
    }

    $fails = @($checks | Where-Object { $_.Status -eq 'fail' }).Count
    $warns = @($checks | Where-Object { $_.Status -eq 'warn' }).Count
    Write-Host ''
    if ($fails -eq 0) {
        Write-Host "점검 통과 — 실패 0, 경고 $warns" -ForegroundColor White
        $exitCode = 0
    } else {
        Write-Host "점검 실패 — 실패 $fails, 경고 $warns" -ForegroundColor Red
        $exitCode = 1
    }
} catch {
    Write-Host ''
    Write-Host "점검하지 못했어요: $($_.Exception.Message)" -ForegroundColor Red
    $exitCode = 1
} finally {
    if ($null -ne $restoreEncoding) { [Console]::OutputEncoding = $restoreEncoding }
}
exit $exitCode
