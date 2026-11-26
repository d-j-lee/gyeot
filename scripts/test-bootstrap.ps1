<#
.SYNOPSIS
    bootstrap.ps1·doctor.ps1 자체 시험. 바꾸는 것은 임시 폴더뿐입니다.
.DESCRIPTION
    늘: 해시 읽기·확인, install.ps1 인자, hello 증명(계약 §13.1 공유 값), print-config 읽기, Node 판 읽기.
    -ZipPath(진짜 배포 zip) 또는 -GyeotdPath·-HookPath(빌드한 실행 파일)를 주면 끝-끝도 돕니다:
    미리보기는 아무것도 설치하지 않음 → 바꾼 zip은 거절 → -Install 로 설치·데몬 띄우기·doctor 통과 →
    다시 실행(업데이트)해도 통과 → 지우기에서 사용자 훅은 남음.
.EXAMPLE
    pwsh ./scripts/test-bootstrap.ps1
.EXAMPLE
    pwsh ./scripts/test-bootstrap.ps1 -ZipPath dist/gyeot-windows-x64.zip
.EXAMPLE
    pwsh ./scripts/test-bootstrap.ps1 -GyeotdPath target/debug/gyeotd -HookPath target/debug/gyeot-hook -ChannelPath channel/dist/gyeot-channel.mjs -FaceDir face/dist
#>
[CmdletBinding()]
param(
    [string]$ZipPath,
    [string]$GyeotdPath,
    [string]$HookPath,
    [string]$ChannelPath,
    [string]$FaceDir,
    [switch]$KeepTemp
)

$ErrorActionPreference = 'Stop'
# bootstrap.ps1 을 점 소싱하면 그 매개변수(-ZipPath 등)가 이 범위에 다시 묶이므로 먼저 옮겨 둔다.
$TestZip = $ZipPath; $TestDaemon = $GyeotdPath; $TestHook = $HookPath; $TestChannel = $ChannelPath; $TestFace = $FaceDir; $TestKeep = [bool]$KeepTemp
. (Join-Path $PSScriptRoot 'gyeot-common.ps1')
. (Join-Path $PSScriptRoot 'bootstrap.ps1')

if ([Console]::IsOutputRedirected) { [Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false) }

$script:fails = 0
$script:passes = 0
function Assert-Boot([bool]$Condition, [string]$Message) {
    if ($Condition) { $script:passes++; Write-Host "  ok   $Message" -ForegroundColor Green }
    else { $script:fails++; Write-Host "  FAIL $Message" -ForegroundColor Red }
}

function Invoke-BootTest([string[]]$Arguments) {
    $exe = Get-BootPowerShellExe
    $all = @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', (Join-Path $PSScriptRoot 'bootstrap.ps1')) + $Arguments
    # 표준 오류는 넘겨받지 않는다(Windows PowerShell 5.1 은 Stop 아래에서 넘겨받은 stderr 를 종료 오류로 바꾼다).
    $text = (& $exe @all | Out-String)
    return [pscustomobject]@{ Code = $LASTEXITCODE; Text = $text }
}

function Stop-BootTestDaemon([string]$Root) {
    foreach ($p in @(Get-Process -ErrorAction SilentlyContinue)) {
        $path = $null
        try { $path = $p.Path } catch { $path = $null }
        if ($path -and (Test-GyeotPathUnder $path $Root)) {
            try { Stop-Process -Id $p.Id -Force -ErrorAction Stop } catch { Write-Verbose $_.Exception.Message }
        }
    }
    Start-Sleep -Milliseconds 500
}

# 127.0.0.1:<Port> 에서 연결마다 정해진 응답을 주는 가짜 서버(별도 프로세스 작업). 시간이 지나면 스스로 끝난다.
function Start-BootFakeServer([int]$Port, [string]$Response, [int]$Seconds = 30) {
    $server = {
        param($ListenPort, $Reply, $Lifetime)
        $l = New-Object System.Net.Sockets.TcpListener ([System.Net.IPAddress]::Loopback, $ListenPort)
        $l.Start()
        $deadline = (Get-Date).AddSeconds($Lifetime)
        try {
            while ((Get-Date) -lt $deadline) {
                if (-not $l.Pending()) { Start-Sleep -Milliseconds 50; continue }
                $c = $l.AcceptTcpClient()
                try {
                    # 준비 확인용 연결은 아무것도 보내지 않고 닫는다. 그때는 답하지 않는다.
                    $c.ReceiveTimeout = 5000
                    $st = $c.GetStream()
                    $buf = New-Object byte[] 4096
                    $n = $st.Read($buf, 0, $buf.Length)
                    if ($n -gt 0) {
                        $bytes = [System.Text.Encoding]::UTF8.GetBytes($Reply)
                        $st.Write($bytes, 0, $bytes.Length)
                        $st.Flush()
                    }
                } catch {
                    Write-Verbose $_.Exception.Message
                } finally { $c.Close() }
            }
        } finally { $l.Stop() }
    }
    $job = Start-Job -ScriptBlock $server -ArgumentList @($Port, $Response, $Seconds)
    # 듣기 시작할 때까지 기다린다.
    $until = (Get-Date).AddSeconds(15)
    while ((Get-Date) -lt $until) {
        $probe = New-Object System.Net.Sockets.TcpClient
        try { $probe.Connect('127.0.0.1', $Port); break } catch { Start-Sleep -Milliseconds 200 } finally { $probe.Close() }
    }
    return $job
}

function Stop-BootFakeServer($Job) {
    if ($Job) { Stop-Job $Job -ErrorAction SilentlyContinue; Remove-Job $Job -Force -ErrorAction SilentlyContinue }
}

function Get-BootFreePort {
    $l = New-Object System.Net.Sockets.TcpListener ([System.Net.IPAddress]::Loopback, 0)
    $l.Start(); $port = $l.LocalEndpoint.Port; $l.Stop()
    return $port
}

Write-Host '== 1. 해시·인자·증명(OS와 상관없는 함수)' -ForegroundColor White
$h1 = 'a' * 64; $h2 = 'B' * 64
$sums = Read-BootSumFile "$h1  gyeot-windows-x64.zip`r`n$h2 *bootstrap.ps1`njunk line`n"
Assert-Boot ($sums['gyeot-windows-x64.zip'] -eq $h1) 'SHA256SUMS: "<hex>  <이름>"'
Assert-Boot ($sums['bootstrap.ps1'] -eq ('b' * 64)) 'SHA256SUMS: "<hex> *<이름>", 소문자로'
Assert-Boot ($sums.Count -eq 2) 'SHA256SUMS: 꼴이 아닌 줄은 버린다'

$tmpRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("gyeot-boot-test-" + [guid]::NewGuid().ToString('N').Substring(0, 8))
New-Item -ItemType Directory -Path $tmpRoot -Force | Out-Null
try {
    $f = Join-Path $tmpRoot 'gyeot-windows-x64.zip'
    [System.IO.File]::WriteAllText($f, 'hello')
    $good = @{ 'gyeot-windows-x64.zip' = '2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824' }
    Assert-Boot ((Assert-BootHash $f $good) -eq $good['gyeot-windows-x64.zip']) '해시가 맞으면 통과'
    $threw = $null
    try { [void](Assert-BootHash $f @{ 'gyeot-windows-x64.zip' = ('0' * 64) }) } catch { $threw = $_.Exception.Message }
    Assert-Boot ($threw -match '맞지 않아요') '해시가 다르면 멈춘다'
    $threw = $null
    try { [void](Assert-BootHash $f @{}) } catch { $threw = $_.Exception.Message }
    Assert-Boot ($threw -match '없어요') 'SHA256SUMS 에 이름이 없으면 멈춘다'

    $a = Get-BootInstallArgument -DryRun $true -Hooks $true -NoActivity $false -Startup $true -Root 'C:\a b' -Settings ''
    Assert-Boot (($a -join '|') -eq '-DryRun|-ApplyClaudeHooks|-RegisterStartup|-InstallRoot|C:\a b') 'install.ps1 인자: 미리보기·훅·시작 등록·폴더'
    $a = Get-BootInstallArgument -DryRun $false -Hooks $false -NoActivity $true -Startup $false -Root '' -Settings 'x.json'
    Assert-Boot (($a -join '|') -eq '-NoActivityHook|-ClaudeSettingsPath|x.json') 'install.ps1 인자: 설치(미리보기 없음)·-NoActivityHook·설정 경로'
    Assert-Boot ((ConvertTo-BootArgLine @('a', 'b c', 'd"e')) -eq 'a "b c" "d\"e"') '명령줄 인자 따옴표'

    $proof = Get-GyeotHelloProof ('0123456789abcdef' * 4) 47321 ('ab' * 32)
    Assert-Boot ($proof -eq 'cb2a3490aa613ccf14c93af5162080aff69a29f105ef62e3ef4777d7f85bd4c5') 'hello 증명 = 서버·셸·channel 공유 값(포트 47321)'
    Assert-Boot ((Get-GyeotHelloProof ('0123456789abcdef' * 4) 47322 ('ab' * 32)) -eq 'c909b0dde49a2587904b41e63cd46b3faa9674ea124aa03d002b4b1235a683a1') 'hello 증명 = 공유 값(포트 47322)'
    Assert-Boot (Test-GyeotSameHex $proof $proof.ToUpperInvariant()) '증명 비교는 대소문자를 가리지 않는다'
    Assert-Boot (-not (Test-GyeotSameHex $proof ('0' * 64))) '다른 증명은 거절'
    Assert-Boot (-not (Test-GyeotSameHex $null $proof)) '증명이 없으면 거절'
    Assert-Boot ((New-GyeotNonce) -match '^[0-9a-f]{64}$') 'n은 32바이트 hex'

    $pc = ConvertFrom-GyeotPrintConfig "# 데이터 폴더: C:\Users\me\AppData\Local\gyeot`n# 설정 파일: C:\x\config.toml`n# 토큰: C:\x\token`n# 얼굴 폴더: C:\x\bin\face`nport = 47999`nstale_minutes = 30"
    Assert-Boot ($pc.Port -eq 47999 -and $pc.Token -eq 'C:\x\token' -and $pc.Home -eq 'C:\Users\me\AppData\Local\gyeot' -and $pc.Face -eq 'C:\x\bin\face') 'gyeotd --print-config 읽기'
    Assert-Boot ((ConvertFrom-GyeotPrintConfig '').Port -eq 47321) 'print-config 에 port 가 없으면 47321'
    Assert-Boot ((Get-GyeotNodeMajor 'v22.11.0') -eq 22 -and (Get-GyeotNodeMajor 'v20.1.0') -eq 20 -and (Get-GyeotNodeMajor 'nope') -eq 0) 'node --version 읽기'

    Assert-Boot ((Test-BootRepoName 'd-j-lee/gyeot') -and -not (Test-BootRepoName 'a/b;c') -and -not (Test-BootRepoName '../x') -and -not (Test-BootRepoName 'x')) '저장소 이름 꼴'
    Assert-Boot ((Test-BootTagName 'v0.2.0') -and (Test-BootTagName 'v0.2.0-rc.1') -and -not (Test-BootTagName '0.2.0') -and -not (Test-BootTagName 'v0.2.0/../x') -and -not (Test-BootTagName 'v1.2.3 x')) '태그 꼴'
    Assert-Boot ((Get-BootReleaseUrl 'd-j-lee/gyeot' '' 'a.zip') -eq 'https://github.com/d-j-lee/gyeot/releases/latest/download/a.zip') '받기 주소: 가장 최근'
    Assert-Boot ((Get-BootReleaseUrl 'd-j-lee/gyeot' 'v0.2.0' 'a.zip') -eq 'https://github.com/d-j-lee/gyeot/releases/download/v0.2.0/a.zip') '받기 주소: 태그'
    Assert-Boot ((Get-BootBuildVersion "곁(gyeot)`r`nversion: 0.2.0`r`nref: x") -eq '0.2.0' -and $null -eq (Get-BootBuildVersion 'commit: x')) 'BUILD.txt 판 읽기'

    Assert-Boot ((Test-GyeotSamePath 'C:\A\gyeot-hook.exe' 'c:\a\GYEOT-HOOK.EXE') -and -not (Test-GyeotSamePath 'C:\a\x.exe' 'C:\b\x.exe') -and -not (Test-GyeotSamePath '' 'x')) '경로 비교(대소문자 무시, 해석 못 하는 경로도 예외 없이)'
    $hx = New-GyeotDict; $hx['type'] = 'command'; $hx['command'] = 'C:\g\gyeot-hook.exe'; $hx['args'] = (New-GyeotList @('claude-code'))
    $hq = New-GyeotDict; $hq['type'] = 'command'; $hq['command'] = '"C:\Program Files\g\gyeot-hook.exe" claude-code'
    $hu = New-GyeotDict; $hu['type'] = 'command'; $hu['command'] = 'C:\g2\gyeot-hook claude-code'
    Assert-Boot ((Get-GyeotHookHandlerExe $hx) -eq 'C:\g\gyeot-hook.exe' -and (Get-GyeotHookHandlerExe $hq) -eq 'C:\Program Files\g\gyeot-hook.exe' -and (Get-GyeotHookHandlerExe $hu) -eq 'C:\g2\gyeot-hook') '곁 훅 처리기가 부르는 실행 파일(exec·셸 형식)'

    # 셸 형식 곁 훅이 다른 경로를 부르면: 그 항목은 두고, 이 설치의 항목을 더하고, Stale 로 센다.
    $st = ConvertFrom-GyeotJsonText '{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"C:\\old\\gyeot-hook.exe claude-code"}]}]}}'
    $mr = Merge-GyeotClaudeHook -Settings $st -HookExe 'C:\new\gyeot-hook.exe'
    $stop = $st['hooks']['Stop']
    Assert-Boot ($mr.Stale -eq 1 -and $mr.StalePaths -contains 'C:\old\gyeot-hook.exe' -and $stop.Count -eq 2 -and $stop[0]['hooks'][0]['command'] -eq 'C:\old\gyeot-hook.exe claude-code' -and $stop[1]['hooks'][0]['command'] -eq 'C:\new\gyeot-hook.exe') '다른 경로의 셸 형식 곁 훅: 두고, 새로 더하고, 알린다'
    $mr2 = Merge-GyeotClaudeHook -Settings $st -HookExe 'C:\new\gyeot-hook.exe'
    Assert-Boot ($mr2.Added -eq 0 -and $st['hooks']['Stop'].Count -eq 2) '두 번째 병합은 더하지 않는다(멱등)'
    $sm = ConvertFrom-GyeotJsonText '{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"\"C:\\new\\gyeot-hook.exe\" claude-code"}]}]}}'
    $mr3 = Merge-GyeotClaudeHook -Settings $sm -HookExe 'C:\new\gyeot-hook.exe'
    Assert-Boot ($mr3.Stale -eq 0 -and $sm['hooks']['Stop'].Count -eq 1) '이 설치를 부르는 셸 형식 곁 훅은 "있음"(더하지 않음)'

    # 127.0.0.1 HTTP: 상태 코드를 돌려주고, 닿지 않으면 예외. HTTPS 받기 함수는 같은 가짜 서버로(http).
    $fp = Get-BootFreePort
    $threw = $false
    try { [void](Get-GyeotLoopbackText -Port $fp -PathAndQuery '/health' -TimeoutMs 800) } catch { $threw = $true }
    Assert-Boot $threw '아무것도 듣지 않는 포트는 예외(닿지 않음)'
    $job = Start-BootFakeServer $fp "HTTP/1.1 404 Not Found`r`nContent-Length: 2`r`nConnection: close`r`n`r`nno"
    try {
        $lr = Get-GyeotLoopbackText -Port $fp -PathAndQuery '/v1/hello?n=00' -TimeoutMs 3000
        Assert-Boot ($lr.Status -eq 404 -and $lr.Body -eq 'no') '다른 서버의 404 는 상태 404 로 돌려준다(예외 아님)'
        Assert-Boot (-not (Test-BootDaemonUp $fp)) '404 를 주는 포트는 "데몬 떠 있음"이 아니다'
    } finally { Stop-BootFakeServer $job }
    $fp = Get-BootFreePort
    $job = Start-BootFakeServer $fp "HTTP/1.1 200 OK`r`nTransfer-Encoding: chunked`r`nConnection: close`r`n`r`n5`r`nhello`r`n6`r`n world`r`n0`r`n`r`n"
    try {
        $lr = Get-GyeotLoopbackText -Port $fp -PathAndQuery '/health' -TimeoutMs 3000
        Assert-Boot ($lr.Status -eq 200 -and $lr.Body -eq 'hello world') '청크 응답을 이어 붙인다'
        $dl = Join-Path $tmpRoot 'dl.bin'
        Invoke-BootDownload "http://127.0.0.1:$fp/x" $dl
        Assert-Boot ((Test-Path -LiteralPath $dl) -and ([System.IO.File]::ReadAllText($dl) -eq 'hello world')) '받기 함수(Invoke-WebRequest)가 파일을 쓴다'
    } finally { Stop-BootFakeServer $job }
    $fp = Get-BootFreePort
    $job = Start-BootFakeServer $fp 'SSH-2.0-OpenSSH_9.6'
    try {
        $lr = Get-GyeotLoopbackText -Port $fp -PathAndQuery '/' -TimeoutMs 3000
        Assert-Boot ($lr.Status -eq 0) 'HTTP 가 아닌 응답은 상태 0'
    } finally { Stop-BootFakeServer $job }

    # ── 끝-끝 ──
    $e2e = ($TestZip -or ($TestDaemon -and $TestHook))
    if (-not $e2e) {
        Write-Host '  skip 끝-끝(-ZipPath 또는 -GyeotdPath·-HookPath 가 없음)' -ForegroundColor Yellow
    } else {
        Write-Host '== 2. 릴리스 꼴 zip 준비' -ForegroundColor White
        $rel = Join-Path $tmpRoot 'release'
        New-Item -ItemType Directory -Path $rel -Force | Out-Null
        $zip = Join-Path $rel 'gyeot-windows-x64.zip'
        if ($TestZip) {
            Copy-Item -LiteralPath $TestZip -Destination $zip
        } else {
            $stage = Join-Path $tmpRoot 'stage'
            New-Item -ItemType Directory -Path (Join-Path $stage 'bin'), (Join-Path $stage 'scripts'), (Join-Path $stage 'face') -Force | Out-Null
            Copy-Item -LiteralPath $TestDaemon -Destination (Join-Path $stage 'bin/gyeotd.exe')
            Copy-Item -LiteralPath $TestHook -Destination (Join-Path $stage 'bin/gyeot-hook.exe')
            if ($TestChannel) { Copy-Item -LiteralPath $TestChannel -Destination (Join-Path $stage 'bin/gyeot-channel.mjs') }
            if ($TestFace) { Copy-Item -Path (Join-Path $TestFace '*') -Destination (Join-Path $stage 'face') -Recurse }
            else { Set-Content -LiteralPath (Join-Path $stage 'face/index.html') -Value '<!doctype html>' }
            foreach ($s in @('install.ps1', 'uninstall.ps1', 'gyeot-common.ps1', 'doctor.ps1', 'bootstrap.ps1')) {
                Copy-Item -LiteralPath (Join-Path $PSScriptRoot $s) -Destination (Join-Path $stage "scripts/$s")
            }
            Copy-Item -Path (Join-Path (Split-Path -Parent $PSScriptRoot) 'examples') -Destination (Join-Path $stage 'examples') -Recurse
            Set-Content -LiteralPath (Join-Path $stage 'BUILD.txt') -Value "곁(gyeot) 시험용`ncommit: test" -Encoding UTF8
            Compress-Archive -Path (Join-Path $stage '*') -DestinationPath $zip -Force
        }
        "$(Get-BootSha256 $zip)  gyeot-windows-x64.zip" | Set-Content -LiteralPath (Join-Path $rel 'SHA256SUMS.txt') -Encoding ASCII

        $root = Join-Path $tmpRoot 'root'
        New-Item -ItemType Directory -Path $root -Force | Out-Null
        $port = Get-BootFreePort
        Set-Content -LiteralPath (Join-Path $root 'config.toml') -Value "port = $port" -Encoding ASCII
        $claudeDir = Join-Path $tmpRoot 'claude'
        New-Item -ItemType Directory -Path $claudeDir -Force | Out-Null
        $settings = Join-Path $claudeDir 'settings.json'
        Set-Content -LiteralPath $settings -Encoding UTF8 -Value '{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"my-own-stop-hook"}]}]},"theme":"dark"}'
        $env:GYEOT_HOME = $root
        $common = @('-ZipPath', $zip, '-InstallRoot', $root, '-ClaudeSettingsPath', $settings, '-WorkDir', (Join-Path $tmpRoot 'work'))
        if (-not (Test-BootWindows)) { $common += '-AllowNonWindows' }

        Write-Host '== 3. 미리보기는 설치하지 않는다' -ForegroundColor White
        $r = Invoke-BootTest ($common + @('-ApplyClaudeHooks'))
        Assert-Boot ($r.Code -eq 0) "미리보기 종료 코드 0 (받은 값 $($r.Code))"
        Assert-Boot ($r.Text -match '일치') '미리보기도 해시를 확인한다'
        Assert-Boot ($r.Text -match '미리보기만 했어요') '미리보기만 했다고 알린다'
        Assert-Boot (-not (Test-Path -LiteralPath (Join-Path $root 'bin'))) '미리보기 뒤 bin\ 이 없다'
        Assert-Boot ((Get-Content -LiteralPath $settings -Raw) -notmatch 'gyeot-hook') '미리보기 뒤 Claude 설정이 그대로'

        Write-Host '== 4. 바뀐 zip 은 거절한다' -ForegroundColor White
        $bad = Join-Path $tmpRoot 'bad'
        New-Item -ItemType Directory -Path $bad -Force | Out-Null
        $badZip = Join-Path $bad 'gyeot-windows-x64.zip'
        Copy-Item -LiteralPath $zip -Destination $badZip
        $fs = [System.IO.File]::Open($badZip, 'Append'); $fs.WriteByte(0); $fs.Dispose()
        Copy-Item -LiteralPath (Join-Path $rel 'SHA256SUMS.txt') -Destination (Join-Path $bad 'SHA256SUMS.txt')
        $badArgs = @($common); $badArgs[1] = $badZip
        $r = Invoke-BootTest ($badArgs + @('-Install', '-ApplyClaudeHooks'))
        Assert-Boot ($r.Code -eq 1) "바뀐 zip: 종료 코드 1 (받은 값 $($r.Code))"
        Assert-Boot ($r.Text -match '맞지 않아요') '바뀐 zip: 해시가 맞지 않는다고 알린다'
        Assert-Boot (-not (Test-Path -LiteralPath (Join-Path $root 'bin'))) '바뀐 zip: 아무것도 설치하지 않는다'

        Write-Host '== 5. 설치·데몬·점검' -ForegroundColor White
        $r = Invoke-BootTest ($common + @('-Install', '-ApplyClaudeHooks'))
        if ($r.Code -ne 0) { Write-Host $r.Text }
        Assert-Boot ($r.Code -eq 0) "설치 종료 코드 0 (받은 값 $($r.Code))"
        Assert-Boot ($r.Text -match '점검 통과') 'doctor 통과'
        Assert-Boot ($r.Text -match '내 곁 데몬') 'doctor 가 hello 증명으로 내 데몬을 확인했다'
        Assert-Boot ($r.Text -match '설치·점검을 마쳤어요') '끝 안내'
        $tree = ConvertFrom-GyeotJsonText (Read-GyeotTextFile $settings)
        Assert-Boot ((Get-GyeotHookCount $tree 'PostToolBatch') -eq 1) '곁 훅이 들어갔다(PostToolBatch 포함)'
        Assert-Boot ($tree['theme'] -eq 'dark' -and ([string]$tree['hooks']['Stop'][0]['hooks'][0]['command']) -eq 'my-own-stop-hook') '사용자 설정·훅은 그대로'
        foreach ($s in @('doctor.ps1', 'bootstrap.ps1', 'uninstall.ps1', 'gyeot-common.ps1')) {
            Assert-Boot (Test-Path -LiteralPath (Join-Path (Join-Path $root 'scripts') $s)) "설치 폴더에 scripts\$s"
        }

        Write-Host '== 6. 다시 실행(업데이트)해도 통과' -ForegroundColor White
        $r = Invoke-BootTest ($common + @('-Install', '-ApplyClaudeHooks'))
        if ($r.Code -ne 0) { Write-Host $r.Text }
        Assert-Boot ($r.Code -eq 0 -and $r.Text -match '점검 통과') "업데이트 종료 코드 0·점검 통과 (받은 값 $($r.Code))"
        $tree = ConvertFrom-GyeotJsonText (Read-GyeotTextFile $settings)
        Assert-Boot ((Get-GyeotHookCount $tree 'Stop') -eq 1) '다시 실행해도 곁 훅은 한 벌'

        Write-Host '== 7. 설치 폴더의 doctor 를 따로 돌려도 통과, 훅·데몬이 어긋나면 알린다' -ForegroundColor White
        $exe = Get-BootPowerShellExe
        $doc = Join-Path (Join-Path $root 'scripts') 'doctor.ps1'
        $t = (& $exe -NoProfile -ExecutionPolicy Bypass -File $doc -InstallRoot $root -ClaudeSettingsPath $settings | Out-String)
        Assert-Boot ($LASTEXITCODE -eq 0) "doctor 단독 종료 코드 0 (받은 값 $LASTEXITCODE)"

        # 훅 없이 설치한 모양: 경고만(실패 아님).
        $noHook = Join-Path $claudeDir 'no-hooks.json'
        Set-Content -LiteralPath $noHook -Encoding UTF8 -Value '{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"my-own-stop-hook"}]}]}}'
        $t = (& $exe -NoProfile -ExecutionPolicy Bypass -File $doc -InstallRoot $root -ClaudeSettingsPath $noHook | Out-String)
        Assert-Boot ($LASTEXITCODE -eq 0 -and $t -match '곁 훅이 없어요') "곁 훅이 없으면 경고만(받은 값 $LASTEXITCODE)"

        # 모든 곁 훅이 없는 파일(옛 설치)을 부르면 실패.
        $staleSet = Join-Path $claudeDir 'stale.json'
        $st = ConvertFrom-GyeotJsonText (Read-GyeotTextFile $settings)
        foreach ($k in @($st['hooks'].Keys)) { foreach ($g in @($st['hooks'][$k])) { foreach ($h in @($g['hooks'])) { if (Test-GyeotHookHandler $h) { $h['command'] = (Join-Path $tmpRoot 'gone\gyeot-hook.exe') } } } }
        Write-GyeotTextFile -Path $staleSet -Text ((ConvertTo-GyeotJson $st) + "`n")
        $t = (& $exe -NoProfile -ExecutionPolicy Bypass -File $doc -InstallRoot $root -ClaudeSettingsPath $staleSet | Out-String)
        Assert-Boot ($LASTEXITCODE -eq 1 -and $t -match '없는 파일을 부르는 곁 훅' -and $t -match '이 설치의 gyeot-hook 를 부르지 않는 이벤트') "옛 경로만 가리키는 곁 훅은 실패(받은 값 $LASTEXITCODE)"

        # 이 설치의 훅은 다 있어도, 없는 파일을 부르는 곁 훅이 하나 더 있으면 실패(그 훅은 매번 실패한다).
        $extraSet = Join-Path $claudeDir 'extra-broken.json'
        $ex = ConvertFrom-GyeotJsonText (Read-GyeotTextFile $settings)
        $hb = New-GyeotDict; $hb['type'] = 'command'; $hb['command'] = '"' + (Join-Path $tmpRoot 'gone2\gyeot-hook.exe') + '" claude-code'
        $gb = New-GyeotDict; $gb['hooks'] = (New-GyeotList @(, $hb))
        [void]$ex['hooks']['Stop'].Add($gb)
        Write-GyeotTextFile -Path $extraSet -Text ((ConvertTo-GyeotJson $ex) + "`n")
        $t = (& $exe -NoProfile -ExecutionPolicy Bypass -File $doc -InstallRoot $root -ClaudeSettingsPath $extraSet | Out-String)
        Assert-Boot ($LASTEXITCODE -eq 1 -and $t -match '없는 파일을 부르는 곁 훅' -and $t -notmatch '부르지 않는 이벤트') "없는 파일을 부르는 곁 훅이 하나라도 있으면 실패(받은 값 $LASTEXITCODE)"

        # 활동 훅만 빠진 모양(-NoActivityHook 설치): 경고만.
        $noAct = Join-Path $claudeDir 'no-activity.json'
        $na = ConvertFrom-GyeotJsonText (Read-GyeotTextFile $settings)
        [void](Remove-GyeotClaudeHook -Settings $na -EventName 'PostToolBatch')
        Write-GyeotTextFile -Path $noAct -Text ((ConvertTo-GyeotJson $na) + "`n")
        $t = (& $exe -NoProfile -ExecutionPolicy Bypass -File $doc -InstallRoot $root -ClaudeSettingsPath $noAct | Out-String)
        Assert-Boot ($LASTEXITCODE -eq 0 -and $t -match '활동 훅') "활동 훅만 없으면 경고만(받은 값 $LASTEXITCODE)"

        Stop-BootTestDaemon $root
        $t = (& $exe -NoProfile -ExecutionPolicy Bypass -File $doc -InstallRoot $root -ClaudeSettingsPath $settings | Out-String)
        Assert-Boot ($LASTEXITCODE -eq 1 -and $t -match '닿지 않아요') "데몬이 없으면 doctor 가 실패(받은 값 $LASTEXITCODE)"

        # 다른 프로그램이 포트를 잡으면: "닿지 않음"이 아니라 "다른 프로그램"으로 알리고 실패.
        $job = Start-BootFakeServer $port "HTTP/1.1 404 Not Found`r`nContent-Length: 0`r`nConnection: close`r`n`r`n"
        try {
            $t = (& $exe -NoProfile -ExecutionPolicy Bypass -File $doc -InstallRoot $root -ClaudeSettingsPath $settings | Out-String)
            Assert-Boot ($LASTEXITCODE -eq 1 -and $t -match '다른 프로그램이 쓰고 있어요' -and $t -notmatch '닿지 않아요') "포트를 다른 프로그램이 잡으면 그렇게 알린다(받은 값 $LASTEXITCODE)"
        } finally { Stop-BootFakeServer $job }

        Write-Host '== 8. 지우기: 곁 훅만 빠지고 사용자 훅은 남는다' -ForegroundColor White
        $u = (& $exe -NoProfile -ExecutionPolicy Bypass -File (Join-Path (Join-Path $root 'scripts') 'uninstall.ps1') -InstallRoot $root -ClaudeSettingsPath $settings -RemoveClaudeHooks | Out-String)
        Assert-Boot ($LASTEXITCODE -eq 0) "지우기 종료 코드 0 (받은 값 $LASTEXITCODE)"
        $tree = ConvertFrom-GyeotJsonText (Read-GyeotTextFile $settings)
        Assert-Boot ((Get-GyeotHookCount $tree 'Stop') -eq 0 -and ([string]$tree['hooks']['Stop'][0]['hooks'][0]['command']) -eq 'my-own-stop-hook') '곁 훅만 빠졌다'
        Assert-Boot ($u -match 'claude mcp remove gyeot') '지우기가 channel 등록 지우는 법을 알려 준다'
    }
} finally {
    if ($env:GYEOT_HOME -and (Test-GyeotPathUnder $env:GYEOT_HOME $tmpRoot)) { Stop-BootTestDaemon $env:GYEOT_HOME; Remove-Item Env:GYEOT_HOME }
    if (-not $TestKeep) { Remove-Item -LiteralPath $tmpRoot -Recurse -Force -ErrorAction SilentlyContinue }
}

Write-Host ''
if ($script:fails -eq 0) { Write-Host "모두 통과 ($($script:passes))" -ForegroundColor White; exit 0 }
Write-Host "실패 $($script:fails) / 통과 $($script:passes)" -ForegroundColor Red
exit 1
