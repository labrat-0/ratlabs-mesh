# Install RatLabs Mesh on Windows.
#
#   irm https://raw.githubusercontent.com/labrat-0/ratlabs-mesh/main/install.ps1 | iex
#
# Downloads the latest release, checks it against SHA256SUMS, installs ratmesh.exe to
# %LOCALAPPDATA%\Programs\RatLabs Mesh, adds it to your PATH and the Start menu, and runs
# `ratmesh doctor`. Nothing else is installed: a missing Blender is reported with the command to get it.
#
# $env:RATMESH_VERSION = "v0.2.0"   install that release instead of the latest
# $env:RATMESH_ARCHIVE = "path.zip" install from a local archive (offline / testing)
$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"   # Invoke-WebRequest is much faster without the progress bar

$Repo = "labrat-0/ratlabs-mesh"
$Name = "ratmesh-x86_64-pc-windows-msvc"   # also runs on Windows on Arm (x64 emulation)
$Dir = Join-Path $env:LOCALAPPDATA "Programs\RatLabs Mesh"
function Say($m) { Write-Host "==> $m" -ForegroundColor Green }

$Tmp = Join-Path ([IO.Path]::GetTempPath()) ("ratmesh-" + [guid]::NewGuid())
New-Item -ItemType Directory -Path $Tmp | Out-Null
try {
    $Zip = Join-Path $Tmp "$Name.zip"
    if ($env:RATMESH_ARCHIVE) {
        Say "Installing from $env:RATMESH_ARCHIVE"
        Copy-Item $env:RATMESH_ARCHIVE $Zip
    } else {
        $Base = if ($env:RATMESH_VERSION) { "https://github.com/$Repo/releases/download/$env:RATMESH_VERSION" } else { "https://github.com/$Repo/releases/latest/download" }
        Say "Downloading RatLabs Mesh for Windows"
        Invoke-WebRequest -UseBasicParsing "$Base/$Name.zip" -OutFile $Zip
        # Saved to a file and read as text: GitHub serves release assets as binary, so .Content would be bytes.
        $SumsFile = Join-Path $Tmp "SHA256SUMS"
        Invoke-WebRequest -UseBasicParsing "$Base/SHA256SUMS" -OutFile $SumsFile
        $Want = (Get-Content $SumsFile | Where-Object { $_ -match " $Name\.zip$" }) -replace " .*", ""
        $Got = (Get-FileHash $Zip -Algorithm SHA256).Hash.ToLower()
        if (-not $Want -or $Want.Trim() -ne $Got) { throw "checksum mismatch for $Name.zip" }
        Say "Checksum OK"
    }
    Expand-Archive $Zip -DestinationPath $Tmp -Force
    New-Item -ItemType Directory -Force -Path $Dir | Out-Null
    Copy-Item (Join-Path $Tmp "$Name\ratmesh.exe") (Join-Path $Dir "ratmesh.exe") -Force
} finally {
    Remove-Item -Recurse -Force $Tmp -ErrorAction SilentlyContinue
}
$Exe = Join-Path $Dir "ratmesh.exe"
Say "Installed $Exe ($(& $Exe --version))"

$UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
if (-not (($UserPath -split ";") -contains $Dir)) {
    [Environment]::SetEnvironmentVariable("Path", "$UserPath;$Dir".TrimStart(";"), "User")
    $env:Path += ";$Dir"
    Say "Added RatLabs Mesh to your PATH (new terminals can run: ratmesh)"
}

$Lnk = Join-Path ([Environment]::GetFolderPath("Programs")) "RatLabs Mesh.lnk"
$Shell = New-Object -ComObject WScript.Shell
$Sc = $Shell.CreateShortcut($Lnk)
$Sc.TargetPath = $Exe
$Sc.Arguments = "open"
$Sc.WorkingDirectory = $Dir
$Sc.Description = "Turn character art into finished 3D models"
$Sc.WindowStyle = 7   # minimized: the studio opens in your browser
$Sc.Save()
Say "Added RatLabs Mesh to the Start menu"

Write-Host ""
$Doctor = & $Exe doctor
$Doctor | Write-Host
Write-Host ""
if (-not ($Doctor -match "✓ Blender")) {
    Say "RatLabs Mesh needs Blender 4.2+:  winget install BlenderFoundation.Blender   (or blender.org/download)"
}
Say "Open the studio:  Start menu > RatLabs Mesh   (or: ratmesh open)"
Say "Connect your AI:  ratmesh connect"
