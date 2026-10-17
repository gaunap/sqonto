# Copyright (C) 2026 Johannes Asal
# SPDX-License-Identifier: AGPL-3.0-or-later
# See LICENSE in the project root for the full license and warranty disclaimer.

# Exercise the actual generated installer in CI, including preservation of user data.
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$installDir = Join-Path ([IO.Path]::GetTempPath()) ("sqonto-installer-test-" + [guid]::NewGuid())
$version = & node "$root/scripts/version.mjs"
if ($LASTEXITCODE -ne 0) { throw 'Version validation failed.' }
$installer = Join-Path $root "dist/packages/Sqonto-$version-windows-x64-setup.exe"
$server = $null
$oldPort = $env:SQONTO_PORT
$oldDataDir = $env:SQONTO_DATA_DIR
try {
    $arguments = @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', "/DIR=`"$installDir`"")
    $setup = Start-Process -FilePath $installer -ArgumentList $arguments -WindowStyle Hidden -PassThru -Wait
    if ($setup.ExitCode -ne 0) { throw "Installation failed: $($setup.ExitCode)" }
    foreach ($name in 'Sqonto.exe', 'sqonto-server.exe', 'LICENSE.txt', 'THIRD-PARTY-LICENSES.txt') {
        if (!(Test-Path "$installDir/$name")) { throw "Installer omitted $name" }
    }
    $env:SQONTO_DATA_DIR = $null
    $env:SQONTO_PORT = '18787'
    $server = Start-Process -FilePath "$installDir/sqonto-server.exe" -WindowStyle Hidden -PassThru
    $ready = $false
    for ($attempt = 0; $attempt -lt 60; $attempt++) {
        if ($server.HasExited) { throw 'Installed server exited before becoming ready.' }
        try {
            $status = Invoke-RestMethod 'http://127.0.0.1:18787/api/system/status' -TimeoutSec 2
            if ([IO.Path]::GetFullPath($status.data_dir) -ne [IO.Path]::GetFullPath("$installDir/data")) {
                throw 'Server reported an unexpected data directory.'
            }
            $page = Invoke-WebRequest 'http://127.0.0.1:18787/' -TimeoutSec 2
            if ($page.Content -notmatch '<html') { throw 'Embedded frontend missing.' }
            $ready = $true
            break
        } catch { Start-Sleep -Seconds 1 }
    }
    if (!$ready) { throw 'Installed server did not serve the API and frontend.' }
    Stop-Process -Id $server.Id
    $server.WaitForExit()
    $server = $null
    $marker = Join-Path $installDir 'data/preserve.txt'
    Set-Content -LiteralPath $marker -Value 'accounting data must survive'
    $setup = Start-Process -FilePath $installer -ArgumentList $arguments -WindowStyle Hidden -PassThru -Wait
    if ($setup.ExitCode -ne 0) { throw 'Upgrade failed.' }
    if (!(Test-Path $marker)) { throw 'Upgrade removed user data.' }
    # Self-update: the setup started by Sqonto starts the updated program again.
    $setup = Start-Process -FilePath $installer -ArgumentList ($arguments + '/RELAUNCH=server') -WindowStyle Hidden -PassThru -Wait
    if ($setup.ExitCode -ne 0) { throw 'Update with relaunch failed.' }
    $relaunched = $false
    for ($attempt = 0; $attempt -lt 60 -and !$relaunched; $attempt++) {
        try {
            $status = Invoke-RestMethod 'http://127.0.0.1:18787/api/system/status' -TimeoutSec 2
            $relaunched = [IO.Path]::GetFullPath($status.data_dir) -eq [IO.Path]::GetFullPath("$installDir/data")
        } catch { Start-Sleep -Seconds 1 }
    }
    # Found by its port: the temporary path may be given in its short 8.3 form.
    $owner = (Get-NetTCPConnection -LocalPort 18787 -State Listen -ErrorAction SilentlyContinue).OwningProcess | Select-Object -First 1
    if ($owner) {
        Stop-Process -Id $owner
        Wait-Process -Id $owner -ErrorAction SilentlyContinue
    }
    if (!$relaunched) { throw 'The setup did not start the server again after /RELAUNCH=server.' }
    $uninstall = Start-Process -FilePath "$installDir/unins000.exe" -ArgumentList '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART' -WindowStyle Hidden -PassThru -Wait
    if ($uninstall.ExitCode -ne 0) { throw 'Uninstall failed.' }
    if (Test-Path "$installDir/Sqonto.exe") { throw 'Uninstall left the application installed.' }
    if (!(Test-Path $marker) -or !(Test-Path "$installDir/data/sqonto.db")) {
        throw 'Uninstall removed user data.'
    }
    Write-Host 'Installer, installed server, upgrade, relaunch and data preservation passed.'
} finally {
    if ($server -and !$server.HasExited) { Stop-Process -Id $server.Id }
    $env:SQONTO_PORT = $oldPort
    $env:SQONTO_DATA_DIR = $oldDataDir
    # Retain the temporary test data for inspection; never delete accounting data recursively.
}
