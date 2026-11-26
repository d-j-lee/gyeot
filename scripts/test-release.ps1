<#
.SYNOPSIS
    올린 GitHub 릴리스로 설치 흐름을 시험합니다(CI release-check 전용).
.DESCRIPTION
    사람이 받는 길 그대로 릴리스를 받아 이 PC에 곁을 설치·업데이트·점검·지우기까지 합니다.
    이 PC의 %LOCALAPPDATA%\gyeot, ~/.claude/settings.json, 작업 스케줄러를 바꾸므로 CI 러너에서만 돌립니다.
    -Step 을 단계마다 따로 불러, 단계마다 다른 PowerShell(5.1·7)로 돌릴 수 있게 했습니다.
.PARAMETER Step
    fetch(받기·무결성) → preview → install → update → doctor → uninstall
.EXAMPLE
    .\scripts\test-release.ps1 -Tag v0.2.0-rc.1 -Step fetch
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$Tag,
    [Parameter(Mandatory = $true)][ValidateSet('fetch', 'preview', 'install', 'update', 'doctor', 'uninstall')][string]$Step,
    [string]$Repo = 'd-j-lee/gyeot',
    [switch]$ExpectAttestation
)

$ErrorActionPreference = 'Stop'
if (-not $env:CI) { throw '이 시험은 이 PC의 곁 설치를 바꿉니다. CI 러너에서만 돌리세요($env:CI 가 없어요).' }
$work = Join-Path $(if ($env:RUNNER_TEMP) { $env:RUNNER_TEMP } else { $env:TEMP }) 'gyeot-boot'
$boot = Join-Path $work 'bootstrap.ps1'
$installed = Join-Path $env:LOCALAPPDATA 'gyeot'

function Write-ReleaseFail([string]$Message) { Write-Host "FAIL $Message" -ForegroundColor Red; exit 1 }
function Write-ReleasePass([string]$Message) { Write-Host "ok   $Message" -ForegroundColor Green }

# bootstrap 은 Write-Host 로 쓴다. 같은 프로세스에서 불러 정보 스트림까지 받는다.
function Invoke-ReleaseBoot([string]$Path, [string[]]$Arguments) {
    $out = & $Path @Arguments 6>&1 | ForEach-Object { Write-Host $_; $_ }
    return [pscustomobject]@{ Code = $LASTEXITCODE; Text = ($out | Out-String) }
}

function Assert-Text($Result, [string[]]$Want) {
    foreach ($w in $Want) {
        if ($Result.Text -notmatch [regex]::Escape($w)) { Write-ReleaseFail "출력에 '$w' 가 없어요" }
        Write-ReleasePass "출력에 '$w'"
    }
}

Write-Host "== release-check $Step ($Tag, PowerShell $($PSVersionTable.PSVersion))" -ForegroundColor White
switch ($Step) {
    'fetch' {
        New-Item -ItemType Directory -Force -Path $work | Out-Null
        if ($PSVersionTable.PSEdition -eq 'Desktop') {
            [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
        }
        $ProgressPreference = 'SilentlyContinue'
        $base = "https://github.com/$Repo/releases/download/$Tag"
        foreach ($f in @('bootstrap.ps1', 'SHA256SUMS.txt', 'INSTALL-WITH-CLAUDE.md')) {
            Invoke-WebRequest -UseBasicParsing -Uri "$base/$f" -OutFile (Join-Path $work $f)
        }
        $sums = Get-Content -LiteralPath (Join-Path $work 'SHA256SUMS.txt')
        foreach ($f in @('bootstrap.ps1', 'INSTALL-WITH-CLAUDE.md')) {
            $line = @($sums | Where-Object { $_ -match "^[0-9a-f]{64}  $([regex]::Escape($f))$" })
            if ($line.Count -ne 1) { Write-ReleaseFail "SHA256SUMS 에 $f 줄이 한 개가 아니에요($($line.Count))" }
            $want = ($line[0] -split '\s+')[0]
            $got = (Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $work $f)).Hash.ToLowerInvariant()
            if ($got -ne $want) { Write-ReleaseFail "$f 해시가 맞지 않아요($got / $want)" }
            Write-ReleasePass "$f SHA-256 일치"
        }
        $doc = [System.IO.File]::ReadAllText((Join-Path $work 'INSTALL-WITH-CLAUDE.md'))
        if ($doc -notmatch 'releases/latest/download/INSTALL-WITH-CLAUDE\.md') { Write-ReleaseFail '받은 설치 문서에 한 줄 프롬프트가 없어요' }
        Write-ReleasePass '받은 설치 문서에 한 줄 프롬프트가 있다'
    }
    'preview' {
        $r = Invoke-ReleaseBoot $boot @('-Tag', $Tag, '-ApplyClaudeHooks', '-RegisterStartup')
        if ($r.Code -ne 0) { Write-ReleaseFail "미리보기 종료 코드 $($r.Code)" }
        Assert-Text $r @('SHA-256 일치', "version: $($Tag.TrimStart('v') -replace '-rc\.\d+$', '')", '미리보기만 했어요')
        if (Test-Path -LiteralPath (Join-Path $installed 'bin')) { Write-ReleaseFail '미리보기가 설치했어요' }
        Write-ReleasePass '미리보기는 설치하지 않았다'
    }
    'install' {
        $r = Invoke-ReleaseBoot $boot @('-Tag', $Tag, '-ApplyClaudeHooks', '-RegisterStartup', '-Install')
        if ($r.Code -ne 0) { Write-ReleaseFail "설치 종료 코드 $($r.Code)" }
        $want = @('SHA-256 일치', '점검 통과', '내 곁 데몬', '설치·점검을 마쳤어요')
        if ($ExpectAttestation) { $want += '출처 증명 일치' }
        Assert-Text $r $want
    }
    'update' {
        $r = Invoke-ReleaseBoot (Join-Path $installed 'scripts\bootstrap.ps1') @('-Tag', $Tag, '-ApplyClaudeHooks', '-RegisterStartup', '-Install')
        if ($r.Code -ne 0) { Write-ReleaseFail "업데이트 종료 코드 $($r.Code)" }
        $want = @('SHA-256 일치', '점검 통과', '내 곁 데몬')
        if ($ExpectAttestation) { $want += '출처 증명 일치' }
        Assert-Text $r $want
        $s = Get-Content -Raw -LiteralPath (Join-Path $env:USERPROFILE '.claude\settings.json') | ConvertFrom-Json
        $n = @($s.hooks.Stop | ForEach-Object { $_.hooks } | Where-Object { $_.command -match 'gyeot-hook' }).Count
        if ($n -ne 1) { Write-ReleaseFail "업데이트 뒤 Stop 의 곁 훅이 $n 개예요(1이어야)" }
        Write-ReleasePass '업데이트 뒤에도 곁 훅은 한 벌'
    }
    'doctor' {
        & (Join-Path $installed 'scripts\doctor.ps1')
        if ($LASTEXITCODE -ne 0) { Write-ReleaseFail "doctor 종료 코드 $LASTEXITCODE" }
        Write-ReleasePass 'doctor 통과'
    }
    'uninstall' {
        & (Join-Path $installed 'scripts\uninstall.ps1') -RemoveClaudeHooks
        if ($LASTEXITCODE -ne 0) { Write-ReleaseFail "지우기 종료 코드 $LASTEXITCODE" }
        if (Get-ScheduledTask -TaskName 'gyeot-daemon' -ErrorAction SilentlyContinue) { Write-ReleaseFail '작업 스케줄러 등록이 남았어요' }
        if (Test-Path -LiteralPath (Join-Path $installed 'bin')) { Write-ReleaseFail 'bin 이 남았어요' }
        $s = Get-Content -Raw -LiteralPath (Join-Path $env:USERPROFILE '.claude\settings.json')
        if ($s -match 'gyeot-hook') { Write-ReleaseFail '곁 훅이 남았어요' }
        Write-ReleasePass '지우기: bin·작업·곁 훅이 없다'
    }
}
exit 0
