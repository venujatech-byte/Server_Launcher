$ErrorActionPreference = 'Stop'
$version = $env:RELEASE_VERSION
if ($version -notmatch '^\d+\.\d+\.\d+$' -or
    (($version -split '\.') | Where-Object { [int]$_ -gt 65535 })) {
    throw "Invalid MSIX version: $version"
}
$identity = $env:STORE_IDENTITY
$publisher = $env:STORE_PUBLISHER
$publisherName = $env:STORE_PUBLISHER_NAME
if (!$identity -or !$publisher -or !$publisherName) {
    Write-Warning 'Using local package identity. Set the three WINDOWS_STORE variables to the Partner Center values before submitting.'
    $identity = 'Venujatech.ServerLauncher'
    $publisher = 'CN=Venujatech'
    $publisherName = 'Venujatech'
}
$publisher = [Security.SecurityElement]::Escape($publisher)
$identity = [Security.SecurityElement]::Escape($identity)
$publisherName = [Security.SecurityElement]::Escape($publisherName)
$stage = Join-Path $env:RUNNER_TEMP 'server-launcher-msix'
$zip = Get-ChildItem windows-packages -Filter 'server_launcher-windows-x86_64.zip' -Recurse | Select-Object -First 1
if (!$zip) { throw 'Windows portable binary artifact is missing' }
Expand-Archive $zip.FullName -DestinationPath $stage
$portableExe = Get-ChildItem $stage -Filter '*.exe' -File | Select-Object -First 1
if (!$portableExe) { throw 'Windows portable executable is missing' }
Rename-Item $portableExe.FullName 'server_launcher_windows.exe'
$logos = New-Item -ItemType Directory -Path (Join-Path $stage 'Assets')
Add-Type -AssemblyName System.Drawing
$icon = [Drawing.Image]::FromFile((Resolve-Path 'assets/icon.png').Path)
try {
    foreach ($size in @(44, 50, 150)) {
        $bitmap = [Drawing.Bitmap]::new($icon, $size, $size)
        try { $bitmap.Save((Join-Path $logos.FullName "Logo$size.png"), [Drawing.Imaging.ImageFormat]::Png) }
        finally { $bitmap.Dispose() }
    }
} finally { $icon.Dispose() }
@"
<?xml version="1.0" encoding="utf-8"?>
<Package xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10"
         xmlns:uap="http://schemas.microsoft.com/appx/manifest/uap/windows10"
         xmlns:rescap="http://schemas.microsoft.com/appx/manifest/foundation/windows10/restrictedcapabilities"
         IgnorableNamespaces="uap rescap">
  <Identity Name="$identity" Publisher="$publisher" Version="$version.0" ProcessorArchitecture="x64" />
  <Properties>
    <DisplayName>Server Launcher</DisplayName>
    <PublisherDisplayName>$publisherName</PublisherDisplayName>
    <Logo>Assets\Logo50.png</Logo>
  </Properties>
  <Dependencies>
    <TargetDeviceFamily Name="Windows.Desktop" MinVersion="10.0.19041.0" MaxVersionTested="10.0.26100.0" />
  </Dependencies>
  <Resources><Resource Language="en-us" /></Resources>
  <Applications>
    <Application Id="ServerLauncher" Executable="server_launcher_windows.exe" EntryPoint="Windows.FullTrustApplication">
      <uap:VisualElements DisplayName="Server Launcher" Description="Manage local and remote servers"
          BackgroundColor="transparent" Square150x150Logo="Assets\Logo150.png" Square44x44Logo="Assets\Logo44.png" />
    </Application>
  </Applications>
  <Capabilities><rescap:Capability Name="runFullTrust" /></Capabilities>
</Package>
"@ | Set-Content (Join-Path $stage 'AppxManifest.xml') -Encoding utf8
$makeappx = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\makeappx.exe" |
    Sort-Object FullName -Descending | Select-Object -First 1
if (!$makeappx) { throw 'Windows SDK MakeAppx not found' }
New-Item -ItemType Directory -Path windows-store | Out-Null
& $makeappx.FullName pack /d $stage /p "windows-store/server-launcher-$version-x64.msix" /o
if ($LASTEXITCODE -ne 0) { throw "MakeAppx failed with exit code $LASTEXITCODE" }

# Verify what MakeAppx wrote, since only the resulting archive is shipped.
$unpacked = Join-Path $env:RUNNER_TEMP 'server-launcher-msix-verified'
& $makeappx.FullName unpack /p "windows-store/server-launcher-$version-x64.msix" /d $unpacked /o
if ($LASTEXITCODE -ne 0) { throw "MakeAppx unpack failed with exit code $LASTEXITCODE" }
[xml]$packedManifest = Get-Content (Join-Path $unpacked 'AppxManifest.xml') -Raw
if ($packedManifest.Package.Identity.Version -ne "$version.0" -or
    $packedManifest.Package.Identity.ProcessorArchitecture -ne 'x64') {
    throw 'MSIX identity version or architecture does not match this release'
}
foreach ($file in @('server_launcher_windows.exe', 'Assets/Logo44.png', 'Assets/Logo50.png', 'Assets/Logo150.png')) {
    $source = (Get-FileHash (Join-Path $stage $file) -Algorithm SHA256).Hash
    $packed = (Get-FileHash (Join-Path $unpacked $file) -Algorithm SHA256).Hash
    if ($source -ne $packed) { throw "MSIX payload changed: $file" }
}
