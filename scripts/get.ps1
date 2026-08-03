# astral-core Windows 一键安装
#
# 最短命令:
#   irm https://raw.githubusercontent.com/AstralNext/astral-core/main/scripts/get.ps1 | iex
#
# 仅下载不装服务:
#   iex "& { $(irm .../get.ps1) } -NoService"
#
# 带控制端:
#   iex "& { $(irm .../get.ps1) } -Controller http://x:8443 -Token secret"
[CmdletBinding()]
param(
  [string]$Version = $(if ($env:ASTRAL_VERSION) { $env:ASTRAL_VERSION } else { 'latest' }),
  [string]$Name = $(if ($env:ASTRAL_NAME) { $env:ASTRAL_NAME } else { 'default' }),
  [string]$Listen = $(if ($env:ASTRAL_LISTEN) { $env:ASTRAL_LISTEN } else { '127.0.0.1:50051' }),
  [string]$Controller = $(if ($env:ASTRAL_CONTROLLER) { $env:ASTRAL_CONTROLLER } else { '' }),
  [string]$Token = $(if ($env:ASTRAL_CONTROLLER_TOKEN) { $env:ASTRAL_CONTROLLER_TOKEN } else { '' }),
  [string]$TlsCa = $(if ($env:ASTRAL_CONTROLLER_TLS_CA) { $env:ASTRAL_CONTROLLER_TLS_CA } else { '' }),
  [string]$TlsDomain = $(if ($env:ASTRAL_CONTROLLER_TLS_DOMAIN) { $env:ASTRAL_CONTROLLER_TLS_DOMAIN } else { '' }),
  [string]$Repo = $(if ($env:ASTRAL_REPO) { $env:ASTRAL_REPO } else { 'AstralNext/astral-core' }),
  [string]$Prefix = $(if ($env:ASTRAL_PREFIX) { $env:ASTRAL_PREFIX } else { '' }),
  [switch]$NoService,
  [switch]$Service
)

$ErrorActionPreference = 'Stop'

function Write-Info([string]$m) { Write-Host "[*] $m" -ForegroundColor Cyan }
function Write-Ok([string]$m) { Write-Host "[+] $m" -ForegroundColor Green }
function Write-Warn([string]$m) { Write-Host "[!] $m" -ForegroundColor Yellow }

function Test-IsAdmin {
  $id = [Security.Principal.WindowsIdentity]::GetCurrent()
  $p = New-Object Security.Principal.WindowsPrincipal($id)
  return $p.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

$wantService = $true
if ($NoService) { $wantService = $false }
if ($Service) { $wantService = $true }
if ($env:ASTRAL_SERVICE -eq '0') { $wantService = $false }
if ($env:ASTRAL_SERVICE -eq '1') { $wantService = $true }

$arch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString().ToLowerInvariant()
switch -Regex ($arch) {
  'x64|amd64' { $assetArch = 'x86_64'; $wintun = 'wintun-windows-x86_64.dll' }
  'arm64' { $assetArch = 'aarch64'; $wintun = 'wintun-windows-aarch64.dll' }
  default { throw "不支持的架构: $arch" }
}
$asset = "astral-core-windows-$assetArch.exe"

if (-not $Prefix) {
  $Prefix = Join-Path $env:LOCALAPPDATA 'Astral\astral-core'
}
$binDir = Join-Path $Prefix 'bin'
New-Item -ItemType Directory -Force -Path $binDir | Out-Null
$exePath = Join-Path $binDir 'astral-core.exe'

if ($Version -eq 'latest') {
  $base = "https://github.com/$Repo/releases/latest/download"
} else {
  if ($Version -notmatch '^v') { $Version = "v$Version" }
  $base = "https://github.com/$Repo/releases/download/$Version"
}

Write-Info "正在下载 $asset ($Version)"
$tmp = Join-Path $env:TEMP ("astral-core-get-" + [guid]::NewGuid().ToString('n'))
New-Item -ItemType Directory -Force -Path $tmp | Out-Null
try {
  $ProgressPreference = 'SilentlyContinue'
  Invoke-WebRequest -Uri "$base/$asset" -OutFile (Join-Path $tmp 'astral-core.exe') -UseBasicParsing
  try {
    Invoke-WebRequest -Uri "$base/$wintun" -OutFile (Join-Path $tmp 'wintun.dll') -UseBasicParsing
  } catch {
    Write-Warn "wintun 下载跳过: $_"
  }
  Copy-Item (Join-Path $tmp 'astral-core.exe') $exePath -Force
  if (Test-Path (Join-Path $tmp 'wintun.dll')) {
    Copy-Item (Join-Path $tmp 'wintun.dll') (Join-Path $binDir 'wintun.dll') -Force
  }
} finally {
  Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}
Write-Ok "已安装 $exePath"

$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
if ($userPath -notlike "*${binDir}*") {
  [Environment]::SetEnvironmentVariable('Path', ($userPath.TrimEnd(';') + ';' + $binDir), 'User')
  $env:Path = "$binDir;$env:Path"
  Write-Info "已加入用户 PATH: $binDir"
}

if (-not $wantService) {
  Write-Ok "完成（仅二进制）。运行: astral-core --listen $Listen"
  return
}

if (-not (Test-IsAdmin)) {
  Write-Warn '注册 Windows 服务需要管理员权限，即将请求 UAC…'
  $arg = @(
    '-NoProfile', '-ExecutionPolicy', 'Bypass', '-Command',
    ("& '{0}' -Version '{1}' -Name '{2}' -Listen '{3}' -Controller '{4}' -Token '{5}' -TlsCa '{6}' -TlsDomain '{7}' -Repo '{8}' -Prefix '{9}' -Service" -f
      $PSCommandPath, $Version, $Name, $Listen, $Controller, $Token, $TlsCa, $TlsDomain, $Repo, $Prefix)
  )
  # When piped via iex, $PSCommandPath is empty — write temp script
  if (-not $PSCommandPath -or -not (Test-Path -LiteralPath $PSCommandPath)) {
    $scriptFile = Join-Path $env:TEMP 'astral-core-get-elevated.ps1'
    # Re-正在下载 script content is hard; instead elevate a direct service install command
    $svcArgs = @(
      'service', 'install',
      '--name', $Name,
      '--listen', $Listen,
      '--program', $exePath
    )
    if ($Controller) {
      if (-not $Token) { throw '启用控制端时必须提供 -Token' }
      $svcArgs += @('--controller', $Controller, '--controller-token', $Token)
      if ($TlsCa) { $svcArgs += @('--controller-tls-ca', $TlsCa) }
      if ($TlsDomain) { $svcArgs += @('--controller-tls-domain', $TlsDomain) }
    }
    $escaped = ($svcArgs | ForEach-Object { "'$($_ -replace "'","''")'" }) -join ' '
    $cmd = "& '$($exePath -replace "'","''")' $escaped"
    @"
`$ErrorActionPreference='Stop'
$cmd
if (`$LASTEXITCODE -ne 0) { exit `$LASTEXITCODE }
`$roots = @(
  (Join-Path `$env:LOCALAPPDATA 'Astral\astral-core\app'),
  (Join-Path `$env:LOCALAPPDATA 'Astral\astral-core\data\app')
)
`$wsrc = '$((Join-Path $binDir 'wintun.dll') -replace "'","''")'
foreach (`$root in `$roots) {
  `$cur = Join-Path `$root 'current'
  if ((Test-Path `$wsrc) -and (Test-Path `$cur)) {
    Copy-Item `$wsrc (Join-Path `$cur 'wintun.dll') -Force -ErrorAction SilentlyContinue
  }
}
Write-Host '[+] service installed'
"@ | Set-Content -Path $scriptFile -Encoding UTF8
    $p = Start-Process powershell.exe -Verb RunAs -ArgumentList @('-NoProfile','-ExecutionPolicy','Bypass','-File',$scriptFile) -Wait -PassThru
    if ($p.ExitCode -ne 0) { throw "提权安装失败: $($p.ExitCode)" }
    Write-Ok "服务已安装: dev.astral.core-$Name"
    Write-Host "监听: $Listen"
    Write-Host "令牌: 数据目录 bootstrap_token.txt（首次启动后生成）"
    Write-Host "状态: astral-core service status --name $Name"
    return
  }
  $p = Start-Process powershell.exe -Verb RunAs -ArgumentList $arg -Wait -PassThru
  exit $p.ExitCode
}

$svcArgs = @(
  'service', 'install',
  '--name', $Name,
  '--listen', $Listen,
  '--program', $exePath
)
if ($Controller) {
  if (-not $Token) { throw '启用控制端时必须提供 -Token / ASTRAL_CONTROLLER_TOKEN' }
  $svcArgs += @('--controller', $Controller, '--controller-token', $Token)
  if ($TlsCa) { $svcArgs += @('--controller-tls-ca', $TlsCa) }
  if ($TlsDomain) { $svcArgs += @('--controller-tls-domain', $TlsDomain) }
}

Write-Info "正在安装 Windows 服务"
& $exePath @svcArgs
if ($LASTEXITCODE -ne 0) { throw "服务安装退出码 $LASTEXITCODE" }

# Best-effort: put wintun beside current junction
$roots = @(
  (Join-Path $env:LOCALAPPDATA 'Astral\astral-core\data\app'),
  (Join-Path $env:LOCALAPPDATA 'Astral\astral-core\app')
)
$wsrc = Join-Path $binDir 'wintun.dll'
foreach ($root in $roots) {
  $cur = Join-Path $root 'current'
  if ((Test-Path $wsrc) -and (Test-Path $cur)) {
    Copy-Item $wsrc (Join-Path $cur 'wintun.dll') -Force -ErrorAction SilentlyContinue
  }
}

Write-Ok "服务已安装: dev.astral.core-$Name"
Write-Host "监听: $Listen"
Write-Host "令牌: 数据目录 bootstrap_token.txt（首次启动后生成）"
Write-Host "状态: astral-core service status --name $Name"