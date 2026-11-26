<#
.SYNOPSIS
    GitHub 릴리스에서 곁(gyeot)을 받아 해시를 확인하고 설치(또는 업데이트)한 뒤 점검합니다.
.DESCRIPTION
    1) gh 로 릴리스의 gyeot-windows-x64.zip 과 SHA256SUMS.txt 를 받습니다(또는 -ZipPath 로 이미 받은 파일).
    2) SHA-256 이 맞지 않으면 멈춥니다.
    3) zip 을 풀고 install.ps1 -DryRun 으로 할 일을 보입니다.
    4) -Install 을 주었을 때만 실제로 설치하고, 원하면 channel 을 claude mcp(사용자 범위)에 등록하고, doctor.ps1 로 점검합니다.
    같은 명령을 다시 실행하면 업데이트입니다(install.ps1 이 실행 중인 곁을 멈추고 바꾼 뒤 다시 띄웁니다).
    이 파일 하나만 있으면 됩니다. 실행 정책은 이 프로세스에만 Bypass 로 주세요(시스템 설정은 바꾸지 않습니다).
.PARAMETER Install
    없으면 받기·해시 확인·미리보기까지만 합니다.
.PARAMETER Tag
    릴리스 태그(v0.2.0 등). 없으면 가장 최근 릴리스.
.PARAMETER ZipPath
    이미 받은 gyeot-windows-x64.zip. 같은 폴더의 SHA256SUMS.txt 로 확인합니다(-SumsPath 로 바꿀 수 있음).
.PARAMETER ApplyClaudeHooks
    ~/.claude/settings.json 을 백업한 뒤 곁의 훅을 더합니다(기존 훅은 그대로).
.PARAMETER RegisterStartup
    로그인할 때 곁 데몬을 창 없이 띄우도록 등록합니다.
.PARAMETER RegisterChannel
    claude mcp 사용자 범위에 gyeot channel 을 등록합니다(Node 22 이상과 claude CLI 필요).
.PARAMETER StartShell
    설치 뒤 곁 셸(트레이 아바타)을 띄웁니다.
.EXAMPLE
    powershell -NoProfile -ExecutionPolicy Bypass -File .\bootstrap.ps1 -ApplyClaudeHooks -RegisterStartup -RegisterChannel
.EXAMPLE
    powershell -NoProfile -ExecutionPolicy Bypass -File .\bootstrap.ps1 -ApplyClaudeHooks -RegisterStartup -RegisterChannel -StartShell -Install
#>
[Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSReviewUnusedParameter', '', Justification = '매개변수는 Invoke-Bootstrap 이 스크립트 범위에서 읽는다')]
[CmdletBinding()]
param(
    [string]$Repo = 'd-j-lee/gyeot',
    [string]$Tag,
    [string]$ZipPath,
    [string]$SumsPath,
    [string]$WorkDir,
    [switch]$Install,
    [switch]$ApplyClaudeHooks,
    [switch]$NoActivityHook,
    [switch]$RegisterStartup,
    [switch]$RegisterChannel,
    [switch]$StartShell,
    [string]$InstallRoot,
    [string]$ClaudeSettingsPath,
    [switch]$AllowNonWindows
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
$script:ZipName = 'gyeot-windows-x64.zip'
$script:SumsName = 'SHA256SUMS.txt'

function Write-BootStep([string]$m) { Write-Host "==> $m" -ForegroundColor Cyan }
function Write-BootInfo([string]$m) { Write-Host "    $m" }
function Write-BootOk([string]$m) { Write-Host "    ✓ $m" -ForegroundColor Green }
function Write-BootWarn([string]$m) { Write-Host "  ! $m" -ForegroundColor Yellow }

# SHA256SUMS.txt("<hex>  <이름>" 또는 "<hex> *<이름>")를 이름 → hex 로 읽는다.
function Read-BootSumFile([string]$Text) {
    $map = @{}
    foreach ($line in ($Text -split "`r?`n")) {
        if ($line -match '^\s*([0-9a-fA-F]{64})\s+\*?(.+?)\s*$') { $map[$Matches[2]] = $Matches[1].ToLowerInvariant() }
    }
    return $map
}

function Get-BootSha256([string]$Path) {
    $sha = [System.Security.Cryptography.SHA256]::Create()
    $fs = [System.IO.File]::OpenRead($Path)
    try { $h = $sha.ComputeHash($fs) } finally { $fs.Dispose(); $sha.Dispose() }
    return (($h | ForEach-Object { $_.ToString('x2') }) -join '')
}

# 파일 해시가 SHA256SUMS 의 값과 같아야 한다. 없거나 다르면 예외(설치하지 않는다).
function Assert-BootHash([string]$Path, [hashtable]$Sums) {
    $name = Split-Path -Leaf $Path
    if (-not $Sums.ContainsKey($name)) { throw "$($script:SumsName) 에 $name 이 없어요. 릴리스 파일이 맞는지 확인하세요." }
    $got = Get-BootSha256 $Path
    if ($got -ne $Sums[$name]) { throw "$name 의 SHA-256 이 맞지 않아요(받은 $got, 기대 $($Sums[$name])). 받다가 깨졌거나 바뀐 파일이라 설치하지 않아요." }
    return $got
}

# install.ps1 에 넘길 인자. 값은 따옴표로 묶는다(-File 로 부르므로 하나의 명령줄 문자열).
function Get-BootInstallArgument {
    param([bool]$DryRun, [bool]$Hooks, [bool]$NoActivity, [bool]$Startup, [string]$Root, [string]$Settings)
    $a = @()
    if ($DryRun) { $a += '-DryRun' }
    if ($Hooks) { $a += '-ApplyClaudeHooks' }
    if ($NoActivity) { $a += '-NoActivityHook' }
    if ($Startup) { $a += '-RegisterStartup' }
    if ($Root) { $a += '-InstallRoot'; $a += $Root }
    if ($Settings) { $a += '-ClaudeSettingsPath'; $a += $Settings }
    return , $a
}

function ConvertTo-BootArgLine([string[]]$Items) {
    return (($Items | ForEach-Object { if ($_ -match '[\s"]') { '"' + ($_ -replace '"', '\"') + '"' } else { $_ } }) -join ' ')
}

# 지금 쓰는 PowerShell 의 실행 파일(Windows PowerShell 5.1 이면 powershell.exe, 아니면 pwsh).
function Get-BootPowerShellExe {
    if ($PSVersionTable.PSEdition -eq 'Desktop') { return (Join-Path $PSHOME 'powershell.exe') }
    foreach ($name in @('pwsh.exe', 'pwsh')) {
        $candidate = Join-Path $PSHOME $name
        if (Test-Path -LiteralPath $candidate -PathType Leaf) { return $candidate }
    }
    return (Get-Command pwsh -ErrorAction Stop).Source
}

# 지금 쓰는 PowerShell 로 스크립트를 새 프로세스에서 돌린다(그 스크립트의 exit 가 이쪽을 끝내지 않게).
function Invoke-BootScript([string]$Script, [string[]]$Arguments) {
    $host_ = Get-BootPowerShellExe
    $all = @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', $Script) + $Arguments
    & $host_ @all | ForEach-Object { Write-Host $_ }
    return $LASTEXITCODE
}

function Invoke-BootNative([string]$Exe, [string[]]$Arguments, [int]$TimeoutMs = 120000) {
    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = $Exe
    $psi.Arguments = ConvertTo-BootArgLine $Arguments
    $psi.UseShellExecute = $false
    $psi.CreateNoWindow = $true
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError = $true
    $psi.StandardOutputEncoding = [System.Text.Encoding]::UTF8
    $psi.StandardErrorEncoding = [System.Text.Encoding]::UTF8
    $p = [System.Diagnostics.Process]::Start($psi)
    $out = $p.StandardOutput.ReadToEndAsync()
    $err = $p.StandardError.ReadToEndAsync()
    if (-not $p.WaitForExit($TimeoutMs)) {
        try { $p.Kill() } catch { Write-Verbose $_.Exception.Message }
        return [pscustomobject]@{ Code = -1; Out = ''; Err = "시간 한도($TimeoutMs ms)를 넘겼어요" }
    }
    $p.WaitForExit()
    return [pscustomobject]@{ Code = $p.ExitCode; Out = $out.Result; Err = $err.Result }
}

# claude CLI 는 claude.exe 일 수도 npm 의 claude.cmd 일 수도 있다. Windows 에서는 cmd 를 거쳐 부른다.
function Invoke-BootClaude([string[]]$Arguments) {
    if ($env:ComSpec) { return (Invoke-BootNative $env:ComSpec (@('/d', '/c', 'claude') + $Arguments)) }
    $c = Get-Command claude -ErrorAction Stop
    return (Invoke-BootNative $c.Source $Arguments)
}

# 그 포트에서 HTTP 200 을 주는 무언가가 있는지(내 데몬인지는 doctor.ps1 이 hello 로 본다).
function Test-BootDaemonUp([int]$Port) {
    try { [void](Get-GyeotLoopbackText -Port $Port -PathAndQuery '/health' -TimeoutMs 1000); return $true } catch { return $false }
}

function Test-BootWindows {
    return ([System.Environment]::OSVersion.Platform -eq [System.PlatformID]::Win32NT)
}

function Invoke-Bootstrap {
    if (-not (Test-BootWindows) -and -not $AllowNonWindows) { throw '곁은 Windows용이에요. 이 PC는 Windows가 아니에요.' }
    if ($PSVersionTable.PSVersion.Major -lt 5) { throw "PowerShell 5.1 이상이 필요해요(지금 $($PSVersionTable.PSVersion))." }

    $base = $WorkDir
    if (-not $base) {
        $tmp = $env:TEMP
        if (-not $tmp) { $tmp = [System.IO.Path]::GetTempPath() }
        $base = Join-Path $tmp 'gyeot-release'
    }

    # ── 1. 릴리스 파일 ──
    if ($ZipPath) {
        Write-BootStep "이미 받은 파일 쓰기: $ZipPath"
        $zip = (Resolve-Path -LiteralPath $ZipPath).Path
        if (-not $SumsPath) { $SumsPath = Join-Path (Split-Path -Parent $zip) $script:SumsName }
        if (-not (Test-Path -LiteralPath $SumsPath)) { throw "$($script:SumsName) 이 없어요($SumsPath). 릴리스 페이지에서 zip 과 함께 받으세요." }
        $sums = (Resolve-Path -LiteralPath $SumsPath).Path
        $label = 'local'
    } else {
        Write-BootStep "GitHub 릴리스 확인: $Repo"
        $gh = Get-Command gh -ErrorAction SilentlyContinue
        if (-not $gh) { throw 'gh(GitHub CLI)가 없어요. https://cli.github.com 에서 설치하고 gh auth login 하거나, 릴리스 페이지에서 zip·SHA256SUMS.txt 를 받아 -ZipPath 로 주세요.' }
        $st = Invoke-BootNative $gh.Source @('auth', 'status')
        if ($st.Code -ne 0) { throw "gh 에 로그인돼 있지 않아요. gh auth login 을 한 뒤 다시 하세요. ($($st.Err.Trim()))" }
        $viewArgs = @('release', 'view')
        if ($Tag) { $viewArgs += $Tag }
        $viewArgs += @('--repo', $Repo, '--json', 'tagName', '--jq', '.tagName')
        $v = Invoke-BootNative $gh.Source $viewArgs
        if ($v.Code -ne 0 -or -not $v.Out.Trim()) { throw "릴리스를 찾지 못했어요($Repo $Tag). 저장소 접근 권한과 태그를 확인하세요. ($($v.Err.Trim()))" }
        $label = $v.Out.Trim()
        $dir = Join-Path $base $label
        if (-not (Test-Path -LiteralPath $dir)) { New-Item -ItemType Directory -Path $dir -Force | Out-Null }
        Write-BootInfo "릴리스 $label → $dir"
        $d = Invoke-BootNative $gh.Source @('release', 'download', $label, '--repo', $Repo, '--pattern', $script:ZipName, '--pattern', $script:SumsName, '--dir', $dir, '--clobber') 600000
        if ($d.Code -ne 0) { throw "릴리스 파일을 받지 못했어요: $($d.Err.Trim())" }
        $zip = Join-Path $dir $script:ZipName
        $sums = Join-Path $dir $script:SumsName
    }

    # ── 2. 해시 ──
    Write-BootStep 'SHA-256 확인'
    $map = Read-BootSumFile ([System.IO.File]::ReadAllText($sums))
    $hash = Assert-BootHash $zip $map
    Write-BootOk "$($script:ZipName) $($hash.Substring(0, 16))… 일치"

    # ── 3. 풀기 ──
    $stage = Join-Path (Split-Path -Parent $zip) 'gyeot-windows-x64'
    Write-BootStep "풀기 → $stage"
    if (Test-Path -LiteralPath $stage) { Remove-Item -LiteralPath $stage -Recurse -Force }
    Expand-Archive -LiteralPath $zip -DestinationPath $stage -Force
    if (Test-BootWindows) { Get-ChildItem -LiteralPath $stage -Recurse -File | Unblock-File }
    else {
        # 시험용(-AllowNonWindows): zip 은 실행 권한을 담지 않는다.
        foreach ($x in @(Get-ChildItem -LiteralPath (Join-Path $stage 'bin') -Filter '*.exe' -File -ErrorAction SilentlyContinue)) { & chmod +x $x.FullName }
    }
    $installer = Join-Path $stage 'scripts\install.ps1'
    if (-not (Test-Path -LiteralPath $installer)) { $installer = Join-Path (Join-Path $stage 'scripts') 'install.ps1' }
    if (-not (Test-Path -LiteralPath $installer)) { throw 'zip 안에 scripts\install.ps1 이 없어요.' }
    $build = Join-Path $stage 'BUILD.txt'
    if (Test-Path -LiteralPath $build) { foreach ($l in (Get-Content -LiteralPath $build -Encoding UTF8)) { Write-BootInfo $l } }

    # ── 4. 미리보기(늘) ──
    Write-BootStep '미리보기(install.ps1 -DryRun)'
    $dry = Get-BootInstallArgument -DryRun $true -Hooks $ApplyClaudeHooks.IsPresent -NoActivity $NoActivityHook.IsPresent -Startup $RegisterStartup.IsPresent -Root $InstallRoot -Settings $ClaudeSettingsPath
    $code = Invoke-BootScript $installer $dry
    if ($code -ne 0) { throw "미리보기가 실패했어요(종료 코드 $code)." }
    if ($RegisterChannel) { Write-BootInfo '[미리보기] claude mcp add --transport stdio --scope user gyeot -- node "<설치 폴더>\bin\gyeot-channel.mjs"' }
    if (-not $Install) {
        Write-Host ''
        Write-Host '미리보기만 했어요. 위 내용으로 설치하려면 같은 명령에 -Install 을 붙이세요.' -ForegroundColor White
        return 0
    }

    # ── 5. 설치 ──
    Write-BootStep '설치(install.ps1)'
    $real = Get-BootInstallArgument -DryRun $false -Hooks $ApplyClaudeHooks.IsPresent -NoActivity $NoActivityHook.IsPresent -Startup $RegisterStartup.IsPresent -Root $InstallRoot -Settings $ClaudeSettingsPath
    $code = Invoke-BootScript $installer $real
    if ($code -ne 0) { throw "설치가 실패했어요(종료 코드 $code). 위 출력의 첫 오류를 보세요." }

    $root = $InstallRoot
    if (-not $root) { if ($env:GYEOT_HOME) { $root = $env:GYEOT_HOME } else { $root = Join-Path $env:LOCALAPPDATA 'gyeot' } }
    $bin = Join-Path $root 'bin'
    $channelJs = Join-Path $bin 'gyeot-channel.mjs'
    $common = Join-Path (Join-Path $root 'scripts') 'gyeot-common.ps1'
    if (-not (Test-Path -LiteralPath $common)) { throw "설치 폴더에 gyeot-common.ps1 이 없어요: $common" }
    . $common

    # ── 5b. 데몬이 떠 있게(셸·시작 등록과 상관없이 지금 바로 쓸 수 있게) ──
    $daemonExe = Join-Path $bin 'gyeotd.exe'
    $pc = Invoke-BootNative $daemonExe @('--print-config')
    $cfg = ConvertFrom-GyeotPrintConfig ($pc.Out + "`n" + $pc.Err)
    if (-not (Test-BootDaemonUp $cfg.Port)) {
        Write-BootStep "곁 데몬 띄우기(127.0.0.1:$($cfg.Port))"
        if (Test-BootWindows) { Start-Process -FilePath $daemonExe -WorkingDirectory $bin -WindowStyle Hidden }
        else {
            Start-Process -FilePath $daemonExe -WorkingDirectory $bin `
                -RedirectStandardOutput (Join-Path $root 'gyeotd.stdout.log') -RedirectStandardError (Join-Path $root 'gyeotd.stderr.log')
        }
        $deadline = (Get-Date).AddSeconds(15)
        while (-not (Test-BootDaemonUp $cfg.Port) -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 300 }
        if (Test-BootDaemonUp $cfg.Port) { Write-BootOk '떴어요.' } else { Write-BootWarn '15초 안에 뜨지 않았어요. 점검 결과를 보세요.' }
    }

    # ── 6. channel 등록(선택) ──
    if ($RegisterChannel) {
        Write-BootStep 'channel 등록(claude mcp, 사용자 범위)'
        $node = Get-Command node -ErrorAction SilentlyContinue
        $claude = Get-Command claude -ErrorAction SilentlyContinue
        $nodeMajor = 0
        if ($node) { $nv = Invoke-BootNative $node.Source @('--version'); if ($nv.Out -match '^\s*v?(\d+)\.') { $nodeMajor = [int]$Matches[1] } }
        if (-not (Test-Path -LiteralPath $channelJs)) { Write-BootWarn 'gyeot-channel.mjs 가 없어 건너뛰어요.' }
        elseif ($nodeMajor -lt 22) { Write-BootWarn 'Node 22 이상이 없어 건너뛰어요(대화 기능만 빠지고 나머지는 돌아요).' }
        elseif (-not $claude) { Write-BootWarn 'claude CLI 를 찾지 못해 건너뛰어요.' }
        else {
            $g = Invoke-BootClaude @('mcp', 'get', 'gyeot')
            $already = ($g.Code -eq 0 -and $g.Out -match [regex]::Escape($channelJs))
            if ($already) { Write-BootOk '이미 같은 경로로 등록돼 있어요.' }
            else {
                if ($g.Code -eq 0) {
                    Write-BootInfo '다른 경로로 등록된 gyeot 를 사용자 범위에서 지우고 다시 등록해요.'
                    [void](Invoke-BootClaude @('mcp', 'remove', 'gyeot', '--scope', 'user'))
                }
                $a = Invoke-BootClaude @('mcp', 'add', '--transport', 'stdio', '--scope', 'user', 'gyeot', '--', 'node', $channelJs)
                if ($a.Code -eq 0) { Write-BootOk "등록했어요: node `"$channelJs`"" }
                else { Write-BootWarn "등록하지 못했어요: $($a.Err.Trim()) $($a.Out.Trim())" }
            }
        }
    }

    # ── 7. 셸 띄우기(선택) ──
    $shellExe = Join-Path $bin 'gyeot-shell.exe'
    if ($StartShell -and (Test-Path -LiteralPath $shellExe) -and (Test-BootWindows)) {
        $running = @(Get-Process -Name 'gyeot-shell' -ErrorAction SilentlyContinue)
        if ($running.Count -eq 0) { Start-Process -FilePath $shellExe -WorkingDirectory $bin; Write-BootOk '곁 셸을 띄웠어요(트레이·Ctrl+Alt+Space).'; Start-Sleep -Seconds 3 }
        else { Write-BootOk '곁 셸이 이미 떠 있어요.' }
    }

    # ── 8. 점검 ──
    Write-BootStep '점검(doctor.ps1)'
    $doctor = Join-Path (Join-Path $root 'scripts') 'doctor.ps1'
    if (-not (Test-Path -LiteralPath $doctor)) { throw "설치 폴더에 doctor.ps1 이 없어요: $doctor" }
    $dArgs = @('-InstallRoot', $root)
    if ($ClaudeSettingsPath) { $dArgs += @('-ClaudeSettingsPath', $ClaudeSettingsPath) }
    if ($NoActivityHook) { $dArgs += '-NoActivityHook' }
    if (-not $ApplyClaudeHooks) { $dArgs += '-NoHooks' }
    $code = Invoke-BootScript $doctor $dArgs
    Write-Host ''
    if ($code -ne 0) { Write-Host '설치는 했지만 점검에서 실패가 있어요. 위 ✗ 줄을 보세요.' -ForegroundColor Red; return 1 }
    Write-Host "곁 $label 설치·점검을 마쳤어요." -ForegroundColor White
    Write-BootInfo "얼굴 열기: 트레이의 '곁 열기', 또는 `"$(Join-Path $bin 'gyeotd.exe')`" --open"
    Write-BootInfo '대화하려면 Claude Code 세션을 이렇게 시작: claude --dangerously-load-development-channels server:gyeot'
    Write-BootInfo "지우기: powershell -NoProfile -ExecutionPolicy Bypass -File `"$(Join-Path (Join-Path $root 'scripts') 'uninstall.ps1')`""
    return 0
}

# 점 소싱(시험)일 때는 함수만 읽는다.
if ($MyInvocation.InvocationName -ne '.') {
    $restoreEncoding = $null
    if ([Console]::IsOutputRedirected) {
        $restoreEncoding = [Console]::OutputEncoding
        [Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false)
    }
    try {
        $exitCode = Invoke-Bootstrap
    } catch {
        Write-Host ''
        Write-Host "곁을 설치하지 못했어요: $($_.Exception.Message)" -ForegroundColor Red
        $exitCode = 1
    } finally {
        if ($null -ne $restoreEncoding) { [Console]::OutputEncoding = $restoreEncoding }
    }
    exit $exitCode
}
