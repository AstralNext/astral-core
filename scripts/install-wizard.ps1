#Requires -Version 5.1
<#
.SYNOPSIS
  astral-core 本地部署引导（分步安装 / 启停 / 更新 / 回滚 / 卸载）

.EXAMPLE
  .\scripts\install-wizard.ps1
  .\scripts\install-wizard.ps1 -Program .\target\release\astral-core.exe
#>
[CmdletBinding()]
param(
  [ValidateSet('menu', 'install', 'uninstall', 'start', 'stop', 'status', 'update', 'rollback', 'versions')]
  [string]$Action = 'menu',

  [string]$Name = 'default',
  [string]$Listen = '127.0.0.1:50051',
  [string]$DataDir = '',
  [string]$InstallRoot = '',
  [string]$Program = '',
  [string]$Version = '',
  [string]$Controller = '',
  [string]$ControllerToken = '',
  [string]$ControllerTlsCa = '',
  [string]$ControllerTlsDomain = '',
  [int]$Retain = 3,
  [switch]$NoStart,
  [switch]$NonInteractive,
  [string]$ElevatedLog = ''
)

$ErrorActionPreference = 'Stop'
$Script:WizardTitle = 'Astral Core 本地部署向导'
$Script:WizardWidth = 64

try {
  [Console]::OutputEncoding = [System.Text.Encoding]::UTF8
  $OutputEncoding = [System.Text.Encoding]::UTF8
  $Host.UI.RawUI.WindowTitle = $Script:WizardTitle
} catch {}

# ---------------- UI ----------------

function Write-Info([string]$m) { Write-Host "  · $m" -ForegroundColor Cyan }
function Write-Ok([string]$m) { Write-Host "  √ $m" -ForegroundColor Green }
function Write-Warn([string]$m) { Write-Host "  ! $m" -ForegroundColor Yellow }
function Write-Err([string]$m) { Write-Host "  × $m" -ForegroundColor Red }

function Write-LogLine([string]$m) {
  if ($ElevatedLog) { Add-Content -LiteralPath $ElevatedLog -Value $m -Encoding UTF8 }
}

function Out-Both([string]$m, [string]$level = 'info') {
  Write-LogLine $m
  switch ($level) {
    'ok' { Write-Ok $m }
    'warn' { Write-Warn $m }
    'err' { Write-Err $m }
    default { Write-Info $m }
  }
}

function Write-Rule([string]$ch = '-') {
  Write-Host ('  ' + ($ch * ($Script:WizardWidth - 4))) -ForegroundColor DarkGray
}

function Write-Banner {
  Clear-Host
  $w = $Script:WizardWidth
  $line = ('=' * $w)
  Write-Host ''
  Write-Host ('  ' + $line) -ForegroundColor DarkCyan
  Write-Host ('  ' + $Script:WizardTitle.PadLeft(([math]::Floor(($w + $Script:WizardTitle.Length) / 2))).PadRight($w)) -ForegroundColor White
  Write-Host '  把 astral-core 安装为系统服务，并管理启停 / 更新' -ForegroundColor DarkGray
  Write-Host ('  ' + $line) -ForegroundColor DarkCyan
  Write-Host ''
}

function Write-Step([int]$n, [int]$total, [string]$title, [string]$hint = '') {
  Write-Host ''
  Write-Host ("  ▸ 步骤 $n/$total  $title") -ForegroundColor Yellow
  if ($hint) { Write-Host ("    $hint") -ForegroundColor DarkGray }
  Write-Rule
}

function Write-Panel([string]$title, [hashtable]$rows) {
  Write-Host ''
  Write-Host ("  ┌─ $title ") -ForegroundColor White -NoNewline
  Write-Host (('─' * [Math]::Max(8, $Script:WizardWidth - 8 - $title.Length))) -ForegroundColor DarkGray
  foreach ($k in $rows.Keys) {
    $v = $rows[$k]
    if ([string]::IsNullOrWhiteSpace([string]$v)) { $v = '（默认）' }
    Write-Host ('  │ ' + $k.PadRight(12) + '  ') -ForegroundColor DarkGray -NoNewline
    Write-Host $v -ForegroundColor White
  }
  Write-Host ('  └' + ('─' * ($Script:WizardWidth - 4))) -ForegroundColor DarkGray
  Write-Host ''
}

function Write-SuccessBox([string]$title, [string[]]$lines) {
  Write-Host ''
  Write-Host '  ********************************************' -ForegroundColor Green
  Write-Host ("  *  $title") -ForegroundColor Green
  Write-Host '  ********************************************' -ForegroundColor Green
  foreach ($l in $lines) {
    Write-Host ("  *  $l") -ForegroundColor Green
  }
  Write-Host '  ********************************************' -ForegroundColor Green
  Write-Host ''
}

function Pause-IfInteractive([string]$msg = '按回车继续') {
  if ($NonInteractive -or $ElevatedLog) { return }
  Write-Host ''
  [void](Read-Host "  $msg")
}

# ---------------- helpers ----------------

function Test-IsAdmin {
  $id = [Security.Principal.WindowsIdentity]::GetCurrent()
  $p = New-Object Security.Principal.WindowsPrincipal($id)
  return $p.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Get-DefaultExe {
  $candidates = @(
    $Program,
    (Join-Path $PSScriptRoot '..\target\release\astral-core.exe'),
    (Join-Path $PSScriptRoot '..\target\debug\astral-core.exe'),
    (Join-Path (Get-Location) 'astral-core.exe'),
    (Join-Path $env:LOCALAPPDATA 'Astral\astral-core\bin\astral-core.exe'),
    (Join-Path $env:LOCALAPPDATA 'Astral\astral-core\app\current\astral-core.exe')
  ) | Where-Object { $_ -and $_.Trim() -ne '' }

  foreach ($c in $candidates) {
    try {
      $full = [System.IO.Path]::GetFullPath($c)
      if (Test-Path -LiteralPath $full) { return $full }
    } catch {}
  }
  return $null
}

function Test-InstanceName([string]$n) {
  if ([string]::IsNullOrWhiteSpace($n)) { return $false }
  if ($n.Length -gt 64) { return $false }
  return [bool]($n -match '^[A-Za-z0-9][A-Za-z0-9_-]*$')
}

function Test-ListenAddress([string]$addr) {
  if ([string]::IsNullOrWhiteSpace($addr)) { return $false }
  if ($addr -match '^[\s、，。；：！？""''（）【】]+$') { return $false }
  if ($addr -notmatch '^[^:\s]+:(\d+)$') { return $false }
  $port = [int]$Matches[1]
  return ($port -ge 1 -and $port -le 65535)
}

function Read-Default([string]$prompt, [string]$default) {
  if ($NonInteractive) { return $default }
  $suffix = if ($default) { " [$default]" } else { '' }
  $v = Read-Host "  $prompt$suffix"
  if ([string]::IsNullOrWhiteSpace($v)) { return $default }
  return $v.Trim()
}

function Read-YesNo([string]$prompt, [bool]$defaultYes = $true) {
  if ($NonInteractive) { return $defaultYes }
  $hint = if ($defaultYes) { 'Y/n' } else { 'y/N' }
  $v = Read-Host "  $prompt ($hint)"
  if ([string]::IsNullOrWhiteSpace($v)) { return $defaultYes }
  return $v -match '^(y|yes|Y|是)$'
}

function Read-InstanceName([string]$default) {
  if ($NonInteractive) {
    if (-not (Test-InstanceName $default)) { throw "无效实例名: $default" }
    return $default
  }
  while ($true) {
    $v = Read-Default '实例名（字母数字，可含 - _）' $default
    if (Test-InstanceName $v) { return $v }
    Write-Warn '实例名无效。须以字母/数字开头，仅含字母数字 - _，长度 1~64'
  }
}

function Read-ListenAddress([string]$default) {
  if ($NonInteractive) {
    if (-not (Test-ListenAddress $default)) { throw "无效监听地址: $default" }
    return $default
  }
  while ($true) {
    $v = Read-Default '监听地址 host:port' $default
    if ($v -match '^[\s、，。；：！？]+$') {
      Write-Warn "检测到误输入「$v」，已使用默认 $default"
      return $default
    }
    if (Test-ListenAddress $v) { return $v }
    Write-Warn "地址无效: $v  （示例: 127.0.0.1:50051）"
  }
}

function Get-ServiceStatusText([string]$Exe) {
  $output = & $Exe service status --name $Name 2>&1
  return ($output | Out-String).Trim()
}

function Invoke-Core {
  param([Parameter(Mandatory)][string]$Exe, [Parameter(Mandatory)][string[]]$ArgList)
  $line = "执行: $Exe $($ArgList -join ' ')"
  Out-Both $line
  $output = & $Exe @ArgList 2>&1
  foreach ($o in @($output)) {
    $s = "$o"
    Write-LogLine $s
    Write-Host "    $s"
  }
  if ($LASTEXITCODE -ne 0) {
    throw "astral-core 退出码 $LASTEXITCODE"
  }
}

function Invoke-ElevatedAction([string]$NextAction) {
  if (Test-IsAdmin) { return $false }

  $log = Join-Path $env:TEMP ("astral-wizard-{0}.log" -f [guid]::NewGuid().ToString('n'))
  if (Test-Path -LiteralPath $log) { Remove-Item -LiteralPath $log -Force }

  Write-Host ''
  Write-Warn '下一步需要管理员权限（Windows 服务）'
  Write-Info '将弹出 UAC，请点击「是」'
  Write-Info "提权日志: $log"
  Write-Host ''

  $argList = @(
    '-NoProfile', '-ExecutionPolicy', 'Bypass',
    '-File', $PSCommandPath,
    '-Action', $NextAction,
    '-Name', $Name,
    '-Listen', $Listen,
    '-Retain', "$Retain",
    '-NonInteractive',
    '-ElevatedLog', $log
  )
  if ($DataDir) { $argList += @('-DataDir', $DataDir) }
  if ($InstallRoot) { $argList += @('-InstallRoot', $InstallRoot) }
  if ($Program) { $argList += @('-Program', $Program) }
  if ($Version) { $argList += @('-Version', $Version) }
  if ($Controller) { $argList += @('-Controller', $Controller) }
  if ($ControllerToken) { $argList += @('-ControllerToken', $ControllerToken) }
  if ($ControllerTlsCa) { $argList += @('-ControllerTlsCa', $ControllerTlsCa) }
  if ($ControllerTlsDomain) { $argList += @('-ControllerTlsDomain', $ControllerTlsDomain) }
  if ($NoStart) { $argList += '-NoStart' }

  $p = Start-Process -FilePath 'powershell.exe' -Verb RunAs -ArgumentList $argList -Wait -PassThru

  Write-Host ''
  Write-Host '  -------- 管理员执行结果 --------' -ForegroundColor White
  if (Test-Path -LiteralPath $log) {
    Get-Content -LiteralPath $log -Encoding UTF8 | ForEach-Object { Write-Host ("  " + $_) }
  } else {
    Write-Warn '未生成提权日志（可能取消了 UAC）'
  }
  Write-Host '  --------------------------------' -ForegroundColor White

  if ($p.ExitCode -ne 0) {
    Write-Err "提权进程退出码: $($p.ExitCode)"
  } else {
    Write-Ok '提权操作已完成'
  }
  return $true
}

# ---------------- actions ----------------

function Invoke-Install([string]$Exe) {
  $total = 5

  if (-not $NonInteractive) {
    Write-Banner
    Write-Host '  本向导将：' -ForegroundColor White
    Write-Host '    1. 把 astral-core 复制到版本化安装目录' -ForegroundColor DarkGray
    Write-Host '    2. 注册 Windows 系统服务（需管理员）' -ForegroundColor DarkGray
    Write-Host '    3. 可选：配置出站连接公网控制端' -ForegroundColor DarkGray
    Write-Host '    4. 可选：安装后立即启动' -ForegroundColor DarkGray
    Write-Host ''
    if (-not (Read-YesNo '开始安装？' $true)) {
      Write-Warn '已取消'
      return
    }

    Write-Step 1 $total '基本信息' '实例名用于区分多节点；监听仅本机可访问请用 127.0.0.1'
    $script:Name = Read-InstanceName $Name
    $script:Listen = Read-ListenAddress $Listen

    Write-Step 2 $total '路径（可回车用默认）' '不确定就全部回车'
    $script:DataDir = Read-Default '数据目录' $DataDir
    $script:InstallRoot = Read-Default '安装根目录' $InstallRoot
    if (-not $Program) {
      $found = Get-DefaultExe
      $script:Program = Read-Default 'astral-core.exe 路径' $(if ($found) { $found } else { '' })
    }

    Write-Step 3 $total '出站控制端（可选）' '家里/内网机器连公网控制台时再开'
    if (Read-YesNo '配置出站控制端？' $false) {
      $script:Controller = Read-Default '控制端 URL' $(if ($Controller) { $Controller } else { 'http://127.0.0.1:8443' })
      $script:ControllerToken = Read-Default '共享密钥 token' $ControllerToken
      if ([string]::IsNullOrWhiteSpace($ControllerToken)) {
        throw '已启用控制端时必须填写 token'
      }
      if ($script:Controller -like 'https://*') {
        $script:ControllerTlsCa = Read-Default 'TLS CA/自签证书 PEM（可空）' $ControllerTlsCa
        $script:ControllerTlsDomain = Read-Default 'TLS 域名/SNI（可空）' $ControllerTlsDomain
      }
    } else {
      $script:Controller = ''
      $script:ControllerToken = ''
      $script:ControllerTlsCa = ''
      $script:ControllerTlsDomain = ''
    }

    Write-Step 4 $total '启动选项'
    $script:NoStart = -not (Read-YesNo '安装完成后立即启动服务？' (-not $NoStart))

    Write-Step 5 $total '确认安装'
    $rows = [ordered]@{
      '实例名'     = $Name
      '服务名'     = "dev.astral.core-$Name"
      '监听'       = $Listen
      '程序'       = $Program
      '数据目录'   = $(if ($DataDir) { $DataDir } else { '平台默认' })
      '安装根'     = $(if ($InstallRoot) { $InstallRoot } else { '平台默认' })
      '控制端'     = $(if ($Controller) { $Controller } else { '未配置' })
      '装后启动'   = $(if ($NoStart) { '否' } else { '是' })
    }
    Write-Panel '即将应用的配置' $rows
    if (-not (Read-YesNo '确认以上配置并继续？' $true)) {
      Write-Warn '已取消'
      return
    }
  }

  if (-not $Program -or -not (Test-Path -LiteralPath $Program)) {
    throw '找不到 astral-core.exe。请用 -Program 指定，或先 cargo build --release。'
  }

  if (Invoke-ElevatedAction -NextAction 'install') {
    # 父进程：提权结束后再查一次状态
    try {
      $st = Get-ServiceStatusText -Exe $Program
      Write-Info "当前状态: $st"
      if ($st -match 'running') {
        Write-SuccessBox '安装成功' @(
          "服务: dev.astral.core-$Name",
          "监听: $Listen",
          '令牌: 数据目录/bootstrap_token.txt',
          '下一步: GUI/SDK 用 Bearer 令牌连接'
        )
      }
    } catch {}
    return
  }

  # 已是管理员 / 提权子进程
  $argList = @(
    'service', 'install',
    '--name', $Name,
    '--listen', $Listen,
    '--program', $Program,
    '--retain', "$Retain"
  )
  if ($DataDir) { $argList += @('--data-dir', $DataDir) }
  if ($InstallRoot) { $argList += @('--install-root', $InstallRoot) }
  if ($Version) { $argList += @('--version', $Version) }
  if ($NoStart) { $argList += '--no-start' }
  if ($Controller) {
    $argList += @('--controller', $Controller, '--controller-token', $ControllerToken)
    if ($ControllerTlsCa) { $argList += @('--controller-tls-ca', $ControllerTlsCa) }
    if ($ControllerTlsDomain) { $argList += @('--controller-tls-domain', $ControllerTlsDomain) }
  }

  Invoke-Core -Exe $Program -ArgList $argList
  Out-Both "服务已安装: dev.astral.core-$Name" 'ok'
  Write-LogLine "监听: $Listen"
  Write-LogLine '令牌文件: 数据目录/bootstrap_token.txt（首次启动后生成）'
  if (-not $ElevatedLog) {
    Write-SuccessBox '安装成功' @(
      "服务: dev.astral.core-$Name",
      "监听: $Listen",
      '令牌: 数据目录/bootstrap_token.txt',
      '下一步: 菜单选 4 查看状态'
    )
  }
}

function Invoke-Simple([string]$Exe, [string]$Sub) {
  if ($Sub -in @('start', 'stop', 'uninstall')) {
    try {
      $st = Get-ServiceStatusText -Exe $Exe
      Write-Info "当前状态: $st"
      if ($st -match 'not-installed') {
        switch ($Sub) {
          'start' { Write-Warn "尚未安装。请先选「1) 安装为系统服务」。"; return }
          'stop' { Write-Warn '未安装，无需停止。'; return }
          'uninstall' { Write-Warn '未安装，无需卸载。'; return }
        }
      }
      if ($Sub -eq 'start' -and $st -match '(^|\s)running(\s|$)') {
        Write-Ok '服务已在运行。'; return
      }
      if ($Sub -eq 'stop' -and $st -match 'stopped') {
        Write-Ok '服务已停止。'; return
      }
    } catch {
      Write-Warn "预检查失败，将继续尝试: $($_.Exception.Message)"
    }
  }

  if ($Sub -eq 'uninstall' -and -not $NonInteractive -and -not $ElevatedLog) {
    Write-Warn "即将卸载服务 dev.astral.core-$Name"
    if (-not (Read-YesNo '确认卸载？' $false)) {
      Write-Warn '已取消'; return
    }
  }

  if ($Sub -in @('uninstall', 'start', 'stop')) {
    if (Invoke-ElevatedAction -NextAction $Sub) { return }
  }
  $argList = @('service', $Sub, '--name', $Name)
  Invoke-Core -Exe $Exe -ArgList $argList
  switch ($Sub) {
    'uninstall' { Out-Both '已卸载' 'ok' }
    'start' { Out-Both '已启动' 'ok' }
    'stop' { Out-Both '已停止' 'ok' }
  }
}

function Invoke-Update([string]$Exe) {
  if (-not $NonInteractive -and -not $ElevatedLog) {
    Write-Banner
    Write-Host '  产品级更新：新版本目录 → 切换 current → 重启服务' -ForegroundColor DarkGray
    Write-Host ''
    $script:Name = Read-InstanceName $Name
    if (-not $Program) { $script:Program = $Exe }
    $script:Program = Read-Default '新版本 astral-core.exe' $Program
    $script:Version = Read-Default '版本号（回车=从二进制推断）' $Version
    $script:Retain = [int](Read-Default '保留版本数' "$Retain")
    Write-Panel '更新摘要' ([ordered]@{
      '程序' = $Program
      '版本' = $(if ($Version) { $Version } else { '自动推断' })
      '保留' = "$Retain"
    })
    if (-not (Read-YesNo '确认更新？' $true)) { Write-Warn '已取消'; return }
  }
  if (Invoke-ElevatedAction -NextAction 'update') { return }
  $prog = if ($Program) { $Program } else { $Exe }
  $argList = @('service', 'update', '--program', $prog, '--retain', "$Retain")
  if ($Version) { $argList += @('--version', $Version) }
  if ($InstallRoot) { $argList += @('--install-root', $InstallRoot) }
  if ($NoStart) { $argList += '--no-start' }
  Invoke-Core -Exe $Exe -ArgList $argList
  Out-Both '更新完成' 'ok'
}

function Invoke-Rollback([string]$Exe) {
  if (-not $NonInteractive -and -not $ElevatedLog) {
    Write-Banner
    Write-Host '  回滚会切换 current 并重启已登记实例' -ForegroundColor DarkGray
    Write-Host ''
    Invoke-Core -Exe $Exe -ArgList @('service', 'versions')
    $script:Version = Read-Default '回滚到版本（回车=上一版）' $Version
    if (-not (Read-YesNo '确认回滚？' $true)) { Write-Warn '已取消'; return }
  }
  if (Invoke-ElevatedAction -NextAction 'rollback') { return }
  $argList = @('service', 'rollback')
  if ($Version) { $argList += @('--version', $Version) }
  if ($NoStart) { $argList += '--no-start' }
  Invoke-Core -Exe $Exe -ArgList $argList
  Out-Both '回滚完成' 'ok'
}

function Show-Menu {
  Write-Banner
  try {
    $st = Get-ServiceStatusText -Exe $script:Program
    Write-Host "  当前实例: $Name" -ForegroundColor White
    Write-Host "  服务状态: $st" -ForegroundColor $(if ($st -match 'running') { 'Green' } elseif ($st -match 'not-installed') { 'DarkGray' } else { 'Yellow' })
  } catch {
    Write-Host "  当前实例: $Name" -ForegroundColor White
    Write-Host '  服务状态: （无法读取）' -ForegroundColor DarkGray
  }
  Write-Host ''
  Write-Host '  安装与生命周期' -ForegroundColor White
  Write-Host '    1) 安装为系统服务          推荐首次使用'
  Write-Host '    2) 启动服务'
  Write-Host '    3) 停止服务'
  Write-Host '    4) 查看状态'
  Write-Host ''
  Write-Host '  版本管理' -ForegroundColor White
  Write-Host '    5) 更新到新版本'
  Write-Host '    6) 回滚到旧版本'
  Write-Host '    7) 列出已装版本'
  Write-Host ''
  Write-Host '  其他' -ForegroundColor White
  Write-Host '    8) 卸载服务'
  Write-Host '    9) 切换默认实例名'
  Write-Host '    0) 退出'
  Write-Host ''
  $c = Read-Host '  请选择'
  switch ($c) {
    '1' { return 'install' }
    '2' { return 'start' }
    '3' { return 'stop' }
    '4' { return 'status' }
    '5' { return 'update' }
    '6' { return 'rollback' }
    '7' { return 'versions' }
    '8' { return 'uninstall' }
    '9' { return 'rename' }
    default { return 'exit' }
  }
}

function Invoke-Action([string]$Act, [string]$Exe) {
  switch ($Act) {
    'install' { Invoke-Install -Exe $Exe }
    'uninstall' { Invoke-Simple -Exe $Exe -Sub 'uninstall' }
    'start' { Invoke-Simple -Exe $Exe -Sub 'start' }
    'stop' { Invoke-Simple -Exe $Exe -Sub 'stop' }
    'status' {
      $st = Get-ServiceStatusText -Exe $Exe
      Write-Panel '服务状态' ([ordered]@{
        '实例' = $Name
        '服务' = "dev.astral.core-$Name"
        '状态' = $st
        '程序' = $Exe
      })
      Out-Both $st
    }
    'update' { Invoke-Update -Exe $Exe }
    'rollback' { Invoke-Rollback -Exe $Exe }
    'versions' {
      Write-Host ''
      Write-Host '  已安装版本（* 为当前）：' -ForegroundColor White
      Invoke-Core -Exe $Exe -ArgList @('service', 'versions')
    }
    'rename' {
      $script:Name = Read-InstanceName $Name
      Write-Ok "默认实例已切换为: $Name"
    }
    default { Write-Err "未知操作: $Act"; return 1 }
  }
  return 0
}

# ---- main ----
try {
  if (-not $Program) { $script:Program = Get-DefaultExe }
  $exe = $Program
  if (-not $exe -or -not (Test-Path -LiteralPath $exe)) {
    Write-Err '未找到 astral-core.exe。请先 cargo build --release，或 -Program 指定路径。'
    exit 1
  }
  # 提权子进程少刷屏
  if (-not $ElevatedLog) { Out-Both "使用二进制: $exe" }

  if ($Action -ne 'menu') {
    $code = Invoke-Action -Act $Action -Exe $exe
    exit $code
  }

  while ($true) {
    $Action = Show-Menu
    if ($Action -eq 'exit') {
      Write-Host ''
      Write-Info '已退出向导'
      Write-Host ''
      break
    }
    if ($Action -in @('start', 'stop', 'status', 'uninstall') -and -not $NonInteractive) {
      Write-Host ''
      $script:Name = Read-InstanceName $Name
    }
    try {
      [void](Invoke-Action -Act $Action -Exe $exe)
    } catch {
      Write-Err $_.Exception.Message
      Write-LogLine ("错误: " + $_.Exception.Message)
    }
    Pause-IfInteractive '按回车返回主菜单'
  }
} catch {
  Write-Err $_.Exception.Message
  Write-LogLine ("错误: " + $_.Exception.Message)
  if (-not $ElevatedLog) { Pause-IfInteractive '按回车退出' }
  exit 1
}