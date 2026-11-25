<#
.SYNOPSIS
    install.ps1 · uninstall.ps1 자체 시험(CI용). 임시 폴더에 설치·훅 병합·되풀이·지우기를 해 보고 결과를 확인한다.

.PARAMETER SourceDir
    설치할 파일이 있는 폴더(zip 모양: bin\, face\). 필수.

.PARAMETER SkipInit
    gyeotd --init 을 건너뛴다(가짜 실행 파일로 시험할 때).

.PARAMETER WithStartup
    -RegisterStartup 도 시험한다(작업 스케줄러에 gyeot-daemon 을 등록했다가 지운다). CI 러너 전용.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$SourceDir,
    [switch]$SkipInit,
    [switch]$WithStartup
)

$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'gyeot-common.ps1')

$script:failures = 0
function Assert-Gyeot([bool]$Condition, [string]$Message) {
    if ($Condition) { Write-Host "  ok   $Message" -ForegroundColor Green }
    else { Write-Host "  FAIL $Message" -ForegroundColor Red; $script:failures++ }
}

# 지금 도는 것과 같은 판의 PowerShell로 자식 스크립트를 돌린다(5.1이면 powershell.exe, 7이면 pwsh).
function Get-GyeotPowerShellExe {
    if ($PSVersionTable.PSEdition -eq 'Desktop') { return (Join-Path $PSHOME 'powershell.exe') }
    foreach ($name in @('pwsh.exe', 'pwsh')) {
        $candidate = Join-Path $PSHOME $name
        if (Test-Path -LiteralPath $candidate -PathType Leaf) { return $candidate }
    }
    return (Get-Command pwsh -ErrorAction Stop).Source
}

function Invoke-GyeotScript([string]$Name, [string[]]$Arguments) {
    $exe = Get-GyeotPowerShellExe
    Write-Host ''
    Write-Host ">> $Name $($Arguments -join ' ')" -ForegroundColor Cyan
    & $exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot $Name) @Arguments | Out-Host
    return $LASTEXITCODE
}

# 자식 스크립트의 표준 출력을 글로 받는다. install.ps1은 출력이 넘겨질 때 UTF-8로 쓰므로 여기서도 UTF-8로 읽는다.
# 표준 오류는 받지 않는다(Windows PowerShell 5.1은 Stop 아래에서 넘겨받은 stderr 한 줄을 종료 오류로 바꾼다).
function Invoke-GyeotScriptOutput([string]$Name, [string[]]$Arguments) {
    $ErrorActionPreference = 'Continue'
    $exe = Get-GyeotPowerShellExe
    Write-Host ''
    Write-Host ">> $Name $($Arguments -join ' ')" -ForegroundColor Cyan
    $prevEncoding = [Console]::OutputEncoding
    try { [Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false) } catch { Write-Verbose "출력 인코딩을 바꾸지 못함: $($_.Exception.Message)" }
    try {
        $lines = & $exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot $Name) @Arguments
        $code = $LASTEXITCODE
    } finally {
        try { [Console]::OutputEncoding = $prevEncoding } catch { Write-Verbose "출력 인코딩을 되돌리지 못함: $($_.Exception.Message)" }
    }
    $text = ($lines | Out-String)
    Write-Host $text
    return @{ Code = $code; Text = $text }
}

function Read-GyeotTree([string]$Path) {
    return , (ConvertFrom-GyeotJsonText -Text (Read-GyeotTextFile $Path) -What $Path)
}

# 시험 안의 경로 조립(Windows에서는 Join-Path와 같다. 다른 OS의 pwsh로 돌려 볼 때도 \\를 구분자로 바꾼다).
function Join-GyeotTestPath([string]$Root, [string]$Relative) {
    return [System.IO.Path]::Combine($Root, ($Relative -replace '\\', [System.IO.Path]::DirectorySeparatorChar))
}

function Get-GyeotFileHash([string]$Path) {
    return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash
}

$tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("gyeot-selftest-" + [guid]::NewGuid().ToString('N'))
$root = Join-Path $tmp 'root'
$home2 = Join-Path $tmp 'home'
$settings = Join-GyeotTestPath $home2 '.claude\settings.json'
New-Item -ItemType Directory -Path (Split-Path -Parent $settings) -Force | Out-Null

# 이미 다른 훅(fsmap·AOK 비슷한 것)과 여러 설정이 있는 사용자 설정
$original = @'
{
  "model": "opus",
  "permissions": {
    "allow": ["Bash(git status)", "Read(./src/**)"],
    "deny": []
  },
  "env": { "GREETING": "안녕하세요 \"곁\"\\n", "RATIO": 0.5, "BIG": 12345678901, "NESTED": { "on": true, "off": false, "nothing": null } },
  "hooks": {
    "Stop": [
      { "hooks": [ { "type": "command", "command": "fsmap-hook stop", "timeout": 30 } ] }
    ],
    "PreToolUse": [
      { "matcher": "Bash", "hooks": [ { "type": "command", "command": "aok-guard pre-bash" } ] }
    ],
    "PostToolBatch": [
      { "hooks": [ { "type": "command", "command": "batch-logger" } ] }
    ],
    "SessionStart": []
  },
  "statusLine": { "type": "command", "command": "echo hi" }
}
'@
[System.IO.File]::WriteAllText($settings, ($original -replace "`r`n", "`n"), (New-Object System.Text.UTF8Encoding($false)))
$origTree = Read-GyeotTree $settings
$origOther = New-GyeotDict
foreach ($k in $origTree.Keys) { if ($k -ne 'hooks') { $origOther[$k] = $origTree[$k] } }
$origOtherJson = ConvertTo-GyeotJson $origOther

$common = @('-SourceDir', $SourceDir, '-InstallRoot', $root, '-ClaudeSettingsPath', $settings)
if ($SkipInit) { $common += '-NoInit' }

try {
    Write-Host '== 0. examples\ 견본이 설치 스크립트의 이벤트 표와 같다' -ForegroundColor White
    $examples = $null
    foreach ($c in @((Join-Path $SourceDir 'examples'), (Join-Path (Split-Path -Parent $PSScriptRoot) 'examples'))) {
        if (Test-Path -LiteralPath (Join-Path $c 'claude-settings.hooks.json')) { $examples = $c; break }
    }
    Assert-Gyeot ($null -ne $examples) 'examples\ 폴더를 찾았다'
    if ($examples) {
        $ex = Read-GyeotTree (Join-Path $examples 'claude-settings.hooks.json')
        $want = @(Get-GyeotClaudeEventList | ForEach-Object { $_.Name })
        $got = @($ex['hooks'].Keys)
        Assert-Gyeot ((($got | Sort-Object) -join ',') -eq (($want | Sort-Object) -join ',')) "Claude 견본 이벤트 = 기본 설치 이벤트 ($($got -join ','))"
        Assert-Gyeot ($want -contains 'PostToolBatch') '기본 설치 이벤트에 PostToolBatch가 있다(§12.3)'
        Assert-Gyeot (-not (@(Get-GyeotClaudeEventList -NoActivity | ForEach-Object { $_.Name }) -contains 'PostToolBatch')) '-NoActivity 목록에는 PostToolBatch가 없다'
        foreach ($ev in (Get-GyeotClaudeEventList)) {
            if (-not $ex['hooks'].Contains($ev.Name)) { continue }
            $grp = $ex['hooks'][$ev.Name][0]
            Assert-Gyeot ($grp.Contains('matcher') -eq [bool]$ev.Matcher) "Claude 견본 matcher 유무: $($ev.Name)"
            $hh = $grp['hooks'][0]
            Assert-Gyeot ($hh['args'][0] -eq 'claude-code' -and $hh['async'] -eq $true) "Claude 견본 처리기 모양: $($ev.Name)"
        }
        $cx = Read-GyeotTree (Join-Path $examples 'codex-hooks.json')
        Assert-Gyeot (((@($cx['hooks'].Keys) | Sort-Object) -join ',') -eq ((@($script:GyeotCodexEvents) | Sort-Object) -join ',')) 'Codex 견본 이벤트 = 설치 이벤트'
        $mx = Read-GyeotTree (Join-Path $examples 'mcp.json')
        Assert-Gyeot ($mx['mcpServers'].Contains('gyeot')) 'mcp 견본 서버 이름 gyeot'
        $cfgPath = Join-Path $examples 'gyeot-config.toml'
        Assert-Gyeot (Test-Path -LiteralPath $cfgPath) '설정 견본(gyeot-config.toml)이 있다'
        if (Test-Path -LiteralPath $cfgPath) {
            $cfgText = Read-GyeotTextFile $cfgPath
            foreach ($key in @('port', 'stale_minutes', 'nudge_min', 'interrupt_min', 'approval_interrupt', 'quiet_hours', 'utc_offset_minutes', 'retention_days')) {
                Assert-Gyeot ($cfgText -match "(?m)^#?[ \t]*$key[ \t]*=") "설정 견본에 맨 위 키 $key 가 있다(§6)"
            }
            Assert-Gyeot ($cfgText -match '(?m)^#?[ \t]*approval_interrupt[ \t]*=[ \t]*false[ \t]*$') '설정 견본의 approval_interrupt 는 기본값 false(§13.3)'
            $firstTable = $cfgText.IndexOf("`n[")
            $commentedTable = $cfgText.IndexOf("`n# [[tail]]")
            $ai = $cfgText.IndexOf('approval_interrupt =')
            Assert-Gyeot ($ai -ge 0 -and ($firstTable -lt 0 -or $ai -lt $firstTable) -and ($commentedTable -lt 0 -or $ai -lt $commentedTable)) 'approval_interrupt 는 [[tail]] 표보다 앞(맨 위 키)'
        }
    }

    Write-Host '== 0b. 폴더 권한 계획·판정(§13.4) — OS와 상관없는 함수' -ForegroundColor White
    $savedLad = $env:LOCALAPPDATA
    try {
        $lad = Join-GyeotTestPath ([System.IO.Path]::GetTempPath()) 'fake-user\AppData\Local'
        $env:LOCALAPPDATA = $lad
        $inside = Join-GyeotTestPath $lad 'gyeot'
        $outsideDir = Join-GyeotTestPath ([System.IO.Path]::GetTempPath()) 'gyeot-elsewhere'
        Assert-Gyeot (@(Get-GyeotPrivateFolderPlan @($inside, $null, '')).Count -eq 0) '%LOCALAPPDATA% 안은 좁히지 않는다'
        $plan = @(Get-GyeotPrivateFolderPlan @($outsideDir, $inside, ($outsideDir + [System.IO.Path]::DirectorySeparatorChar)))
        Assert-Gyeot ($plan.Count -eq 1 -and $plan[0] -eq $outsideDir) "%LOCALAPPDATA% 밖만, 한 번씩: $($plan -join ' | ')"
        Assert-Gyeot (@(Get-GyeotPrivateFolderPlan @(($lad + '-other'))).Count -eq 1) '이름만 비슷한 옆 폴더(Local-other)는 밖이다'
        $env:LOCALAPPDATA = ''
        Assert-Gyeot (@(Get-GyeotPrivateFolderPlan @($inside)).Count -eq 1) 'LOCALAPPDATA를 모르면 밖으로 본다(좁히는 쪽)'
    } finally {
        $env:LOCALAPPDATA = $savedLad
    }
    $want = @('C:\gyeot', '/inheritance:r', '/grant:r', '*S-1-5-21-1-2-3-1001:(OI)(CI)F', '/grant:r', '*S-1-5-18:(OI)(CI)F')
    $gotArgs = @(Get-GyeotIcaclsArgument 'C:\gyeot' 'S-1-5-21-1-2-3-1001')
    Assert-Gyeot (($gotArgs -join ' ') -ceq ($want -join ' ')) "icacls 인자 = §13.4 ($($gotArgs -join ' '))"
    $me = 'S-1-5-21-1-2-3-1001'
    function New-GyeotTestRule([string]$Sid, [string]$Type = 'Allow', [string]$Rights = 'FullControl') {
        return [pscustomobject]@{ Sid = $Sid; Type = $Type; Rights = $Rights; Inherited = $false }
    }
    function New-GyeotTestSummary([object[]]$Rules, [bool]$Protected = $true, [string]$Owner = $me) {
        return [pscustomobject]@{ Protected = $Protected; Rules = $Rules; Owner = $Owner }
    }
    $good = New-GyeotTestSummary @((New-GyeotTestRule $me), (New-GyeotTestRule 'S-1-5-18'))
    Assert-Gyeot (@(Get-GyeotAclProblem -Summary $good -UserSid $me).Count -eq 0) '그 사용자·SYSTEM 모든 권한 + 상속 끊김 = 문제 없음'
    $adminOwned = New-GyeotTestSummary @((New-GyeotTestRule $me), (New-GyeotTestRule 'S-1-5-18')) $true 'S-1-5-32-544'
    Assert-Gyeot (@(Get-GyeotAclProblem -Summary $adminOwned -UserSid $me).Count -eq 0) '주인이 Administrators면 받아들인다(관리자 셸에서 만든 폴더)'
    foreach ($case in @(
            @{ Name = '다른 주체(Authenticated Users)가 남음'; S = (New-GyeotTestSummary @((New-GyeotTestRule $me), (New-GyeotTestRule 'S-1-5-18'), (New-GyeotTestRule 'S-1-5-11' 'Allow' 'Modify'))) },
            @{ Name = '상속이 끊기지 않음'; S = (New-GyeotTestSummary @((New-GyeotTestRule $me), (New-GyeotTestRule 'S-1-5-18')) $false) },
            @{ Name = 'SYSTEM이 없음'; S = (New-GyeotTestSummary @(, (New-GyeotTestRule $me))) },
            @{ Name = '그 사용자 권한이 모든 권한이 아님'; S = (New-GyeotTestSummary @((New-GyeotTestRule $me 'Allow' 'ReadAndExecute'), (New-GyeotTestRule 'S-1-5-18'))) },
            @{ Name = '거부 항목'; S = (New-GyeotTestSummary @((New-GyeotTestRule $me), (New-GyeotTestRule 'S-1-5-18'), (New-GyeotTestRule $me 'Deny' 'Write'))) },
            @{ Name = '주인이 다른 사용자'; S = (New-GyeotTestSummary @((New-GyeotTestRule $me), (New-GyeotTestRule 'S-1-5-18')) $true 'S-1-5-21-9-9-9-1002') }
        )) {
        Assert-Gyeot (@(Get-GyeotAclProblem -Summary $case.S -UserSid $me).Count -gt 0) "문제로 본다: $($case.Name)"
    }

    Write-Host '== 1. 미리보기는 아무것도 바꾸지 않는다' -ForegroundColor White
    $before = Get-GyeotFileHash $settings
    $code = Invoke-GyeotScript 'install.ps1' ($common + @('-DryRun', '-ApplyClaudeHooks', '-RegisterStartup'))
    Assert-Gyeot ($code -eq 0) "DryRun 종료 코드 0 (받은 값 $code)"
    Assert-Gyeot (-not (Test-Path -LiteralPath $root)) 'DryRun은 설치 폴더를 만들지 않는다'
    Assert-Gyeot ((Get-GyeotFileHash $settings) -eq $before) 'DryRun은 settings.json을 바꾸지 않는다'
    Assert-Gyeot (@(Get-ChildItem -LiteralPath (Split-Path -Parent $settings) -Filter 'settings.json.bak-*').Count -eq 0) 'DryRun은 백업을 만들지 않는다'

    Write-Host '== 2. 설치 + 훅 적용' -ForegroundColor White
    $args2 = $common + @('-ApplyClaudeHooks')
    if ($WithStartup) { $args2 += @('-RegisterStartup', '-NoStart') }
    $run = Invoke-GyeotScriptOutput 'install.ps1' $args2
    $code = $run.Code
    Assert-Gyeot ($code -eq 0) "설치 종료 코드 0 (받은 값 $code)"
    Assert-Gyeot ($run.Text -match 'gyeotd\.exe"? --open') '설치 출력이 얼굴 여는 법(gyeotd --open)을 알려 준다'
    if ($run.Text -match '설치를 마쳤어요') {
        Assert-Gyeot ($run.Text -match '곁 열기') "설치 출력이 트레이의 '곁 열기'를 알려 준다"
    } else {
        Write-Host '  skip 이 콘솔에서 자식 출력의 한국어를 읽지 못해 문구 확인을 건너뜀' -ForegroundColor Yellow
    }
    foreach ($f in @('bin\gyeotd.exe', 'bin\gyeot-hook.exe', 'bin\face\index.html', 'scripts\uninstall.ps1', 'scripts\gyeot-common.ps1',
            'snippets\claude-settings.hooks.json', 'snippets\codex-hooks.json', 'snippets\codex-config.notify.toml', 'snippets\mcp.json')) {
        Assert-Gyeot (Test-Path -LiteralPath (Join-GyeotTestPath $root $f)) "설치됨: $f"
    }
    if (-not $SkipInit) {
        $tokenPath = Join-Path $root 'token'
        Assert-Gyeot (Test-Path -LiteralPath $tokenPath) 'gyeotd --init 이 토큰을 만들었다'
        if (Test-Path -LiteralPath $tokenPath) {
            Assert-Gyeot ((Read-GyeotTextFile $tokenPath).Trim() -match '^[0-9a-f]{64}$') '토큰은 소문자 hex 64자'
        }
    }
    $hookExe = Join-GyeotTestPath $root 'bin\gyeot-hook.exe'
    $tree = Read-GyeotTree $settings
    foreach ($ev in (Get-GyeotClaudeEventList)) {
        Assert-Gyeot ((Get-GyeotHookCount -Settings $tree -EventName $ev.Name) -eq 1) "곁 훅 1개: $($ev.Name)"
    }
    Assert-Gyeot ((Get-GyeotHookCount -Settings $tree -EventName 'PostToolBatch') -eq 1) 'PostToolBatch는 기본으로 넣는다(승인 뒤 대기를 닫음)'
    $batch = $tree['hooks']['PostToolBatch']
    Assert-Gyeot ($batch.Count -eq 2 -and $batch[0]['hooks'][0]['command'] -eq 'batch-logger') 'PostToolBatch: 기존 묶음이 그대로(첫 자리) + 곁 묶음'
    Assert-Gyeot ($batch.Count -eq 2 -and -not $batch[1].Contains('matcher')) 'PostToolBatch 곁 묶음에는 matcher가 없다'
    $snip = Read-GyeotTree (Join-GyeotTestPath $root 'snippets\claude-settings.hooks.json')
    Assert-Gyeot ($snip['hooks'].Contains('PostToolBatch')) '출력 조각에도 PostToolBatch가 있다'
    $stopGroups = $tree['hooks']['Stop']
    Assert-Gyeot ($stopGroups.Count -eq 2) 'Stop: 기존 묶음 + 곁 묶음'
    Assert-Gyeot ($stopGroups[0]['hooks'][0]['command'] -eq 'fsmap-hook stop') 'Stop: 기존 fsmap 훅이 그대로(첫 자리)'
    $pre = $tree['hooks']['PreToolUse']
    Assert-Gyeot ($pre.Count -eq 1 -and $pre[0]['hooks'][0]['command'] -eq 'aok-guard pre-bash') 'PreToolUse(곁이 쓰지 않음)는 그대로'
    $g = $tree['hooks']['Notification'][0]
    Assert-Gyeot ($g['matcher'] -eq '*') 'Notification 묶음 matcher = *'
    $h = $g['hooks'][0]
    Assert-Gyeot ($h['type'] -eq 'command' -and $h['command'] -eq $hookExe) '처리기 command = 설치된 gyeot-hook.exe 절대 경로'
    Assert-Gyeot ($h['args'].Count -eq 1 -and $h['args'][0] -eq 'claude-code') '처리기 args = ["claude-code"] (exec 형식)'
    Assert-Gyeot ($h['async'] -eq $true -and $h['timeout'] -eq 10) '처리기 async=true, timeout=10'
    Assert-Gyeot (-not $tree['hooks']['Stop'][1].Contains('matcher')) 'matcher가 없는 이벤트(Stop)에는 matcher를 넣지 않는다'
    $other = New-GyeotDict
    foreach ($k in $tree.Keys) { if ($k -ne 'hooks') { $other[$k] = $tree[$k] } }
    Assert-Gyeot ((ConvertTo-GyeotJson $other) -eq $origOtherJson) 'hooks 밖의 설정은 값·순서 그대로'
    Assert-Gyeot ($tree['env']['GREETING'] -eq "안녕하세요 `"곁`"\n") '한글·따옴표·역슬래시 문자열 그대로'
    $backups = @(Get-ChildItem -LiteralPath (Split-Path -Parent $settings) -Filter 'settings.json.bak-*')
    Assert-Gyeot ($backups.Count -eq 1) '백업 하나'
    if ($backups.Count -ge 1) {
        Assert-Gyeot ((Get-GyeotFileHash $backups[0].FullName) -eq $before) '백업은 원래 파일과 같다'
    }
    $bytes = [System.IO.File]::ReadAllBytes($settings)
    Assert-Gyeot (-not ($bytes.Length -ge 3 -and $bytes[0] -eq 0xEF -and $bytes[1] -eq 0xBB -and $bytes[2] -eq 0xBF)) 'settings.json에 BOM이 없다'
    $mcp = Read-GyeotTree (Join-GyeotTestPath $root 'snippets\mcp.json')
    Assert-Gyeot ($mcp['mcpServers']['gyeot']['command'] -eq 'node') 'mcp.json: node로 channel 실행'
    $notify = Read-GyeotTextFile (Join-GyeotTestPath $root 'snippets\codex-config.notify.toml')
    Assert-Gyeot ($notify -match "^notify = \['.*gyeot-hook\.exe', 'codex-notify'\]") "codex notify 줄 모양: $($notify.Trim())"
    if ($WithStartup) {
        $task = Get-ScheduledTask -TaskName $script:GyeotTaskName -ErrorAction SilentlyContinue
        $runValue = Get-GyeotRunValue $script:GyeotRunValueDaemon
        Assert-Gyeot ($null -ne $task -or $runValue) '로그인 시작 등록(작업 스케줄러 또는 Run 값)'
    }

    Write-Host '== 3. 다시 설치해도 같다(멱등)' -ForegroundColor White
    $afterFirst = Get-GyeotFileHash $settings
    $code = Invoke-GyeotScript 'install.ps1' ($common + @('-ApplyClaudeHooks'))
    Assert-Gyeot ($code -eq 0) "두 번째 설치 종료 코드 0 (받은 값 $code)"
    Assert-Gyeot ((Get-GyeotFileHash $settings) -eq $afterFirst) '두 번째 적용은 settings.json을 바꾸지 않는다'
    Assert-Gyeot (@(Get-ChildItem -LiteralPath (Split-Path -Parent $settings) -Filter 'settings.json.bak-*').Count -eq 1) '바뀐 것이 없으면 백업도 더 만들지 않는다'

    Write-Host '== 4. -NoActivityHook 은 곁의 PostToolBatch만 뺀다, 기본으로 되돌리면 다시 넣는다' -ForegroundColor White
    # 사용자가 셸 형식으로 손수 고친 곁 훅(args 없음)은 -NoActivityHook 도 건드리지 않아야 한다.
    $tree = Read-GyeotTree $settings
    $handMade = 'C:\hand\gyeot-hook.exe claude-code'
    $hand = New-GyeotDict
    $hand['type'] = 'command'
    $hand['command'] = $handMade
    $tree['hooks']['SessionEnd'][0]['hooks'][0] = $hand
    Write-GyeotTextFile -Path $settings -Text ((ConvertTo-GyeotJson $tree) + "`n")
    Start-Sleep -Seconds 1  # 백업 이름(초 단위)이 겹치지 않게
    $code = Invoke-GyeotScript 'install.ps1' ($common + @('-ApplyClaudeHooks', '-NoActivityHook'))
    Assert-Gyeot ($code -eq 0) "-NoActivityHook 설치 종료 코드 0 (받은 값 $code)"
    $tree = Read-GyeotTree $settings
    $se = $tree['hooks']['SessionEnd']
    Assert-Gyeot ($se.Count -eq 1 -and $se[0]['hooks'].Count -eq 1 -and $se[0]['hooks'][0]['command'] -eq $handMade -and -not $se[0]['hooks'][0].Contains('args')) '-NoActivityHook: 손수 고친 곁 훅(SessionEnd)은 그대로'

    Assert-Gyeot ((Get-GyeotHookCount -Settings $tree -EventName 'PostToolBatch') -eq 0) '-NoActivityHook: 곁의 PostToolBatch 0개'
    Assert-Gyeot ($tree['hooks']['PostToolBatch'].Count -eq 1 -and $tree['hooks']['PostToolBatch'][0]['hooks'][0]['command'] -eq 'batch-logger') '-NoActivityHook: 사용자의 PostToolBatch 훅은 그대로'
    Assert-Gyeot ((Get-GyeotHookCount -Settings $tree -EventName 'Stop') -eq 1) 'Stop은 여전히 1개'
    $snip = Read-GyeotTree (Join-GyeotTestPath $root 'snippets\claude-settings.hooks.json')
    Assert-Gyeot (-not $snip['hooks'].Contains('PostToolBatch')) '-NoActivityHook: 출력 조각에도 PostToolBatch가 없다'
    $afterOptOut = Get-GyeotFileHash $settings
    $code = Invoke-GyeotScript 'install.ps1' ($common + @('-ApplyClaudeHooks', '-NoActivityHook'))
    Assert-Gyeot ($code -eq 0 -and (Get-GyeotFileHash $settings) -eq $afterOptOut) '-NoActivityHook 되풀이도 멱등'
    Start-Sleep -Seconds 1
    $code = Invoke-GyeotScript 'install.ps1' ($common + @('-ApplyClaudeHooks'))
    $tree = Read-GyeotTree $settings
    Assert-Gyeot ($code -eq 0 -and (Get-GyeotHookCount -Settings $tree -EventName 'PostToolBatch') -eq 1) '기본으로 다시 설치하면 PostToolBatch 1개'
    Assert-Gyeot ($tree['hooks']['PostToolBatch'].Count -eq 2) 'PostToolBatch: 사용자 묶음 + 곁 묶음'

    Write-Host '== 5. 옛 경로의 곁 훅은 새 경로로 고친다' -ForegroundColor White
    $tree['hooks']['Stop'][1]['hooks'][0]['command'] = 'C:\old\place\gyeot-hook.exe'
    Write-GyeotTextFile -Path $settings -Text ((ConvertTo-GyeotJson $tree) + "`n")
    Start-Sleep -Seconds 1
    $code = Invoke-GyeotScript 'install.ps1' ($common + @('-ApplyClaudeHooks'))
    $tree = Read-GyeotTree $settings
    Assert-Gyeot ($code -eq 0 -and $tree['hooks']['Stop'][1]['hooks'][0]['command'] -eq $hookExe) 'Stop 곁 훅 경로를 새 경로로 고쳤다'
    Assert-Gyeot ((Get-GyeotHookCount -Settings $tree -EventName 'Stop') -eq 1) '고친 뒤에도 1개'

    Write-Host '== 6. 지우기(데이터는 남김, 곁 훅만 뺌)' -ForegroundColor White
    $code = Invoke-GyeotScript 'uninstall.ps1' @('-InstallRoot', $root, '-ClaudeSettingsPath', $settings, '-RemoveClaudeHooks')
    Assert-Gyeot ($code -eq 0) "지우기 종료 코드 0 (받은 값 $code)"
    Assert-Gyeot (-not (Test-Path -LiteralPath (Join-Path $root 'bin'))) 'bin\ 지움'
    Assert-Gyeot (-not (Test-Path -LiteralPath (Join-Path $root 'scripts'))) 'scripts\ 지움'
    if (-not $SkipInit) { Assert-Gyeot (Test-Path -LiteralPath (Join-Path $root 'token')) '데이터(token)는 남는다' }
    $tree = Read-GyeotTree $settings
    $left = 0
    foreach ($k in @($tree['hooks'].Keys)) { $left += (Get-GyeotHookCount -Settings $tree -EventName $k) }
    Assert-Gyeot ($left -eq 0) '곁 훅이 하나도 남지 않았다'
    Assert-Gyeot ($tree['hooks']['Stop'].Count -eq 1 -and $tree['hooks']['Stop'][0]['hooks'][0]['command'] -eq 'fsmap-hook stop') '기존 Stop 훅은 남았다'
    Assert-Gyeot ($tree['hooks']['PreToolUse'][0]['hooks'][0]['command'] -eq 'aok-guard pre-bash') '기존 PreToolUse 훅은 남았다'
    Assert-Gyeot ($tree['hooks']['PostToolBatch'].Count -eq 1 -and $tree['hooks']['PostToolBatch'][0]['hooks'][0]['command'] -eq 'batch-logger') '기존 PostToolBatch 훅은 남고 곁 것만 빠졌다'
    # 원래 빈 배열이던 SessionStart 는 곁이 채웠다가 비우면서 키째 빠질 수 있다(누가 만든 키인지 알 수 없어서). 둘 다 허용.
    Assert-Gyeot ((-not $tree['hooks'].Contains('SessionStart')) -or $tree['hooks']['SessionStart'].Count -eq 0) 'SessionStart 에 곁 항목이 남지 않았다'
    Assert-Gyeot (-not $tree['hooks'].Contains('Notification')) '곁만 있던 이벤트(Notification)는 빠졌다'
    if ($WithStartup) {
        Assert-Gyeot ($null -eq (Get-ScheduledTask -TaskName $script:GyeotTaskName -ErrorAction SilentlyContinue)) '작업 스케줄러 등록을 지웠다'
    }

    Write-Host '== 7. -PurgeData 는 폴더 전체를 지운다' -ForegroundColor White
    $code = Invoke-GyeotScript 'uninstall.ps1' @('-InstallRoot', $root, '-ClaudeSettingsPath', $settings, '-PurgeData')
    Assert-Gyeot ($code -eq 0 -and -not (Test-Path -LiteralPath $root)) '설치 폴더 전체를 지웠다'

    Write-Host '== 8. %LOCALAPPDATA% 밖에 설치하면 상속을 끊고 그 사용자와 SYSTEM만 남긴다(§13.4, Windows)' -ForegroundColor White
    if (-not (Test-GyeotWindowsHost)) {
        Write-Host '  skip Windows가 아니라 폴더 권한(icacls·Get-Acl) 시험을 건너뜀' -ForegroundColor Yellow
    } else {
        $outside = $null
        foreach ($base in @("$env:SystemDrive\", $env:PUBLIC)) {
            if (-not $base -or $base -eq '\') { continue }
            $candidate = Join-Path $base ('gyeot-acl-test-' + [guid]::NewGuid().ToString('N'))
            if (Test-GyeotUnderLocalAppData $candidate) { continue }
            try {
                New-Item -ItemType Directory -Path $candidate -ErrorAction Stop | Out-Null
                $outside = $candidate
                break
            } catch {
                Write-Verbose "임시 폴더를 만들지 못함($candidate): $($_.Exception.Message)"
            }
        }
        if (-not $outside) {
            Write-Host '  skip %LOCALAPPDATA% 밖에 임시 폴더를 만들지 못해 건너뜀' -ForegroundColor Yellow
        } else {
            try {
                $aclRoot = Join-Path $outside 'root'
                $aclArgs = @('-SourceDir', $SourceDir, '-InstallRoot', $aclRoot, '-ClaudeSettingsPath', (Join-Path $outside 'settings.json'), '-NoInit')
                Assert-Gyeot (-not (Test-GyeotUnderLocalAppData $aclRoot)) "시험 폴더는 %LOCALAPPDATA% 밖: $aclRoot"
                $run = Invoke-GyeotScriptOutput 'install.ps1' ($aclArgs + @('-DryRun'))
                Assert-Gyeot ($run.Code -eq 0) "밖 폴더 DryRun 종료 코드 0 (받은 값 $($run.Code))"
                Assert-Gyeot ($run.Text -match 'icacls' -and $run.Text -match '/inheritance:r' -and $run.Text -match '\*S-1-5-18:\(OI\)\(CI\)F') 'DryRun은 할 icacls 명령을 보인다'
                Assert-Gyeot (-not (Test-Path -LiteralPath $aclRoot)) 'DryRun은 밖 폴더를 만들지도 권한을 바꾸지도 않는다'
                $run = Invoke-GyeotScriptOutput 'install.ps1' $aclArgs
                Assert-Gyeot ($run.Code -eq 0) "밖 폴더 설치 종료 코드 0 (받은 값 $($run.Code))"
                Assert-Gyeot ($run.Text -match 'S-1-5-18') '설치 출력이 권한 결과(SYSTEM SID 포함)를 보인다'
                $meSid = Get-GyeotCurrentUserSid
                $rootAcl = Get-GyeotAclSummary $aclRoot
                Write-Host "  …   $(Format-GyeotAclSummary $rootAcl)"
                Assert-Gyeot ($rootAcl.Protected) '설치 폴더: 상속이 끊겼다'
                $sids = @($rootAcl.Rules | ForEach-Object { $_.Sid } | Sort-Object -Unique)
                Assert-Gyeot ((($sids | Sort-Object) -join ',') -eq ((@($meSid, 'S-1-5-18') | Sort-Object) -join ',')) "설치 폴더: 허용 주체는 그 사용자와 SYSTEM뿐 ($($sids -join ','))"
                Assert-Gyeot (@(Get-GyeotAclProblem -Summary $rootAcl -UserSid $meSid).Count -eq 0) "설치 폴더: §13.4 문제 없음 ($((Get-GyeotAclProblem -Summary $rootAcl -UserSid $meSid) -join '; '))"
                foreach ($child in @('bin', 'bin\gyeot-hook.exe', 'bin\gyeotd.exe', 'bin\face\index.html', 'snippets\mcp.json')) {
                    $childPath = Join-Path $aclRoot $child
                    if (-not (Test-Path -LiteralPath $childPath)) { Assert-Gyeot $false "있어야 함: $child"; continue }
                    $childAcl = Get-GyeotAclSummary $childPath
                    $childProblems = @(Get-GyeotAclProblem -Summary $childAcl -UserSid $meSid -Child)
                    Assert-Gyeot ($childProblems.Count -eq 0) "$child`: 그 사용자·SYSTEM만 ($($childProblems -join '; '))"
                }
                # 다시 설치(이미 좁힌 폴더)도 그대로 통과한다.
                $code = Invoke-GyeotScript 'install.ps1' $aclArgs
                Assert-Gyeot ($code -eq 0 -and @(Get-GyeotAclProblem -Summary (Get-GyeotAclSummary $aclRoot) -UserSid $meSid).Count -eq 0) '다시 설치해도 그 사용자·SYSTEM만'
            } finally {
                Remove-Item -LiteralPath $outside -Recurse -Force -ErrorAction SilentlyContinue
            }
            Assert-Gyeot (-not (Test-Path -LiteralPath $outside)) '밖 시험 폴더를 지웠다'
        }
    }
} finally {
    Remove-Item -LiteralPath $tmp -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Host ''
if ($script:failures -gt 0) {
    Write-Host "실패 $($script:failures)건" -ForegroundColor Red
    exit 1
}
Write-Host '모두 통과' -ForegroundColor Green
exit 0
