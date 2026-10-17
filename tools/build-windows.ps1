# Compile Backlog Panda en un .exe autonome (Windows).
# Lance via build-windows.bat (double-clic) ou : powershell -ExecutionPolicy Bypass -File tools\build-windows.ps1
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $root

function Step($text) { Write-Host ""; Write-Host "== $text" -ForegroundColor Cyan }
function Ok($text)   { Write-Host "   OK  $text" -ForegroundColor Green }
function Warn($text) { Write-Host "   !   $text" -ForegroundColor Yellow }
function Fail($text) { Write-Host ""; Write-Host "ECHEC : $text" -ForegroundColor Red; exit 1 }

# 1. Rust ---------------------------------------------------------------------------------------
Step "Rust"
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    $bin = Join-Path $env:USERPROFILE '.cargo\bin'
    if (Test-Path (Join-Path $bin 'cargo.exe')) { $env:PATH = "$bin;$env:PATH" }
}
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    Fail "Rust est introuvable. Installe-le (https://rustup.rs) ou lance : winget install Rustlang.Rustup"
}
$rustVersion = (& rustc --version)
$host_line = (& rustc -vV) | Where-Object { $_ -like 'host:*' }
Ok $rustVersion
if ($host_line -notlike '*windows-msvc*') {
    Fail "La chaine Rust installee n'est pas celle de Microsoft ($host_line). Lance : rustup default stable-x86_64-pc-windows-msvc"
}

# 2. Outils de compilation C++ de Visual Studio --------------------------------------------------
Step "Outils de compilation C++ (Visual Studio Build Tools)"
$pf86 = ${env:ProgramFiles(x86)}
$vswhere = Join-Path $pf86 'Microsoft Visual Studio\Installer\vswhere.exe'
$vs = $null
if (Test-Path $vswhere) {
    $vs = (& $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath) | Select-Object -First 1
}
if (-not $vs) {
    Write-Host ""
    Write-Host "Les outils C++ de Visual Studio manquent (c'est la cause de l'erreur LNK1104 / msvcrt.lib)." -ForegroundColor Yellow
    Write-Host "Installe-les une fois (environ 3 Go), puis relance ce script :" -ForegroundColor Yellow
    Write-Host '  winget install Microsoft.VisualStudio.2022.BuildTools --override "--passive --wait --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"' -ForegroundColor White
    Write-Host "Ou : https://visualstudio.microsoft.com/fr/downloads/ -> Outils de generation pour Visual Studio -> charge de travail 'Developpement Desktop en C++'." -ForegroundColor White
    Fail "Outils C++ introuvables."
}
$vcvars = Join-Path $vs 'VC\Auxiliary\Build\vcvars64.bat'
if (-not (Test-Path $vcvars)) { Fail "vcvars64.bat introuvable dans $vs : reinstalle la charge de travail 'Developpement Desktop en C++'." }
Ok $vs

# Charge l'environnement de compilation (LIB, INCLUDE, PATH) dans cette fenetre : evite les erreurs d'edition de liens.
& cmd /c "`"$vcvars`" >nul 2>&1 && set" | ForEach-Object {
    $i = $_.IndexOf('=')
    if ($i -gt 0) { [Environment]::SetEnvironmentVariable($_.Substring(0, $i), $_.Substring($i + 1), 'Process') }
}
$msvcrt = (Get-ChildItem -Path (Join-Path $vs 'VC\Tools\MSVC') -Recurse -Filter msvcrt.lib -ErrorAction SilentlyContinue | Select-Object -First 1)
if (-not $msvcrt) { Fail "msvcrt.lib est introuvable dans les outils C++ : reinstalle la charge de travail 'Developpement Desktop en C++' (composant MSVC x64/x86)." }
Ok "environnement de compilation charge"

# 3. WebView2 (le moteur d'affichage de la fenetre) -----------------------------------------------
Step "WebView2"
$wv = '{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}'
$keys = @("HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\$wv", "HKLM:\SOFTWARE\Microsoft\EdgeUpdate\Clients\$wv", "HKCU:\SOFTWARE\Microsoft\EdgeUpdate\Clients\$wv")
if ($keys | Where-Object { Test-Path $_ }) { Ok "WebView2 est installe" }
else { Warn "WebView2 n'a pas ete detecte. Il est fourni avec Windows 11 et les Windows 10 a jour ; sinon : https://developer.microsoft.com/microsoft-edge/webview2/ (Evergreen Bootstrapper)." }

# 4. Compilation ---------------------------------------------------------------------------------
Step "Compilation (5 a 15 minutes la premiere fois, quelques secondes ensuite)"
Push-Location (Join-Path $root 'src-tauri')
try {
    & cargo build --release
    if ($LASTEXITCODE -ne 0) { Fail "La compilation a echoue (voir les messages ci-dessus). Envoie-moi les dernieres lignes rouges." }
} finally { Pop-Location }

# 5. Resultat ------------------------------------------------------------------------------------
Step "Resultat"
$built = Join-Path $root 'src-tauri\target\release\backlog-panda-app.exe'
if (-not (Test-Path $built)) { Fail "L'exe n'a pas ete produit ($built)." }
$dist = Join-Path $root 'dist'
New-Item -ItemType Directory -Path $dist -Force | Out-Null
$exe = Join-Path $dist 'Backlog Panda.exe'
Copy-Item $built $exe -Force
$mb = [math]::Round((Get-Item $exe).Length / 1MB, 1)
Ok "$exe ($mb Mo)"
Write-Host ""
Write-Host "Termine. Double-clique sur 'Backlog Panda.exe' dans le dossier dist." -ForegroundColor Green
Write-Host "Au premier lancement, Windows SmartScreen peut afficher 'Windows a protege votre PC' : clique sur 'Informations complementaires' puis 'Executer quand meme' (l'exe n'est pas signe)." -ForegroundColor Gray
Start-Process explorer.exe $dist
