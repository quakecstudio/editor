# Releasing

How QuakeC Studio is packaged, signed, checked and released (P4-18,
[#129](https://github.com/quakecstudio/editor/issues/129)), and the signing it uses (P0-23,
[#23](https://github.com/quakecstudio/editor/issues/23)). Two workflows do it: `.github/workflows/release.yml`, started by a tag or by hand, calls the
reusable `.github/workflows/reusable-build.yml`, which builds, signs and attests every package.
The attestations are made in the reusable workflow so that each names it as the signer,
whatever calls it. On a tag, the Windows executables are signed with Azure Artifact Signing
(formerly Trusted Signing); a run by hand leaves them unsigned, since Azure trusts only tag runs.

## Making a release

1. Set `version` in the root `Cargo.toml` to the release's, `YYYY.MM.N`: the year, the month,
   and the month's release counted from 1, such as `2026.11.1` (`docs/conventions.md`), and
   merge it.
2. Push a tag naming it: `v2026.11.1`, or `v2026.11.1-rc1` for a pre-release. The workflow
   fails a tag that does not name the workspace's version.
3. The **release** workflow builds, signs, checks and publishes. A tag with a `-` is published as
   a pre-release; any other tag too, as a release.

A pull request that changes `packaging/` or either release workflow runs a dry run, and so does
running **release** from the Actions tab (or `gh workflow run release.yml --ref <branch>`):
it does everything but publish, and keeps the packages and `SHA256SUMS` as the run's `release`
artifact.

## The packages

| Platform | Package | Made by |
| --- | --- | --- |
| macOS, Apple silicon (arm64) | `quakec-studio-<version>-macos-arm64.pkg`: an installer package, signed with the Developer ID Installer identity, notarized and stapled, that puts `QuakeC Studio.app` in `/Applications`, replacing whatever version is there, and whose `postinstall` script links to its `quakec-ide` and `quakec-lsp` in `/usr/local/bin`; each executable and the app inside signed with the hardened runtime, the app notarized and stapled | `packaging/macos/package.sh`, with `packaging/macos/Info.plist`, `components.plist` and `scripts/postinstall` (for `pkgbuild`) and `distribution.xml` (for `productbuild`) |
| Windows x64 and arm64 | `quakec-studio-<version>-windows-<arch>.msi`, installing for every user under Program Files with a Start menu shortcut and the folder on `PATH`; and a portable `.zip`; on a tag, the executables in both are signed | `packaging/windows/package.sh`, with WiX 5 and `packaging/windows/quakec-studio.wxs` |
| Linux x86_64 and aarch64 | `.deb`, `.rpm`, `.AppImage` and `.flatpak` | `packaging/linux/package.sh`, with the desktop entry and icon, `deb/control`, `quakec-studio.spec`, `AppRun.sh` and the Flatpak manifest `com.quakecstudio.QuakeCStudio.yml` beside it |
| All | `SHA256SUMS`, every package's SHA-256 | the workflow's last job |

Each file a package needs is its own file under `packaging/`, never written out by a script,
so it can be checked and formatted on its own; a script fills in only the version (and the
deb's architecture) with `sed`. The build checks the desktop entry with
`desktop-file-validate`, the rpm's spec with `rpmspec -P`, the Info.plist with `plutil -lint`,
and the WiX source by building it.

On Linux, `quakec-ide` and `quakec-lsp` are static musl binaries (`<arch>-unknown-linux-musl`),
which run on any Linux with no library from the system; the app and the engine link glibc, since
winit and wgpu open the display and GPU libraries (libxkbcommon, X11 or Wayland, Vulkan or GL,
and the GPU's own driver) at run time, which a static binary cannot do. The deb and rpm require
those libraries. On Windows every executable has the C runtime linked in (`+crt-static`), so
none needs the Visual C++ Redistributable. `quakec-ide` and `quakec-lsp` use mimalloc as
their global allocator, faster than musl's own; the app and the engine keep the system's. macOS binaries link only the system's libraries, as every Mac app does.

Every package holds the four binaries, the desktop app `quakec-studio`, the preview host
`quakec-engine` beside it (where `qs-engine`'s `locate_host` looks), the command line
`quakec-ide` and the language server `quakec-lsp`, with `LICENSE` (the GPL-3.0 text) and
`NOTICE` (`packaging/NOTICE.txt`: the copyright, the corresponding-source link, the ported
code and the non-affiliation notice). On macOS the preview host is signed as its own
executable before the app is, and the app's own notarization covers both. An MSI's version
is three numbers whose first is at most 255, so 2026.11.1 is the MSI's 26.11.1.

The Flatpak's sandbox reaches the home directory (projects, and Steam, GOG and Heroic's
folders, their Flatpak clients' under `~/.var/app` included), `/mnt` and `/media` read only,
the GPU, the display, sound, the network for the agent's providers, and the Secret Service
(`org.freedesktop.secrets`) for the keychain.

## The install tests

Before anything is published, each package is installed on a runner that never built the app
and checked there with `quakec-ide doctor` (P4-18), which reports the games it finds, starts the
preview host headless on a game and stops it over its bridge, and writes, reads back and
deletes a keychain entry:

- **macOS:** the package's signature is checked with `pkgutil --check-signature`, `spctl
  --assess --type install` accepts it and its staple is validated; it installs with `sudo
  installer -pkg ... -target /`; `spctl` assesses the app, its staple is validated, each of its
  four executables is checked as signed with the hardened runtime, `/usr/local/bin/quakec-lsp`
  links into the app, `quakec-ide` runs from the `PATH` for the host check, and the app's
  window opens.
- **Windows x64 and arm64:** the MSI installs with `msiexec /qn`, the install folder is on the
  machine `PATH`, the host check runs, the app's window opens, and the portable zip's host
  check runs too.
- **Linux x86_64 and aarch64:** the deb installs with `apt` on Ubuntu 24.04 (host check, and the
  window under Xvfb), the rpm with `dnf` in a Fedora 41 container (host check), and the
  AppImage runs its host check. The Flatpak runs every check from inside its sandbox: a Steam
  folder holding LibreQuake as Quake (`packaging/linux/steam`) is found, the host starts on it, and the keychain is
  reached through gnome-keyring.

The game is LibreQuake (`cargo xtask assets fetch`); Quake's data is never used in CI.

## The signing secrets

The five Azure secrets and the ten `APPLE_*` ones are **repository-level**, by Justin's choice, so only a workflow that signs may reference them:
`.github/workflows/release.yml` passes each, by name, to the reusable
`.github/workflows/reusable-build.yml`, the one workflow that reads them. Agents never handle
their values; the reusable workflow's first job checks each is set and prints no value.

| Secret | What it is | Used by |
| --- | --- | --- |
| `APPLE_APPLICATION_CERTIFICATE_PEM` | The Developer ID Application certificate, PEM | `security import` |
| `APPLE_APPLICATION_KEY_PEM` | Its private key, PEM | `security import` |
| `APPLE_APPLICATION_IDENTITY` | The certificate's SHA-1 fingerprint | `codesign --sign` |
| `APPLE_INSTALLER_CERTIFICATE_PEM` | The Developer ID Installer certificate, PEM | `security import` |
| `APPLE_INSTALLER_KEY_PEM` | Its private key, PEM | `security import` |
| `APPLE_INSTALLER_IDENTITY` | The certificate's SHA-1 fingerprint, which finds the identity's name for `productsign` | `productsign --sign` |
| `APPLE_KEYCHAIN_PASSWORD` | A password for the `build.keychain` the job makes and deletes, and for the PKCS#12 files it imports | `security` |
| `APPLE_NOTARY_KEY_ID` | The App Store Connect API key notarization signs in with | `notarytool` |
| `APPLE_NOTARY_ISSUER_ID` | That key's issuer | `notarytool` |
| `APPLE_NOTARY_KEY` | That key's `.p8` | `notarytool` |
| `AZURE_CLIENT_ID` | The Entra app registration that signs, trusted over OIDC | `aas-sign` |
| `AZURE_TENANT_ID` | Its tenant | `aas-sign` |
| `TRUSTED_SIGNING_ENDPOINT` | The Trusted Signing account's regional endpoint | `aas-sign` |
| `TRUSTED_SIGNING_ACCOUNT` | The Trusted Signing account's name | `aas-sign` |
| `CERTIFICATE_PROFILE` | The certificate profile that signs | `aas-sign` |

**Owner and renewal:** every credential is Justin's.
- **Apple secrets:** `smoketurner/smoketurner-infra` makes and copies every `APPLE_*` secret
  above (`environments/apple/`, then `environments/github/`). App Store Connect lets only the
  Account Holder create a Developer ID certificate, so the root makes each certificate's private
  key and CSR, the Account Holder uploads each CSR in Certificates, Identifiers & Profiles, and
  the issued certificates go back in as Terraform variables. Both Developer ID certificates last
  five years and are renewed the same way. The workflow imports Apple's Developer ID G2
  intermediate beside them, fetched from apple.com and checked against its SHA-256; it is valid
  to 2031-09-17. The notary key is an App Store Connect team API key with the Developer role; it
  stays valid until revoked.
- **Entra app registration:** `github-actions-release-signing`, which signs every Smoke Turner project as Smoke Turner, LLC through the `vouch-azure-prod` account's `vouch-prod` certificate profile. It uses federated credentials, not a secret, so nothing expires. `smoketurner/smoketurner-infra` manages this repository's credential (`environments/azure/code_signing.tf`) and copies the five Azure values into its secrets (`environments/github/main.tf`). The credential trusts exactly `repo:quakecstudio@343029298/editor@1420764495:workflow:release:ref_type:tag`, the subject GitHub issues under the organization's customized OIDC claims: `workflow` is the calling workflow's name, `release`, not the reusable one's, and `ref_type:tag` confines signing to tag runs. Renaming `release.yml`'s workflow, or signing from a branch, fails Azure's login with AADSTS700213, as the first run of the workflow then named Sign did from a branch ([#23](https://github.com/quakecstudio/editor/issues/23)). Such a failure goes to Justin; it is not worked around.

## Signing

On macOS the job checks every secret above is set, builds for Apple silicon
(`aarch64-apple-darwin`) and lays out the app. It bundles each Developer ID certificate and key
into a PKCS#12 file with the system's `/usr/bin/openssl` and imports both into `build.keychain`
(made the default keychain, with `codesign` and `productsign` allowed to use the keys). It signs
each executable and then the app with `/usr/bin/codesign --options runtime --timestamp`, the
identity named by its SHA-1 fingerprint, and verifies it; stores the notary API key as the
`notarytool-profile` keychain profile; and notarizes and staples the app itself, so it carries
its ticket once installed and opens offline. It then makes the package with `package.sh pkg`,
which refuses a payload holding AppleDouble `._` files, signs it with `productsign` as the
Developer ID Installer identity, and notarizes and staples it. It always deletes the keychain. On Windows, a tag's run signs the four executables with `skeeto/aas-sign`, which exchanges
the job's OIDC token for an Azure token itself, then checks each Authenticode signature before
the zip and the MSI are made. `aas-sign` signs PE files only, so the MSI itself is not signed;
the executables it installs are.

## Checking an attestation

Every package has a build provenance attestation from `actions/attest`, made in the job of
`reusable-build.yml` that built it and signed with that workflow's OIDC identity. Check a downloaded package with:

```bash
gh attestation verify quakec-studio-2026.11.1-windows-x64.msi --repo quakecstudio/editor \
  --signer-workflow quakecstudio/editor/.github/workflows/reusable-build.yml
```

## The compatibility list

Every release tag also runs the release workflow's **Regenerate the compatibility list** job
(P4-21, COMP-9). It fetches the conformance corpus, writes
[`docs/compatibility.md`](compatibility.md) with `cargo xtask compatibility`, attaches it to the
run and shows it in the job summary. The job fails when the list on the tagged commit differs
from the one it wrote. In that case, commit the attached list and tag again.

To write the list before tagging, with the corpus fetched (`cargo xtask corpus fetch`):

```bash
cargo xtask compatibility
```

`corpus/compatibility.toml` records what the list cannot measure: why a codebase is not
compared in the compiler differential, and the issues that track each gap.

## Removing QuakeC Studio from a Mac

The package installs the app and two links, and macOS records them under the package's
identifier. To remove them:

```bash
sudo rm -rf "/Applications/QuakeC Studio.app"
sudo rm -f /usr/local/bin/quakec-ide /usr/local/bin/quakec-lsp
sudo pkgutil --forget com.quakecstudio.QuakeCStudio
```

The app's own files stay where `qs-paths` puts them: settings and data in
`~/Library/Application Support/QuakeC Studio`, its cache in `~/Library/Caches/QuakeC Studio`
and its logs in `~/Library/Logs/QuakeC Studio`. Remove those folders too for a clean slate.
