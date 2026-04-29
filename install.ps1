$ErrorActionPreference = "Stop"

$Repo = if ($env:CORTEX_REPO) { $env:CORTEX_REPO } else { "tky0065/cortex" }
$InstallDir = if ($env:CORTEX_INSTALL_DIR) { $env:CORTEX_INSTALL_DIR } else { Join-Path $HOME ".cortex\bin" }
$Version = if ($env:CORTEX_VERSION) { $env:CORTEX_VERSION } else { "latest" }
$Target = "x86_64-pc-windows-msvc"

if ($PSVersionTable.PSEdition -eq "Desktop") {
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
}

if (-not [Environment]::Is64BitOperatingSystem) {
    throw "cortex currently supports 64-bit Windows only"
}

if ($Version -eq "latest") {
    $Release = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest"
    if (-not $Release.tag_name) {
        throw "Could not resolve latest Cortex release"
    }
    $Version = $Release.tag_name
}

$Archive = "cortex-$Version-$Target.zip"
$BaseUrl = "https://github.com/$Repo/releases/download/$Version"
$TempDir = Join-Path ([System.IO.Path]::GetTempPath()) ("cortex-install-" + [System.Guid]::NewGuid())

New-Item -ItemType Directory -Path $TempDir | Out-Null

try {
    $ArchivePath = Join-Path $TempDir $Archive
    $SumsPath = Join-Path $TempDir "SHA256SUMS"

    Write-Host "Installing cortex $Version for $Target..."
    Invoke-WebRequest -Uri "$BaseUrl/$Archive" -OutFile $ArchivePath
    Invoke-WebRequest -Uri "$BaseUrl/SHA256SUMS" -OutFile $SumsPath

    $ExpectedLine = Get-Content $SumsPath | Where-Object { $_ -match "\s$([regex]::Escape($Archive))$" } | Select-Object -First 1
    if (-not $ExpectedLine) {
        throw "Checksum entry not found for $Archive"
    }

    $Expected = ($ExpectedLine -split "\s+")[0].ToLowerInvariant()
    $Actual = (Get-FileHash -Algorithm SHA256 $ArchivePath).Hash.ToLowerInvariant()
    if ($Actual -ne $Expected) {
        throw "Checksum verification failed for $Archive"
    }

    Expand-Archive -Path $ArchivePath -DestinationPath $TempDir -Force
    New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
    Copy-Item -Force (Join-Path $TempDir "cortex.exe") (Join-Path $InstallDir "cortex.exe")

    $UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
    $PathParts = @()
    if ($UserPath) {
        $PathParts = $UserPath -split ";"
    }

    if ($PathParts -notcontains $InstallDir) {
        $NewPath = if ($UserPath) { "$UserPath;$InstallDir" } else { $InstallDir }
        [Environment]::SetEnvironmentVariable("Path", $NewPath, "User")
        Write-Host "Added $InstallDir to your user PATH. Open a new terminal before running cortex."
    }

    Write-Host "cortex installed to $(Join-Path $InstallDir "cortex.exe")"
    Write-Host "Run: cortex --version"
}
finally {
    Remove-Item -Recurse -Force $TempDir -ErrorAction SilentlyContinue
}
