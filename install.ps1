$ErrorActionPreference = "Stop"

$repository = "COPPSARY/subshell"
$windowsArchitecture = if ($env:PROCESSOR_ARCHITEW6432) { $env:PROCESSOR_ARCHITEW6432 } else { $env:PROCESSOR_ARCHITECTURE }

if ($env:OS -ne "Windows_NT") {
  throw "SubShell's Windows installer only runs on Windows."
}
if ($windowsArchitecture -ne "AMD64") {
  throw "SubShell releases currently support Windows x64 only; detected $windowsArchitecture."
}

[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
$headers = @{
  Accept = "application/vnd.github+json"
  "X-GitHub-Api-Version" = "2022-11-28"
}

Write-Host "Finding the latest SubShell release..."
$release = Invoke-RestMethod -Headers $headers -Uri "https://api.github.com/repos/$repository/releases/latest"
$installers = @($release.assets | Where-Object { $_.name -match "(?i)-setup\.exe$" })
if ($installers.Count -ne 1) {
  throw "The latest SubShell release does not contain exactly one Windows x64 .exe installer."
}

$assetUrl = [string]$installers[0].browser_download_url
if (-not $assetUrl.StartsWith("https://github.com/$repository/releases/download/", [StringComparison]::OrdinalIgnoreCase)) {
  throw "GitHub returned an unexpected installer URL."
}

$temporaryFile = Join-Path ([IO.Path]::GetTempPath()) "SubShell-$([Guid]::NewGuid()).exe"
try {
  Write-Host "Downloading the latest SubShell installer..."
  Invoke-WebRequest -Headers $headers -Uri $assetUrl -OutFile $temporaryFile -UseBasicParsing
  $process = Start-Process -FilePath $temporaryFile -Wait -PassThru
  if ($process.ExitCode -ne 0) {
    throw "The SubShell installer exited with code $($process.ExitCode)."
  }
} finally {
  Remove-Item -LiteralPath $temporaryFile -Force -ErrorAction SilentlyContinue
}
