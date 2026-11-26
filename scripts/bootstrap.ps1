<#
.SYNOPSIS
    GitHub 릴리스에서 곁(gyeot)을 받아 확인하고 설치(또는 업데이트)한 뒤 점검합니다.
.DESCRIPTION
    1) 릴리스의 gyeot-windows-x64.zip 과 SHA256SUMS.txt 를 받습니다. gh 가 github.com 에 로그인돼 있으면 gh 로,
       아니면 HTTPS(공개 저장소)로 받습니다. -ZipPath 로 이미 받은 파일을 줄 수도 있습니다.
    2) SHA-256(받다가 깨지지 않았는지)이 맞지 않으면 멈춥니다. gh 로 받았으면 출처 증명(GitHub Actions 가
       이 저장소에서 만든 파일인지, gh attestation verify)도 확인하고, 맞지 않으면 멈춥니다.
    3) zip 을 %TEMP% 아래에 풀고 install.ps1 -DryRun 으로 할 일을 보입니다.
    4) -Install 을 주었을 때만 실제로 설치하고, 원하면 channel 을 claude mcp(사용자 범위)에 등록하고, doctor.ps1 로 점검합니다.
    같은 명령을 다시 실행하면 업데이트입니다(install.ps1 이 실행 중인 곁을 멈추고 바꾼 뒤 다시 띄웁니다).
    이 파일 하나만 있으면 됩니다. 실행 정책은 이 프로세스에만 Bypass 로 주세요(시스템 설정은 바꾸지 않습니다).
.PARAMETER Install
    없으면 받기·해시 확인·미리보기까지만 합니다.
.PARAMETER Tag
    릴리스 태그(v0.2.0 등). 없으면 가장 최근 릴리스.
.PARAMETER ZipPath
    이미 받은 gyeot-windows-x64.zip. 같은 폴더의 SHA256SUMS.txt 로 확인합니다(-SumsPath 로 바꿀 수 있음).
    출처 증명은 확인하지 않으니, 필요하면 gh attestation verify <zip> --repo d-j-lee/gyeot 로 따로 보세요.
.PARAMETER SkipAttestation
    출처 증명 확인을 건너뜁니다. 확인이 실패한 이유(회사망 차단 등)를 안 뒤에만 쓰세요.
.NOTES
    받은 zip 안의 bootstrap.ps1 이 이 파일과 다르면(설치된 옛 bootstrap 으로 업데이트할 때) 확인을 마친 zip 을
    넘겨 새 판의 bootstrap 이 이어서 하게 합니다(-HandoffVia). -ReleaseBaseUrl 은 시험용입니다.
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
    [switch]$SkipAttestation,
    [string]$InstallRoot,
    [string]$ClaudeSettingsPath,
    [switch]$AllowNonWindows,
    [string]$ReleaseBaseUrl,
    [string]$HandoffVia,
    [string]$HandoffLabel,
    [switch]$HandoffAttested
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
$script:ZipName = 'gyeot-windows-x64.zip'
$script:SumsName = 'SHA256SUMS.txt'
$script:BootSelf = $PSCommandPath

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
# 자식은 출력이 파이프라 UTF-8 로 쓴다. 이쪽도 그동안 UTF-8 로 읽어야 한글이 깨지지 않는다.
function Invoke-BootScript([string]$Script, [string[]]$Arguments) {
    $host_ = Get-BootPowerShellExe
    $all = @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', $Script) + $Arguments
    $prev = $null
    try { $prev = [Console]::OutputEncoding; [Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false) } catch { $prev = $null }
    try {
        & $host_ @all | ForEach-Object { Write-Host $_ }
        $code = $LASTEXITCODE
    } finally {
        if ($null -ne $prev) { try { [Console]::OutputEncoding = $prev } catch { Write-Verbose $_.Exception.Message } }
    }
    return $code
}

function Invoke-BootNative([string]$Exe, [string[]]$Arguments, [int]$TimeoutMs = 120000, [hashtable]$Env) {
    $psi = New-Object System.Diagnostics.ProcessStartInfo
    if ($Env) { foreach ($k in $Env.Keys) { $psi.EnvironmentVariables[$k] = [string]$Env[$k] } }
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

# gh 는 늘 github.com 을 보게 한다(회사 GitHub Enterprise 에 로그인돼 있어도 섞이지 않게).
function Invoke-BootGh([string]$Gh, [string[]]$Arguments, [int]$TimeoutMs = 120000) {
    return (Invoke-BootNative $Gh $Arguments $TimeoutMs @{ GH_HOST = 'github.com' })
}

# claude CLI 는 claude.exe 일 수도 npm 의 claude.cmd 일 수도 있다. Windows 에서는 cmd 를 거쳐 부른다.
function Invoke-BootClaude([string[]]$Arguments) {
    if ($env:ComSpec) { return (Invoke-BootNative $env:ComSpec (@('/d', '/c', 'claude') + $Arguments)) }
    $c = Get-Command claude -ErrorAction Stop
    return (Invoke-BootNative $c.Source $Arguments)
}

# 그 포트에서 HTTP 200 을 주는 무언가가 있는지(내 데몬인지는 doctor.ps1 이 hello 로 본다).
function Test-BootDaemonUp([int]$Port) {
    try { return ((Get-GyeotLoopbackText -Port $Port -PathAndQuery '/health' -TimeoutMs 1000).Status -eq 200) } catch { return $false }
}

# 저장소·태그는 URL 과 명령줄에 들어가므로 꼴을 좁힌다.
# \z: 끝 줄바꿈도 받지 않는다. -cmatch: 대소문자를 가린다(태그는 소문자 v). CI 의 태그 검사와 같은 꼴이다.
function Test-BootRepoName([string]$Name) { return ($Name -cmatch '\A[A-Za-z0-9-]+/[A-Za-z0-9_-][A-Za-z0-9._-]*\z' -and $Name -notmatch '\.\.') }
function Test-BootTagName([string]$Name) { return ($Name -cmatch '\Av\d+\.\d+\.\d+(-rc\.\d+)?\z') }

function Get-BootReleaseUrl([string]$Repo, [string]$Tag, [string]$Asset) {
    if ($Tag) { return "https://github.com/$Repo/releases/download/$Tag/$Asset" }
    return "https://github.com/$Repo/releases/latest/download/$Asset"
}

# HTTPS 로 한 파일을 받는다. 5.1 은 TLS 1.2 를 켜야 하고, 진행 막대를 끄면 몇 배 빠르다.
# 인증 프록시(NTLM 등)를 위해 기본 자격 증명을 쓰게 한다(5.1).
function Invoke-BootDownload([string]$Url, [string]$OutFile) {
    $ProgressPreference = 'SilentlyContinue'
    if ($PSVersionTable.PSEdition -eq 'Desktop') {
        [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
        try { [System.Net.WebRequest]::DefaultWebProxy.Credentials = [System.Net.CredentialCache]::DefaultNetworkCredentials } catch { Write-Verbose $_.Exception.Message }
    }
    Invoke-WebRequest -Uri $Url -OutFile $OutFile -UseBasicParsing -Headers @{ 'User-Agent' = 'gyeot-bootstrap' }
}

# 두 스크립트가 줄 끝을 빼고 같은지.
function Test-BootSameScript([string]$A, [string]$B) {
    $ta = ([System.IO.File]::ReadAllText($A)) -replace "`r`n", "`n"
    $tb = ([System.IO.File]::ReadAllText($B)) -replace "`r`n", "`n"
    return ($ta -ceq $tb)
}

# BUILD.txt 의 "version: x.y.z" 줄.
function Get-BootBuildVersion([string]$BuildText) {
    if ($BuildText -match '(?m)^version:\s*(\S+)') { return $Matches[1] }
    return $null
}

# 곁 데몬을 띄운다. -PreferTask 이고 작업 스케줄러에 곁 데몬 작업이 있으면 그것으로 띄운다
# (부른 도구의 프로세스 트리와 떨어져 돈다). 돌려주는 값: 띄운 방법.
function Start-BootDaemon([string]$DaemonExe, [string]$Root, [switch]$PreferTask) {
    $bin = Split-Path -Parent $DaemonExe
    if (Test-BootWindows) {
        if ($PreferTask) {
            $task = $null
            try { $task = Get-ScheduledTask -TaskName 'gyeot-daemon' -ErrorAction Stop } catch { $task = $null }
            if ($task) {
                try { Start-ScheduledTask -TaskName 'gyeot-daemon' -ErrorAction Stop; return '작업 스케줄러' } catch { Write-Verbose $_.Exception.Message }
            }
        }
        Start-Process -FilePath $DaemonExe -WorkingDirectory $bin -WindowStyle Hidden
        return '숨긴 창'
    }
    Start-Process -FilePath $DaemonExe -WorkingDirectory $bin `
        -RedirectStandardOutput (Join-Path $Root 'gyeotd.stdout.log') -RedirectStandardError (Join-Path $Root 'gyeotd.stderr.log')
    return '시험용 프로세스'
}

function Wait-BootDaemon([int]$Port, [int]$Seconds) {
    $deadline = (Get-Date).AddSeconds($Seconds)
    while ((Get-Date) -lt $deadline) {
        if (Test-BootDaemonUp $Port) { return $true }
        Start-Sleep -Milliseconds 300
    }
    return (Test-BootDaemonUp $Port)
}

function Test-BootWindows {
    return ([System.Environment]::OSVersion.Platform -eq [System.PlatformID]::Win32NT)
}

function Invoke-Bootstrap {
    if (-not (Test-BootWindows) -and -not $AllowNonWindows) { throw '곁은 Windows용이에요. 이 PC는 Windows가 아니에요.' }
    if ($PSVersionTable.PSVersion.Major -lt 5) { throw "PowerShell 5.1 이상이 필요해요(지금 $($PSVersionTable.PSVersion))." }
    if (-not (Test-BootRepoName $Repo)) { throw "저장소 이름 꼴이 아니에요: $Repo" }
    if ($Tag -and -not (Test-BootTagName $Tag)) { throw "태그 꼴이 아니에요(v0.2.0, v0.2.0-rc.1 처럼): $Tag" }
    if ($ReleaseBaseUrl -and $ReleaseBaseUrl -notmatch '\Ahttps?://') { throw "-ReleaseBaseUrl 은 http(s) 주소여야 해요: $ReleaseBaseUrl" }

    $base = $WorkDir
    if (-not $base) {
        $tmp = $env:TEMP
        if (-not $tmp) { $tmp = [System.IO.Path]::GetTempPath() }
        $base = Join-Path $tmp 'gyeot-release'
    }

    # ── 1. 릴리스 파일 ──
    $gh = Get-Command gh -ErrorAction SilentlyContinue
    $ghReady = $false
    $via = ''
    if ($ZipPath) {
        Write-BootStep "이미 받은 파일 쓰기: $ZipPath"
        $zip = (Resolve-Path -LiteralPath $ZipPath).Path
        if (-not $SumsPath) { $SumsPath = Join-Path (Split-Path -Parent $zip) $script:SumsName }
        if (-not (Test-Path -LiteralPath $SumsPath)) { throw "$($script:SumsName) 이 없어요($SumsPath). 릴리스 페이지에서 zip 과 함께 받으세요." }
        $sums = (Resolve-Path -LiteralPath $SumsPath).Path
        $label = 'local'
        $dir = Join-Path $base 'local'
        $via = 'local'
    } else {
        Write-BootStep "GitHub 릴리스 받기: $Repo $(if ($Tag) { $Tag } else { '(가장 최근)' })"
        if ($gh) {
            $st = Invoke-BootGh $gh.Source @('auth', 'status', '--hostname', 'github.com')
            $ghReady = ($st.Code -eq 0)
        }
        $label = $Tag
        if ($ghReady) {
            try {
                $viewArgs = @('release', 'view')
                if ($Tag) { $viewArgs += $Tag }
                $viewArgs += @('--repo', $Repo, '--json', 'tagName', '--jq', '.tagName')
                $v = Invoke-BootGh $gh.Source $viewArgs
                if ($v.Code -ne 0 -or -not $v.Out.Trim()) { throw "릴리스를 찾지 못했어요($($v.Err.Trim()))" }
                $label = $v.Out.Trim()
                if (-not (Test-BootTagName $label)) { throw "릴리스 태그 꼴이 아니에요: $label" }
                $dir = Join-Path $base $label
                if (-not (Test-Path -LiteralPath $dir)) { New-Item -ItemType Directory -Path $dir -Force | Out-Null }
                $d = Invoke-BootGh $gh.Source @('release', 'download', $label, '--repo', $Repo, '--pattern', $script:ZipName, '--pattern', $script:SumsName, '--dir', $dir, '--clobber') 600000
                if ($d.Code -ne 0) { throw "릴리스 파일을 받지 못했어요($($d.Err.Trim()))" }
                $via = 'gh'
            } catch {
                Write-BootWarn "gh 로 받지 못했어요: $($_.Exception.Message) — HTTPS 로 다시 해 볼게요."
                $label = $Tag
            }
        }
        if (-not $via) {
            $dir = Join-Path $base $(if ($Tag) { $Tag } else { 'latest' })
            if (-not (Test-Path -LiteralPath $dir)) { New-Item -ItemType Directory -Path $dir -Force | Out-Null }
            foreach ($asset in @($script:SumsName, $script:ZipName)) {
                $url = Get-BootReleaseUrl $Repo $Tag $asset
                if ($ReleaseBaseUrl) { $url = "$($ReleaseBaseUrl.TrimEnd('/'))/$asset" }
                Write-BootInfo "받기: $url"
                try { Invoke-BootDownload $url (Join-Path $dir $asset) }
                catch { throw "릴리스 파일을 받지 못했어요($url): $($_.Exception.Message). 회사망이 github.com 을 막으면 gh auth login 뒤 다시 하거나, 릴리스 페이지에서 받아 -ZipPath 로 주세요." }
            }
            $via = 'https'
        }
        $zip = Join-Path $dir $script:ZipName
        $sums = Join-Path $dir $script:SumsName
        Write-BootInfo "받은 곳: $dir ($via)"
    }

    # ── 2. 확인: 무결성(SHA-256), 출처(gh 로 받았을 때) ──
    Write-BootStep '받은 파일 확인'
    $map = Read-BootSumFile ([System.IO.File]::ReadAllText($sums))
    $hash = Assert-BootHash $zip $map
    Write-BootOk "SHA-256 일치(받다가 깨지지 않았어요): $($hash.Substring(0, 16))…"
    $attested = $false
    if ($via -eq 'local' -and $HandoffVia) {
        Write-BootInfo "받기는 앞선 bootstrap 이 했어요($HandoffVia). SHA-256 은 여기서 다시 봤어요."
        if ($HandoffAttested) { Write-BootOk '출처 증명은 앞선 bootstrap 이 확인했어요.'; $attested = $true }
        else { Write-BootInfo '출처 증명은 확인하지 않았어요(앞선 bootstrap 도 하지 않았어요).' }
    } elseif ($via -eq 'local') {
        Write-BootInfo "출처 증명은 확인하지 않았어요(-ZipPath). 필요하면: gh attestation verify `"$zip`" --repo $Repo"
    } elseif ($SkipAttestation) {
        Write-BootWarn '출처 증명 확인을 건너뛰었어요(-SkipAttestation).'
    } elseif (-not $ghReady) {
        Write-BootInfo '출처 증명(GitHub Actions 가 이 저장소에서 만든 파일인지)은 gh 가 github.com 에 로그인돼 있을 때만 확인해요. 이번엔 SHA-256 만 봤어요.'
    } else {
        $help = Invoke-BootGh $gh.Source @('attestation', 'verify', '--help')
        if ($help.Code -ne 0) {
            Write-BootInfo 'gh 가 오래돼 출처 증명을 확인하지 못했어요(gh 2.49 이상 필요). 이번엔 SHA-256 만 봤어요.'
        } else {
            # 이 저장소의 ci.yml 이, 이 태그에서, GitHub 호스트 러너로 만든 것만 받는다(gh 가 그 옵션을 알 때).
            $avArgs = @('attestation', 'verify', $zip, '--repo', $Repo)
            $helpText = $help.Out + "`n" + $help.Err
            if ($helpText -match '--signer-workflow') { $avArgs += @('--signer-workflow', "$Repo/.github/workflows/ci.yml") }
            if ($label -and (Test-BootTagName $label) -and $helpText -match '--source-ref') { $avArgs += @('--source-ref', "refs/tags/$label") }
            if ($helpText -match '--deny-self-hosted-runners') { $avArgs += '--deny-self-hosted-runners' }
            $av = Invoke-BootGh $gh.Source $avArgs 180000
            if ($av.Code -ne 0) {
                throw "출처 증명을 확인하지 못했어요: $((($av.Err + ' ' + $av.Out).Trim() -split "`r?`n")[0]). 파일이 바뀌었거나 회사망이 확인 주소(GitHub·sigstore)를 막았을 수 있어요. 이유를 확인한 뒤에만 -SkipAttestation 을 붙이세요."
            }
            Write-BootOk "출처 증명 일치: $Repo 의 GitHub Actions 가 만든 파일이에요."
            $attested = $true
        }
    }

    # ── 3. 풀기(%TEMP% 아래, 받은 폴더와 같은 곳) ──
    $stage = Join-Path $dir 'gyeot-windows-x64'
    Write-BootStep "풀기 → $stage"
    if (-not (Test-Path -LiteralPath $dir)) { New-Item -ItemType Directory -Path $dir -Force | Out-Null }
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
    if (Test-Path -LiteralPath $build) {
        $buildText = [System.IO.File]::ReadAllText($build)
        foreach ($l in ($buildText -split "`r?`n")) { if ($l.Trim()) { Write-BootInfo $l } }
        $ver = Get-BootBuildVersion $buildText
        if ($ver) {
            if (-not $label) { $label = "v$ver" }
            elseif ($label -eq 'local') { $label = "v$ver(받은 파일)" }
        }
    }
    if ($HandoffLabel) { $label = $HandoffLabel }
    if (-not $label) { $label = '(판 모름)' }

    # ── 3b. 받은 판의 bootstrap 이 이 파일과 다르면 그쪽으로 넘긴다 ──
    # 설치된 옛 bootstrap 으로 업데이트할 때, 옛 코드가 새 판의 스크립트와 섞여 돌지 않게 한다.
    # 넘기는 zip 은 여기서 SHA-256(과 출처 증명)을 마친 것이다.
    # 줄 끝(CRLF·LF)만 다른 것은 같은 판으로 본다.
    $newBoot = Join-Path (Join-Path $stage 'scripts') 'bootstrap.ps1'
    if (-not $HandoffVia -and $script:BootSelf -and (Test-Path -LiteralPath $newBoot) -and (Test-Path -LiteralPath $script:BootSelf) -and
        -not (Test-BootSameScript $newBoot $script:BootSelf)) {
        Write-BootStep '받은 판의 bootstrap 으로 넘겨요(이 파일과 내용이 달라요)'
        $fw = @('-Repo', $Repo, '-ZipPath', $zip, '-SumsPath', $sums, '-WorkDir', (Join-Path $base 'handoff'), '-HandoffVia', $via, '-HandoffLabel', $label)
        if ($attested) { $fw += '-HandoffAttested' }
        foreach ($sw in @('Install', 'ApplyClaudeHooks', 'NoActivityHook', 'RegisterStartup', 'RegisterChannel', 'StartShell', 'AllowNonWindows')) {
            if ((Get-Variable -Name $sw -ValueOnly)) { $fw += "-$sw" }
        }
        if ($InstallRoot) { $fw += @('-InstallRoot', $InstallRoot) }
        if ($ClaudeSettingsPath) { $fw += @('-ClaudeSettingsPath', $ClaudeSettingsPath) }
        return (Invoke-BootScript $newBoot $fw)
    }

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
        $how = Start-BootDaemon $daemonExe $root -PreferTask
        $up = Wait-BootDaemon $cfg.Port 15
        if (-not $up -and $how -eq '작업 스케줄러') {
            # 로그인 세션이 없는 환경 등에서는 작업이 바로 돌지 않는다. 그때는 직접 띄운다.
            Write-BootInfo '작업 스케줄러로 뜨지 않아 직접 띄워요.'
            $how = Start-BootDaemon $daemonExe $root
            $up = Wait-BootDaemon $cfg.Port 15
        }
        if ($up) { Write-BootOk "떴어요($how)." } else { Write-BootWarn '15초 안에 뜨지 않았어요. 점검 결과를 보세요.' }
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
                $addArgs = @('mcp', 'add', '--transport', 'stdio', '--scope', 'user')
                # 데이터 폴더가 기본(%LOCALAPPDATA%\gyeot)이 아니면 channel 도 같은 곳을 보게 GYEOT_HOME 을 고정한다.
                $defaultHome = $null
                if ($env:LOCALAPPDATA) { $defaultHome = Join-Path $env:LOCALAPPDATA 'gyeot' }
                if ($cfg.Home -and -not (Test-GyeotSamePath $cfg.Home $defaultHome)) { $addArgs += @('-e', "GYEOT_HOME=$($cfg.Home)") }
                $addArgs += @('gyeot', '--', 'node', $channelJs)
                $a = Invoke-BootClaude $addArgs
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
    # 훅을 이번에 넣지 않았어도 점검은 한다. 곁 훅이 없으면 doctor 가 경고만 하고, 옛 경로를 부르는 곁 훅은 실패로 알린다.
    $code = Invoke-BootScript $doctor $dArgs
    Write-Host ''
    if ($code -ne 0) { Write-Host '설치는 했지만 점검에서 실패가 있어요. 위 ✗ 줄을 보세요.' -ForegroundColor Red; return 1 }
    Write-Host "곁 $label 설치·점검을 마쳤어요." -ForegroundColor White
    Write-BootInfo "얼굴 열기: 트레이의 '곁 열기', 또는 `"$(Join-Path $bin 'gyeotd.exe')`" --open"
    Write-BootInfo '대화하려면 Claude Code 세션을 이렇게 시작: claude --dangerously-load-development-channels server:gyeot'
    Write-BootInfo "점검만 다시: powershell -NoProfile -ExecutionPolicy Bypass -File `"$doctor`""
    Write-BootInfo "지우기: powershell -NoProfile -ExecutionPolicy Bypass -File `"$(Join-Path (Join-Path $root 'scripts') 'uninstall.ps1')`" -RemoveClaudeHooks"
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
