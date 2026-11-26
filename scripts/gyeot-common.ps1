# 곁(gyeot) 설치 스크립트 공용 함수. install.ps1·uninstall.ps1·test-install.ps1이 점 소싱한다.
# Windows PowerShell 5.1과 PowerShell 7 모두에서 돈다. 이 파일은 UTF-8(BOM)으로 저장한다.

Set-StrictMode -Version 2.0

# ───────────────────────── 이름 ─────────────────────────

$script:GyeotTaskName = 'gyeot-daemon'
$script:GyeotRunValueDaemon = 'gyeotd'
$script:GyeotRunValueShell = 'gyeot-shell'   # 셸 트레이의 "로그인 시 시작"(tauri-plugin-autostart)
$script:GyeotRunKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'

# Claude Code 훅: 이벤트 이름 → matcher를 쓰는 이벤트인지(공식 hooks 레퍼런스 2026-11 기준)
$script:GyeotClaudeEvents = @(
    @{ Name = 'SessionStart';       Matcher = $true },
    @{ Name = 'SessionEnd';         Matcher = $true },
    @{ Name = 'UserPromptSubmit';   Matcher = $false },
    @{ Name = 'Notification';       Matcher = $true },
    @{ Name = 'PermissionRequest';  Matcher = $true },
    @{ Name = 'Stop';               Matcher = $false },
    @{ Name = 'StopFailure';        Matcher = $true },
    @{ Name = 'SubagentStart';      Matcher = $true },
    @{ Name = 'SubagentStop';       Matcher = $true },
    @{ Name = 'PostToolUseFailure'; Matcher = $true },
    @{ Name = 'TaskCompleted';      Matcher = $false },
    @{ Name = 'PreCompact';         Matcher = $true }
)
# 기본(docs/contracts.md §12.3): 병렬 도구 묶음이 끝나면 한 번 불린다(승인을 기다린 도구 포함).
# 사람이 터미널에서 승인한 뒤 곁의 승인 대기를 닫는 신호라 기본으로 넣는다. matcher가 없는 이벤트다.
# install.ps1 -NoActivityHook 이면 빼고, 이미 넣은 곁의 항목도 걷어 낸다.
$script:GyeotClaudeActivityEvent = @{ Name = 'PostToolBatch'; Matcher = $false }

# Codex 훅(~/.codex/hooks.json). Codex에 있는 이벤트 가운데 곁이 정규화하는 것만.
$script:GyeotCodexEvents = @('SessionStart', 'SessionEnd', 'UserPromptSubmit', 'PermissionRequest', 'Stop', 'SubagentStart', 'SubagentStop', 'PreCompact')

# ───────────────────────── 출력 ─────────────────────────

function Write-GyeotStep([string]$Message) { Write-Host "==> $Message" -ForegroundColor Cyan }
function Write-GyeotInfo([string]$Message) { Write-Host "    $Message" }
function Write-GyeotOk([string]$Message) { Write-Host "    ✓ $Message" -ForegroundColor Green }
function Write-GyeotWarn([string]$Message) { Write-Host "  ! $Message" -ForegroundColor Yellow }
function Write-GyeotPlan([string]$Message) { Write-Host "    [미리보기] $Message" -ForegroundColor DarkGray }

function Write-GyeotBlock([string]$Title, [string]$Body) {
    Write-Host ''
    Write-Host "----- $Title -----" -ForegroundColor Magenta
    Write-Host $Body
    Write-Host ('-' * ($Title.Length + 12)) -ForegroundColor Magenta
}

# ───────────────────────── 경로 ─────────────────────────

function Get-GyeotDefaultRoot {
    if ($env:GYEOT_HOME) { return $env:GYEOT_HOME }
    if ($env:LOCALAPPDATA) { return (Join-Path $env:LOCALAPPDATA 'gyeot') }
    throw 'LOCALAPPDATA 환경 변수가 없어 설치 폴더를 정하지 못했어요. -InstallRoot 로 알려 주세요.'
}

function Get-GyeotHomeDir {
    if ($env:USERPROFILE) { return $env:USERPROFILE }
    if ($env:HOME) { return $env:HOME }
    return [Environment]::GetFolderPath('UserProfile')
}

# 상대 경로는 PowerShell의 현재 위치를 기준으로 푼다(프로세스 작업 폴더가 아니라).
function Get-GyeotFullPath([string]$Path) {
    return $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($Path)
}

# 두 경로가 같은 파일을 가리키는지(대소문자 무시). 해석할 수 없는 경로(없는 드라이브 등)는 글자 그대로 비교한다.
function Test-GyeotSamePath([string]$A, [string]$B) {
    if (-not $A -or -not $B) { return $false }
    $fa = $A; $fb = $B
    try { $fa = Get-GyeotFullPath $A } catch { $fa = $A }
    try { $fb = Get-GyeotFullPath $B } catch { $fb = $B }
    return $fa.TrimEnd('\', '/').Equals($fb.TrimEnd('\', '/'), [StringComparison]::OrdinalIgnoreCase)
}

function Test-GyeotPathUnder([string]$Path, [string]$Root) {
    if (-not $Path -or -not $Root) { return $false }
    $p = (Get-GyeotFullPath $Path).TrimEnd('\', '/')
    $r = (Get-GyeotFullPath $Root).TrimEnd('\', '/')
    return ($p.Equals($r, [StringComparison]::OrdinalIgnoreCase) -or
        $p.StartsWith($r + [System.IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase) -or
        $p.StartsWith($r + '/', [StringComparison]::OrdinalIgnoreCase))
}

# 공백이 있는 경로를 셸 문자열로 넘겨야 할 때(Codex 훅 command) 8.3 짧은 이름을 쓴다. 없으면 그대로.
function Get-GyeotShortPath([string]$Path) {
    if ($Path -notmatch '\s') { return $Path }
    try {
        $fso = New-Object -ComObject Scripting.FileSystemObject
        $short = $fso.GetFile($Path).ShortPath
        if ($short -and $short -notmatch '\s') { return $short }
    } catch {
        Write-Verbose "짧은 경로를 얻지 못함: $($_.Exception.Message)"
    }
    return $Path
}

# ───────────────────────── 폴더 권한(Windows, docs/contracts.md §13.4) ─────────────────────────
# %LOCALAPPDATA% 아래는 그 사용자 전용 권한을 이어받는다. 그 밖(C:\gyeot 등)은 Authenticated Users가
# 수정 권한을 물려받을 수 있어서, 다른 사용자가 token을 읽거나 Claude Code가 실행하는 gyeot-hook.exe를
# 바꿔칠 수 있다(R3 M4). 그런 폴더는 상속을 끊고 그 사용자와 SYSTEM만 남긴다.

$script:GyeotSystemSid = 'S-1-5-18'
$script:GyeotAdminsSid = 'S-1-5-32-544'

# Windows인지(Windows PowerShell 5.1에는 $IsWindows 가 없다).
function Test-GyeotWindowsHost {
    return ([System.Environment]::OSVersion.Platform -eq [System.PlatformID]::Win32NT)
}

# %LOCALAPPDATA% 아래인지. LOCALAPPDATA를 모르면 밖으로 본다(좁히는 쪽).
# 8.3 짧은 이름(RUNNER~1 등)은 펼치지 않으므로 안쪽이어도 밖으로 볼 수 있다(그때도 좁히기만 한다).
function Test-GyeotUnderLocalAppData([string]$Path) {
    if (-not $env:LOCALAPPDATA) { return $false }
    return (Test-GyeotPathUnder $Path $env:LOCALAPPDATA)
}

# 좁힐 폴더: 주어진 폴더(설치 폴더·데이터 폴더) 가운데 %LOCALAPPDATA% 밖인 것. 빈 값·중복은 뺀다.
function Get-GyeotPrivateFolderPlan([string[]]$Folders) {
    $out = New-Object System.Collections.Generic.List[string]
    foreach ($f in $Folders) {
        if (-not $f) { continue }
        $full = (Get-GyeotFullPath $f).TrimEnd('\', '/')
        if (Test-GyeotUnderLocalAppData $full) { continue }
        $dup = $false
        foreach ($o in $out) { if ($o.Equals($full, [StringComparison]::OrdinalIgnoreCase)) { $dup = $true } }
        if (-not $dup) { $out.Add($full) }
    }
    # 호출하는 쪽이 @(...)로 감싼다(빈 목록도 그대로 빈 배열이 되게).
    return $out.ToArray()
}

function Get-GyeotCurrentUserSid {
    return [System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value
}

function Get-GyeotSidName([string]$Sid) {
    try {
        return (New-Object System.Security.Principal.SecurityIdentifier($Sid)).Translate([System.Security.Principal.NTAccount]).Value
    } catch {
        return $Sid
    }
}

function Get-GyeotIcaclsPath {
    $sysRoot = $env:SystemRoot
    if (-not $sysRoot) { $sysRoot = 'C:\Windows' }
    return ($sysRoot.TrimEnd('\') + '\System32\icacls.exe')
}

# icacls 인자(§13.4): 상속을 끊고(/inheritance:r) 그 사용자와 SYSTEM에 모든 권한(하위 폴더·파일로 이어짐).
function Get-GyeotIcaclsArgument([string]$Folder, [string]$UserSid) {
    return @($Folder, '/inheritance:r', '/grant:r', "*$($UserSid):(OI)(CI)F", '/grant:r', "*$($script:GyeotSystemSid):(OI)(CI)F")
}

# 폴더(파일) ACL 요약: 상속이 끊겼는지, 항목(SID·허용/거부·권한·물려받음), 주인 SID.
function Get-GyeotAclSummary([string]$Path) {
    $acl = Get-Acl -LiteralPath $Path
    $rules = @()
    foreach ($r in $acl.GetAccessRules($true, $true, [System.Security.Principal.SecurityIdentifier])) {
        $rules += [pscustomobject]@{
            Sid       = $r.IdentityReference.Value
            Type      = [string]$r.AccessControlType
            Rights    = [string]$r.FileSystemRights
            Inherited = [bool]$r.IsInherited
        }
    }
    return [pscustomobject]@{
        Protected = [bool]$acl.AreAccessRulesProtected
        Rules     = $rules
        Owner     = $acl.GetOwner([System.Security.Principal.SecurityIdentifier]).Value
    }
}

# 주인이 그 사용자·SYSTEM·Administrators가 아니면 그 주인은 언제든 권한을 다시 넓힐 수 있다.
function Test-GyeotTrustedOwner([string]$OwnerSid, [string]$UserSid) {
    return ($OwnerSid -eq $UserSid -or $OwnerSid -eq $script:GyeotSystemSid -or $OwnerSid -eq $script:GyeotAdminsSid)
}

# 요약이 §13.4를 지키는지. 지키면 아무것도, 아니면 까닭들을 내보낸다.
# 지킬 것: 상속이 끊김(폴더만), 항목은 허용뿐이고 주체는 그 사용자·SYSTEM뿐, 둘 다 모든 권한, 주인이 믿을 만함.
function Get-GyeotAclProblem($Summary, [string]$UserSid, [switch]$Child) {
    $problems = @()
    if (-not $Child -and -not $Summary.Protected) { $problems += '상속이 끊기지 않았어요' }
    foreach ($r in $Summary.Rules) {
        if ($r.Sid -ne $UserSid -and $r.Sid -ne $script:GyeotSystemSid) {
            $problems += "다른 주체가 남았어요: $(Get-GyeotSidName $r.Sid) ($($r.Type) $($r.Rights))"
        } elseif ($r.Type -ne 'Allow') {
            $problems += "거부 항목이 있어요: $(Get-GyeotSidName $r.Sid)"
        }
    }
    foreach ($sid in @($UserSid, $script:GyeotSystemSid)) {
        $full = @($Summary.Rules | Where-Object { $_.Sid -eq $sid -and $_.Type -eq 'Allow' -and $_.Rights -match 'FullControl' })
        if ($full.Count -eq 0) { $problems += "$(Get-GyeotSidName $sid) 에게 모든 권한이 없어요" }
    }
    if (-not (Test-GyeotTrustedOwner $Summary.Owner $UserSid)) {
        $problems += "주인이 다른 사용자예요: $(Get-GyeotSidName $Summary.Owner)"
    }
    # 호출하는 쪽이 @(...)로 감싼다.
    return $problems
}

function Format-GyeotAclSummary($Summary) {
    $parts = @()
    foreach ($r in $Summary.Rules) {
        $how = '물려받음'
        if (-not $r.Inherited) { $how = '직접' }
        $parts += "$(Get-GyeotSidName $r.Sid) [$($r.Sid)] $($r.Type) $($r.Rights) ($how)"
    }
    $inherit = '상속 이어받음'
    if ($Summary.Protected) { $inherit = '상속 끊김' }
    return "$inherit; 허용: $($parts -join ', '); 주인: $(Get-GyeotSidName $Summary.Owner)"
}

# §13.4대로 좁힌다: icacls 로 상속을 끊고 그 사용자·SYSTEM만 남긴 뒤, 남은 다른 주체의 직접 항목을 빼고,
# 이미 있던 하위 항목은 이 폴더만 이어받게 다시 맞춘다. 결과 요약을 돌려준다.
function Set-GyeotFolderPrivate([string]$Folder, [string]$UserSid) {
    $icacls = Get-GyeotIcaclsPath
    $icaclsArgs = Get-GyeotIcaclsArgument $Folder $UserSid
    $out = & $icacls @icaclsArgs
    if ($LASTEXITCODE -ne 0) { throw "icacls 가 종료 코드 $LASTEXITCODE 로 끝났어요($Folder): $($out -join ' ')" }
    $others = @((Get-GyeotAclSummary $Folder).Rules | Where-Object { $_.Sid -ne $UserSid -and $_.Sid -ne $script:GyeotSystemSid } |
            ForEach-Object { $_.Sid } | Sort-Object -Unique)
    foreach ($sid in $others) {
        $out = & $icacls $Folder '/remove' "*$sid"
        if ($LASTEXITCODE -ne 0) { throw "icacls /remove 가 종료 코드 $LASTEXITCODE 로 끝났어요($Folder, $sid): $($out -join ' ')" }
    }
    if (@(Get-ChildItem -LiteralPath $Folder -Force).Count -gt 0) {
        $out = & $icacls (Join-Path $Folder '*') '/reset' '/T' '/C' '/Q'
        if ($LASTEXITCODE -ne 0) { Write-GyeotWarn "하위 항목 일부의 권한을 다시 맞추지 못했어요(icacls 종료 코드 $LASTEXITCODE): $($out -join ' ')" }
    }
    return (Get-GyeotAclSummary $Folder)
}

# ───────────────────────── JSON ─────────────────────────
# ConvertTo-Json은 PowerShell 판마다 들여쓰기·이스케이프가 달라서, 사용자의 settings.json을
# 다시 쓸 때는 같은 모양을 내는 직렬화기를 쓴다(2칸 들여쓰기, UTF-8 그대로).

function New-GyeotDict {
    return , (New-Object System.Collections.Specialized.OrderedDictionary ([StringComparer]::Ordinal))
}

function New-GyeotList([object[]]$Items) {
    $list = New-Object System.Collections.ArrayList
    if ($null -ne $Items) { foreach ($i in $Items) { [void]$list.Add($i) } }
    return , $list
}

function Format-GyeotJsonString([string]$Text) {
    $sb = New-Object System.Text.StringBuilder
    [void]$sb.Append('"')
    foreach ($ch in $Text.ToCharArray()) {
        $code = [int]$ch
        if ($code -eq 0x22) { [void]$sb.Append('\"') }
        elseif ($code -eq 0x5C) { [void]$sb.Append('\\') }
        elseif ($code -eq 0x0A) { [void]$sb.Append('\n') }
        elseif ($code -eq 0x0D) { [void]$sb.Append('\r') }
        elseif ($code -eq 0x09) { [void]$sb.Append('\t') }
        elseif ($code -eq 0x08) { [void]$sb.Append('\b') }
        elseif ($code -eq 0x0C) { [void]$sb.Append('\f') }
        elseif ($code -lt 0x20) { [void]$sb.Append(('\u{0:x4}' -f $code)) }
        else { [void]$sb.Append($ch) }
    }
    [void]$sb.Append('"')
    return $sb.ToString()
}

# 형식 이름은 모두 System.* 전체 이름으로 쓴다(Windows PowerShell 5.1에 없는 형식 별칭이 있어서).
function Test-GyeotNumber($Value) {
    if ($null -eq $Value) { return $false }
    if ($Value -is [System.Int32] -or $Value -is [System.Int64] -or $Value -is [System.Int16] -or
        $Value -is [System.Byte] -or $Value -is [System.SByte] -or $Value -is [System.UInt16] -or
        $Value -is [System.UInt32] -or $Value -is [System.UInt64] -or $Value -is [System.Decimal]) {
        return $true
    }
    return ($Value.GetType().FullName -eq 'System.Numerics.BigInteger')
}

function ConvertTo-GyeotJson {
    param($Value, [int]$Indent = 0)
    $inv = [System.Globalization.CultureInfo]::InvariantCulture
    $pad = '  ' * $Indent
    $pad1 = '  ' * ($Indent + 1)
    if ($null -eq $Value) { return 'null' }
    if ($Value -is [bool]) { if ($Value) { return 'true' } else { return 'false' } }
    if ($Value -is [string] -or $Value -is [char]) { return (Format-GyeotJsonString ([string]$Value)) }
    if ($Value -is [System.DateTime] -or $Value -is [System.DateTimeOffset]) {
        throw '날짜로 바뀐 문자열이 있어요(PowerShell 7.0–7.4의 ConvertFrom-Json). Windows PowerShell 5.1이나 PowerShell 7.5 이상으로 다시 실행하세요.'
    }
    if (Test-GyeotNumber $Value) { return ([System.Convert]::ToString($Value, $inv)) }
    if ($Value -is [System.Double] -or $Value -is [System.Single]) {
        $d = [double]$Value
        if ([double]::IsNaN($d) -or [double]::IsInfinity($d)) { throw 'JSON에 쓸 수 없는 숫자(NaN·Infinity)가 있어요.' }
        return $d.ToString('R', $inv)
    }
    if ($Value -is [System.Collections.IDictionary]) {
        if ($Value.Count -eq 0) { return '{}' }
        $parts = New-Object System.Collections.Generic.List[string]
        foreach ($k in $Value.Keys) {
            $parts.Add($pad1 + (Format-GyeotJsonString ([string]$k)) + ': ' + (ConvertTo-GyeotJson -Value $Value[$k] -Indent ($Indent + 1)))
        }
        return "{`n" + ($parts -join ",`n") + "`n$pad}"
    }
    if ($Value -is [System.Management.Automation.PSCustomObject]) {
        $props = @($Value.PSObject.Properties)
        if ($props.Count -eq 0) { return '{}' }
        $parts = New-Object System.Collections.Generic.List[string]
        foreach ($p in $props) {
            $parts.Add($pad1 + (Format-GyeotJsonString $p.Name) + ': ' + (ConvertTo-GyeotJson -Value $p.Value -Indent ($Indent + 1)))
        }
        return "{`n" + ($parts -join ",`n") + "`n$pad}"
    }
    if ($Value -is [System.Collections.IEnumerable]) {
        $items = New-Object System.Collections.Generic.List[string]
        foreach ($i in $Value) { $items.Add($pad1 + (ConvertTo-GyeotJson -Value $i -Indent ($Indent + 1))) }
        if ($items.Count -eq 0) { return '[]' }
        return "[`n" + ($items -join ",`n") + "`n$pad]"
    }
    return (Format-GyeotJsonString ([string]$Value))
}

# ConvertFrom-Json 결과(PSCustomObject·배열)를 고치기 쉬운 순서 사전·ArrayList 나무로 바꾼다.
function ConvertTo-GyeotTree {
    param($Value)
    if ($null -eq $Value) { return $null }
    if ($Value -is [string]) { return $Value }
    if ($Value -is [System.Management.Automation.PSCustomObject]) {
        $d = New-GyeotDict
        foreach ($p in $Value.PSObject.Properties) { $d[$p.Name] = (ConvertTo-GyeotTree -Value $p.Value) }
        return , $d
    }
    if ($Value -is [System.Collections.IDictionary]) {
        $d = New-GyeotDict
        foreach ($k in $Value.Keys) { $d[[string]$k] = (ConvertTo-GyeotTree -Value $Value[$k]) }
        return , $d
    }
    if ($Value -is [System.Collections.IList]) {
        $list = New-Object System.Collections.ArrayList
        foreach ($i in $Value) { [void]$list.Add((ConvertTo-GyeotTree -Value $i)) }
        return , $list
    }
    return $Value
}

function Test-GyeotHasDate {
    param($Value)
    if ($null -eq $Value) { return $false }
    if ($Value -is [System.DateTime] -or $Value -is [System.DateTimeOffset]) { return $true }
    if ($Value -is [string]) { return $false }
    if ($Value -is [System.Collections.IDictionary]) {
        foreach ($k in $Value.Keys) { if (Test-GyeotHasDate -Value $Value[$k]) { return $true } }
        return $false
    }
    if ($Value -is [System.Collections.IEnumerable]) {
        foreach ($i in $Value) { if (Test-GyeotHasDate -Value $i) { return $true } }
    }
    return $false
}

# JSON 글을 나무로 읽는다. 빈 글이면 빈 사전. 맨 위가 객체가 아니면 실패.
function ConvertFrom-GyeotJsonText {
    param([string]$Text, [string]$What = 'JSON')
    if ([string]::IsNullOrWhiteSpace($Text)) { return , (New-GyeotDict) }
    $cmd = Get-Command ConvertFrom-Json
    try {
        if ($cmd.Parameters.ContainsKey('DateKind')) {
            $parsed = ConvertFrom-Json -InputObject $Text -DateKind String
        } else {
            $parsed = ConvertFrom-Json -InputObject $Text
        }
    } catch {
        throw "$What 을(를) JSON으로 읽지 못했어요: $($_.Exception.Message)"
    }
    $tree = ConvertTo-GyeotTree -Value $parsed
    if ($tree -isnot [System.Collections.Specialized.OrderedDictionary]) {
        throw "$What 의 맨 위가 JSON 객체({ … })가 아니에요."
    }
    if (Test-GyeotHasDate -Value $tree) {
        throw "$What 에 날짜처럼 보이는 문자열이 있어 이 PowerShell($($PSVersionTable.PSVersion))로는 그대로 다시 쓸 수 없어요. Windows PowerShell 5.1(powershell.exe)이나 PowerShell 7.5 이상으로 다시 실행하세요."
    }
    return , $tree
}

function Read-GyeotTextFile([string]$Path) {
    return [System.IO.File]::ReadAllText($Path)
}

# UTF-8(BOM 없음)으로 쓴다. Node의 JSON.parse는 BOM을 받지 않는다.
# 보통은 임시 파일에 쓴 뒤 바꿔 끼우고(중간에 끊겨도 반쪽 파일이 남지 않게),
# 심볼릭 링크(dotfiles)면 링크가 깨지지 않도록 대상에 바로 쓴다.
function Write-GyeotTextFile([string]$Path, [string]$Text) {
    $utf8 = New-Object System.Text.UTF8Encoding($false)
    $dir = Split-Path -Parent $Path
    if ($dir -and -not (Test-Path -LiteralPath $dir)) { New-Item -ItemType Directory -Path $dir -Force | Out-Null }
    if (Test-Path -LiteralPath $Path) {
        $item = Get-Item -LiteralPath $Path -Force
        if ($item.LinkType) {
            [System.IO.File]::WriteAllText($Path, $Text, $utf8)
            return
        }
    }
    $tmp = "$Path.gyeot-tmp"
    [System.IO.File]::WriteAllText($tmp, $Text, $utf8)
    try {
        if (Test-Path -LiteralPath $Path) {
            [System.IO.File]::Replace($tmp, $Path, [NullString]::Value)
        } else {
            [System.IO.File]::Move($tmp, $Path)
        }
    } catch {
        [System.IO.File]::Copy($tmp, $Path, $true)
        Remove-Item -LiteralPath $tmp -Force -ErrorAction SilentlyContinue
    }
}

# ───────────────────────── Claude Code 훅 ─────────────────────────

# 곁이 넣는 Claude Code 훅 이벤트. 기본은 PostToolBatch 포함, -NoActivity 이면 뺀다.
function Get-GyeotClaudeEventList([switch]$NoActivity) {
    $list = @($script:GyeotClaudeEvents)
    if (-not $NoActivity) { $list += $script:GyeotClaudeActivityEvent }
    return $list
}

function New-GyeotClaudeHandler([string]$HookExe) {
    $h = New-GyeotDict
    $h['type'] = 'command'
    $h['command'] = $HookExe
    $h['args'] = (New-GyeotList @('claude-code'))
    $h['async'] = $true
    $h['timeout'] = 10
    return , $h
}

function New-GyeotClaudeGroup([string]$HookExe, [bool]$Matcher) {
    $g = New-GyeotDict
    if ($Matcher) { $g['matcher'] = '*' }
    $g['hooks'] = (New-GyeotList @(, (New-GyeotClaudeHandler $HookExe)))
    return , $g
}

# 곁의 훅 처리기인지: command가 gyeot-hook(.exe)를 가리키면.
function Test-GyeotHookHandler($Handler) {
    if ($Handler -isnot [System.Collections.IDictionary]) { return $false }
    if (-not $Handler.Contains('command')) { return $false }
    $cmd = $Handler['command']
    if ($cmd -isnot [string]) { return $false }
    return ($cmd -match '(?i)(^|[\\/"''\s])gyeot-hook(\.exe)?(["''\s]|$)')
}

# Claude Code 훅 조각 전체({ "hooks": { … } }).
function New-GyeotClaudeHooksSnippet([string]$HookExe, [switch]$NoActivity) {
    $hooks = New-GyeotDict
    foreach ($ev in (Get-GyeotClaudeEventList -NoActivity:$NoActivity)) {
        $hooks[$ev.Name] = (New-GyeotList @(, (New-GyeotClaudeGroup $HookExe $ev.Matcher)))
    }
    $root = New-GyeotDict
    $root['hooks'] = $hooks
    return , $root
}

# settings 나무에 곁의 훅을 더한다. 기존 훅은 지우지 않는다. 이미 있으면 경로만 맞춘다.
# -NoActivity 이면 PostToolBatch 를 더하지 않고, 이미 있는 곁의 PostToolBatch 처리기만 뺀다.
# 셸 형식(args 없음)으로 손수 넣은 곁 항목은 고치지 않는다. 그 항목이 이 설치의 gyeot-hook 를
# 부르면 "있음"으로 치고, 다른 경로를 부르면 그대로 둔 채 이 설치의 항목을 더하고 Stale 로 센다.
# 돌려주는 값: @{ Added = n; Updated = n; Kept = n; Removed = n; Stale = n; StalePaths = @(…) }
function Merge-GyeotClaudeHook {
    param($Settings, [string]$HookExe, [switch]$NoActivity)
    $result = @{ Added = 0; Updated = 0; Kept = 0; Removed = 0; Stale = 0; StalePaths = @() }
    if (-not $Settings.Contains('hooks') -or $null -eq $Settings['hooks']) {
        $Settings['hooks'] = (New-GyeotDict)
    }
    $hooks = $Settings['hooks']
    if ($hooks -isnot [System.Collections.IDictionary]) { throw 'settings.json의 "hooks"가 객체가 아니에요. 손으로 확인해 주세요.' }
    if ($NoActivity) {
        $result.Removed = Remove-GyeotClaudeHook -Settings $Settings -EventName $script:GyeotClaudeActivityEvent.Name
    }
    foreach ($ev in (Get-GyeotClaudeEventList -NoActivity:$NoActivity)) {
        $name = $ev.Name
        if (-not $hooks.Contains($name) -or $null -eq $hooks[$name]) { $hooks[$name] = (New-GyeotList @()) }
        $groups = $hooks[$name]
        if ($groups -isnot [System.Collections.IList]) { throw "settings.json의 hooks.$name 이 배열이 아니에요. 손으로 확인해 주세요." }
        $found = $false
        foreach ($group in $groups) {
            if ($group -isnot [System.Collections.IDictionary] -or -not $group.Contains('hooks')) { continue }
            $handlers = $group['hooks']
            if ($handlers -isnot [System.Collections.IList]) { continue }
            foreach ($h in $handlers) {
                if (-not (Test-GyeotHookHandler $h)) { continue }
                if (-not $h.Contains('args')) {
                    # 셸 형식으로 손수 넣은 항목: 건드리지 않는다.
                    $exe = Get-GyeotHookHandlerExe $h
                    if (Test-GyeotSamePath $exe $HookExe) { $found = $true; $result.Kept++ }
                    else {
                        $result.Stale++
                        if ($result.StalePaths -notcontains $exe) { $result.StalePaths += $exe }
                    }
                    continue
                }
                $found = $true
                $wanted = New-GyeotClaudeHandler $HookExe
                $same = ($h['command'] -eq $HookExe) -and (ConvertTo-GyeotJson $h['args']) -eq (ConvertTo-GyeotJson $wanted['args'])
                if ($same) { $result.Kept++; continue }
                $h['command'] = $HookExe
                $h['args'] = $wanted['args']
                $result.Updated++
            }
        }
        if (-not $found) {
            [void]$groups.Add((New-GyeotClaudeGroup $HookExe $ev.Matcher))
            $result.Added++
        }
    }
    return $result
}

# 곁의 처리기만 뺀다(PostToolBatch 포함). 그 때문에 빈 묶음·빈 이벤트가 되면 그것도 뺀다.
# -EventName 을 주면 그 이벤트에서만 뺀다. 돌려주는 값: 뺀 수.
function Remove-GyeotClaudeHook {
    param($Settings, [string]$EventName)
    $removed = 0
    if (-not $Settings.Contains('hooks')) { return 0 }
    $hooks = $Settings['hooks']
    if ($hooks -isnot [System.Collections.IDictionary]) { return 0 }
    foreach ($name in @($hooks.Keys)) {
        if ($EventName -and $name -cne $EventName) { continue }
        $groups = $hooks[$name]
        if ($groups -isnot [System.Collections.IList]) { continue }
        $touched = $false
        foreach ($group in @($groups)) {
            if ($group -isnot [System.Collections.IDictionary] -or -not $group.Contains('hooks')) { continue }
            $handlers = $group['hooks']
            if ($handlers -isnot [System.Collections.IList]) { continue }
            $groupTouched = $false
            foreach ($h in @($handlers)) {
                if (Test-GyeotHookHandler $h) { $handlers.Remove($h); $removed++; $groupTouched = $true }
            }
            if ($groupTouched) {
                $touched = $true
                if ($handlers.Count -eq 0) { $groups.Remove($group) }
            }
        }
        if ($touched -and $groups.Count -eq 0) { $hooks.Remove($name) }
    }
    return $removed
}

function Get-GyeotHookCount {
    param($Settings, [string]$EventName)
    if (-not $Settings.Contains('hooks')) { return 0 }
    $hooks = $Settings['hooks']
    if ($hooks -isnot [System.Collections.IDictionary] -or -not $hooks.Contains($EventName)) { return 0 }
    $n = 0
    foreach ($group in $hooks[$EventName]) {
        if ($group -isnot [System.Collections.IDictionary] -or -not $group.Contains('hooks')) { continue }
        foreach ($h in $group['hooks']) { if (Test-GyeotHookHandler $h) { $n++ } }
    }
    return $n
}

# 파일을 읽어 고치고(백업 후) 다시 쓴다. $Mutate는 나무를 받아 바뀐 수를 돌려주는 스크립트 블록.
function Update-GyeotJsonFile {
    param([string]$Path, [scriptblock]$Mutate, [switch]$DryRun, [string]$What = 'settings.json')
    $exists = Test-Path -LiteralPath $Path
    $raw = ''
    if ($exists) { $raw = Read-GyeotTextFile $Path }
    $tree = ConvertFrom-GyeotJsonText -Text $raw -What $What
    $before = ConvertTo-GyeotJson $tree
    $changes = & $Mutate $tree
    $after = ConvertTo-GyeotJson $tree
    if ($before -eq $after) {
        return @{ Changed = $false; Backup = $null; Detail = $changes }
    }
    $newline = "`n"
    if ($raw.Contains("`r`n")) { $newline = "`r`n" }
    $text = ($after -replace "`n", $newline) + $newline
    # 쓴 글이 다시 읽히는지 먼저 확인한다.
    [void](ConvertFrom-GyeotJsonText -Text $text -What "새 $What")
    if ($DryRun) {
        return @{ Changed = $true; Backup = $null; Detail = $changes; Preview = $text }
    }
    $backup = $null
    if ($exists) {
        $backup = "$Path.bak-$(Get-Date -Format 'yyyyMMdd-HHmmss')"
        Copy-Item -LiteralPath $Path -Destination $backup -Force
    }
    Write-GyeotTextFile -Path $Path -Text $text
    return @{ Changed = $true; Backup = $backup; Detail = $changes }
}

# ───────────────────────── 실행 파일 ─────────────────────────

# PE 머리의 Subsystem: 2 = GUI(창 없음), 3 = 콘솔. 읽지 못하면 0.
function Get-GyeotPeSubsystem([string]$Path) {
    try {
        $fs = [System.IO.File]::OpenRead($Path)
        try {
            $br = New-Object System.IO.BinaryReader($fs)
            if ($fs.Length -lt 0x40 -or $br.ReadUInt16() -ne 0x5A4D) { return 0 }
            $fs.Position = 0x3C
            $pe = $br.ReadInt32()
            if ($pe -le 0 -or $pe + 4 + 20 + 70 -gt $fs.Length) { return 0 }
            $fs.Position = $pe
            if ($br.ReadUInt32() -ne 0x00004550) { return 0 }
            $fs.Position = $pe + 4 + 20 + 68
            return [int]$br.ReadUInt16()
        } finally {
            $fs.Dispose()
        }
    } catch {
        return 0
    }
}

# 데몬을 창 없이 띄우는 명령. GUI 서브시스템이면 그대로, 콘솔이면 숨긴 PowerShell로 감싼다.
function Get-GyeotDaemonLaunch([string]$DaemonExe, [string]$ProbePath) {
    $dir = Split-Path -Parent $DaemonExe
    if (-not $ProbePath) { $ProbePath = $DaemonExe }
    if ((Get-GyeotPeSubsystem $ProbePath) -eq 2) {
        return @{ Execute = $DaemonExe; Arguments = ''; WorkingDirectory = $dir; Wrapped = $false }
    }
    $sysRoot = $env:SystemRoot
    if (-not $sysRoot) { $sysRoot = 'C:\Windows' }
    $ps = $sysRoot.TrimEnd('\') + '\System32\WindowsPowerShell\v1.0\powershell.exe'
    $q = { param($s) "'" + ($s -replace "'", "''") + "'" }
    $inner = "Start-Process -FilePath $(& $q $DaemonExe) -WorkingDirectory $(& $q $dir) -WindowStyle Hidden"
    $arguments = "-NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -WindowStyle Hidden -Command `"$inner`""
    return @{ Execute = $ps; Arguments = $arguments; WorkingDirectory = $dir; Wrapped = $true }
}

function Get-GyeotProcessesUnder([string[]]$Names, [string]$Root) {
    $found = @()
    # Windows 밖(시험)에서는 프로세스 이름에 .exe 가 붙어 있다.
    if (-not (Test-GyeotWindowsHost)) { $Names = @($Names) + @($Names | ForEach-Object { "$_.exe" }) }
    foreach ($n in $Names) {
        foreach ($p in @(Get-Process -Name $n -ErrorAction SilentlyContinue)) {
            $path = $null
            try { $path = $p.Path } catch { $path = $null }
            if ($path -and (Test-GyeotPathUnder $path $Root)) { $found += $p }
        }
    }
    return , $found
}

function Get-GyeotRunValue([string]$Name) {
    try {
        $item = Get-ItemProperty -LiteralPath $script:GyeotRunKey -Name $Name -ErrorAction Stop
        return [string]$item.$Name
    } catch {
        return $null
    }
}

# TOML 문자열: 작은따옴표(리터럴)가 되면 그것, 아니면 큰따옴표로 이스케이프.
function ConvertTo-GyeotTomlString([string]$Text) {
    if ($Text -notmatch "['\r\n]") { return "'" + $Text + "'" }
    return '"' + ($Text -replace '\\', '\\' -replace '"', '\"' -replace "`r", '\r' -replace "`n", '\n') + '"'
}

# ───────────────────────── 점검(doctor.ps1·bootstrap.ps1) ─────────────────────────

# `gyeotd --print-config` 출력에서 경로·포트를 읽는다. 데몬과 같은 해석을 쓰기 위해서다.
function ConvertFrom-GyeotPrintConfig([string]$Text) {
    $r = @{ Home = $null; Token = $null; Config = $null; Face = $null; Port = 47321 }
    foreach ($line in ($Text -split "`r?`n")) {
        if ($line -match '^#\s*데이터 폴더:\s*(.+)$') { $r.Home = $Matches[1].Trim() }
        elseif ($line -match '^#\s*토큰:\s*(.+)$') { $r.Token = $Matches[1].Trim() }
        elseif ($line -match '^#\s*설정 파일:\s*(.+)$') { $r.Config = $Matches[1].Trim() }
        elseif ($line -match '^#\s*얼굴 폴더:\s*(.+)$') { $r.Face = $Matches[1].Trim() }
        elseif ($line -match '^port\s*=\s*(\d+)\s*$') { $r.Port = [int]$Matches[1] }
    }
    return $r
}

# 계약 §13.1: hex(HMAC-SHA256(key = 토큰 UTF-8, msg = "gyeot-hello/v1|<포트>|<n>")).
function Get-GyeotHelloProof([string]$Token, [int]$Port, [string]$Nonce) {
    $key = [System.Text.Encoding]::UTF8.GetBytes($Token)
    $msg = [System.Text.Encoding]::UTF8.GetBytes("gyeot-hello/v1|$Port|$Nonce")
    $h = New-Object System.Security.Cryptography.HMACSHA256 (, $key)
    try { $mac = $h.ComputeHash($msg) } finally { $h.Dispose() }
    return (($mac | ForEach-Object { $_.ToString('x2') }) -join '')
}

function New-GyeotNonce {
    $b = New-Object byte[] 32
    $rng = [System.Security.Cryptography.RandomNumberGenerator]::Create()
    try { $rng.GetBytes($b) } finally { $rng.Dispose() }
    return (($b | ForEach-Object { $_.ToString('x2') }) -join '')
}

# 길이가 같은 두 hex 글을 상수 시간으로 비교한다(대소문자 무시).
function Test-GyeotSameHex([string]$A, [string]$B) {
    if ($null -eq $A -or $null -eq $B) { return $false }
    $a = $A.ToLowerInvariant(); $b = $B.ToLowerInvariant()
    if ($a.Length -ne $b.Length -or $a.Length -eq 0) { return $false }
    $diff = 0
    for ($i = 0; $i -lt $a.Length; $i++) { $diff = $diff -bor ([int][char]$a[$i] -bxor [int][char]$b[$i]) }
    return ($diff -eq 0)
}

# `node --version`(v22.11.0 꼴)의 큰 판. 읽지 못하면 0.
function Get-GyeotNodeMajor([string]$VersionText) {
    if ($VersionText -match '^\s*v?(\d+)\.') { return [int]$Matches[1] }
    return 0
}

# 점검 결과 한 줄. Status: ok · warn · fail · skip
function New-GyeotCheck([string]$Name, [string]$Status, [string]$Detail) {
    return [pscustomobject]@{ Name = $Name; Status = $Status; Detail = $Detail }
}

function Write-GyeotCheck($Check) {
    switch ($Check.Status) {
        'ok'   { Write-Host "  ✓ $($Check.Name) — $($Check.Detail)" -ForegroundColor Green }
        'warn' { Write-Host "  ! $($Check.Name) — $($Check.Detail)" -ForegroundColor Yellow }
        'fail' { Write-Host "  ✗ $($Check.Name) — $($Check.Detail)" -ForegroundColor Red }
        default { Write-Host "  - $($Check.Name) — $($Check.Detail)" -ForegroundColor DarkGray }
    }
}

# 127.0.0.1 에 HTTP/1.1 GET 을 직접 보낸다. 시스템 프록시 설정을 타지 않게 소켓을 쓴다.
# 연결하지 못하면 예외. 연결됐으면 @{ Status = 상태 코드(HTTP 꼴이 아니면 0); Body = 본문 } 을 돌려준다.
# 응답은 64 KiB까지만 읽는다.
function Get-GyeotLoopbackText([int]$Port, [string]$PathAndQuery, [int]$TimeoutMs = 3000) {
    $client = New-Object System.Net.Sockets.TcpClient
    try {
        $ar = $client.BeginConnect('127.0.0.1', $Port, $null, $null)
        if (-not $ar.AsyncWaitHandle.WaitOne($TimeoutMs)) { throw "127.0.0.1:$Port 연결 시간 초과" }
        $client.EndConnect($ar)
        $client.ReceiveTimeout = $TimeoutMs
        $client.SendTimeout = $TimeoutMs
        $stream = $client.GetStream()
        $req = "GET $PathAndQuery HTTP/1.1`r`nHost: 127.0.0.1:$Port`r`nConnection: close`r`nAccept: application/json`r`n`r`n"
        $bytes = [System.Text.Encoding]::ASCII.GetBytes($req)
        $stream.Write($bytes, 0, $bytes.Length)
        $buf = New-Object byte[] 65536
        $ms = New-Object System.IO.MemoryStream
        try {
            while ($ms.Length -lt 65536) {
                $n = $stream.Read($buf, 0, $buf.Length)
                if ($n -le 0) { break }
                $ms.Write($buf, 0, $n)
            }
        } catch [System.IO.IOException] {
            if ($ms.Length -eq 0) { return @{ Status = 0; Body = '' } }
        }
        $text = [System.Text.Encoding]::UTF8.GetString($ms.ToArray())
    } finally { $client.Close() }
    $sep = $text.IndexOf("`r`n`r`n")
    if ($sep -lt 0 -or $text -notmatch '^HTTP/1\.[01] (\d{3}) ') { return @{ Status = 0; Body = $text } }
    $status = [int]$Matches[1]
    $head = $text.Substring(0, $sep)
    $body = $text.Substring($sep + 4)
    if ($head -match '(?im)^transfer-encoding:\s*chunked') {
        $out = New-Object System.Text.StringBuilder
        $rest = $body
        while ($rest.Length -gt 0) {
            $nl = $rest.IndexOf("`r`n")
            if ($nl -lt 0) { break }
            $size = [Convert]::ToInt32(($rest.Substring(0, $nl).Split(';')[0]).Trim(), 16)
            if ($size -eq 0) { break }
            [void]$out.Append($rest.Substring($nl + 2, [Math]::Min($size, $rest.Length - $nl - 2)))
            $rest = $rest.Substring([Math]::Min($rest.Length, $nl + 2 + $size + 2))
        }
        $body = $out.ToString()
    }
    return @{ Status = $status; Body = $body }
}

# 곁 훅 처리기가 부르는 실행 파일 경로. exec 형식이면 command, 셸 형식이면 그 안의 gyeot-hook 경로.
function Get-GyeotHookHandlerExe($Handler) {
    $cmd = [string]$Handler['command']
    if ($Handler.Contains('args')) { return $cmd }
    if ($cmd -match '^\s*"([^"]*gyeot-hook(?:\.exe)?)"') { return $Matches[1] }
    if ($cmd -match '^\s*(\S*gyeot-hook(?:\.exe)?)') { return $Matches[1] }
    return $cmd
}

# settings 나무에서 곁 훅 처리기를 모두 꺼낸다: @{ Event; Exe }. 부르는 쪽은 @( ) 로 받는다.
function Get-GyeotHookHandlerList($Settings) {
    $list = @()
    if (-not $Settings.Contains('hooks') -or $Settings['hooks'] -isnot [System.Collections.IDictionary]) { return }
    foreach ($evName in @($Settings['hooks'].Keys)) {
        foreach ($g in @($Settings['hooks'][$evName])) {
            if ($g -isnot [System.Collections.IDictionary] -or -not $g.Contains('hooks')) { continue }
            foreach ($h in @($g['hooks'])) {
                if (Test-GyeotHookHandler $h) { $list += [pscustomobject]@{ Event = [string]$evName; Exe = [string](Get-GyeotHookHandlerExe $h) } }
            }
        }
    }
    return $list
}
