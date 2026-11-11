# Releasing

How QuakeC Studio is packaged, signed, checked and released (P4-18,
[#129](https://github.com/quakecstudio/editor/issues/129)), and the signing it uses (P0-23,
[#23](https://github.com/quakecstudio/editor/issues/23)). Two workflows do it: `.github/workflows/release.yml`, started by a tag or by hand, calls the
reusable `.github/workflows/reusable-build.yml`, which builds, signs and attests every package.
The attestations are made in the reusable workflow so that each names it as the signer,
whatever calls it. On a tag, the Windows executables are signed with Azure Artifact Signing
(formerly Trusted Signing); a run by hand leaves them unsigned, since Azure trusts only tag runs.

## Making a release

1. Set `version` in the root `Cargo.toml` to the release's, such as `2026.11.0`, and merge it.
2. Push a tag naming it: `v2026.11.0`, or `v2026.11.0-rc1` for a pre-release. The workflow
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
| macOS, universal | `quakec-studio-<version>-macos-universal.dmg`: `QuakeC Studio.app`, signed with the hardened runtime, in a signed, notarized and stapled disk image | `packaging/macos/package.sh`, with `packaging/macos/Info.plist` |
| Windows x64 and arm64 | `quakec-studio-<version>-windows-<arch>.msi`, installing for every user under Program Files with a Start menu shortcut and the folder on `PATH`; and a portable `.zip`; on a tag, the executables in both are signed | `packaging/windows/package.sh`, with WiX 5 and `packaging/windows/quakec-studio.wxs` |
| Linux x86_64 and aarch64 | `.deb`, `.rpm`, `.AppImage` and `.flatpak` | `packaging/linux/package.sh`, with the desktop entry and icon, `deb/control`, `quakec-studio.spec`, `AppRun.sh` and the Flatpak manifest `com.quakecstudio.QuakeCStudio.yml` beside it |
| All | `SHA256SUMS`, every package's SHA-256 | the workflow's last job |

Each file a package needs is its own file under `packaging/`, never written out by a script,
so it can be checked and formatted on its own; a script fills in only the version (and the
deb's architecture) with `sed`. The build checks the desktop entry with
`desktop-file-validate`, the rpm's spec with `rpmspec -P`, the Info.plist with `plutil -lint`,
and the WiX source by building it.

Every package holds the four binaries, the desktop app `quakec-studio`, the preview host
`preview-host` beside it (where `qs-engine`'s `locate_host` looks), the command line
`quakec-ide` and the language server `quakec-lsp`, with `LICENSE` (the GPL-3.0 text) and
`NOTICE` (`packaging/NOTICE.txt`: the copyright, the corresponding-source link, the ported
code and the non-affiliation notice). On macOS the preview host is signed as its own
executable before the app is, and the disk image's notarization covers both. An MSI's version
is three numbers whose first is at most 255, so 2026.11.0 is the MSI's 26.11.0.

The Flatpak's sandbox reaches the home directory (projects, and Steam, GOG and Heroic's
folders, their Flatpak clients' under `~/.var/app` included), `/mnt` and `/media` read only,
the GPU, the display, sound, the network for the agent's providers, and the Secret Service
(`org.freedesktop.secrets`) for the keychain.

## The install tests

Before anything is published, each package is installed on a runner that never built the app
and checked there with `quakec-ide doctor` (P4-18), which reports the games it finds, starts the
preview host headless on a game and stops it over its bridge, and writes, reads back and
deletes a keychain entry:

- **macOS:** the disk image's staple is validated, the app is copied to `/Applications`,
  `spctl` assesses it, the host check runs and the app's window opens.
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

Justin entered these twelve secrets on 2026-10-16, following vouch and quack. They are
**repository-level**, by his choice, so only a workflow that signs may reference them:
`.github/workflows/release.yml` passes each, by name, to the reusable
`.github/workflows/reusable-build.yml`, the one workflow that reads them. Agents never handle
their values; the reusable workflow's first job checks each is set and prints no value.

| Secret | What it is | Used by |
| --- | --- | --- |
| `APPLE_CERTIFICATE_P12_BASE64` | The Developer ID Application certificate and its key, as a base64 PKCS#12 file | `security import` |
| `APPLE_CERTIFICATE_PASSWORD` | That file's password | `security import` |
| `APPLE_CERTIFICATE_NAME` | The certificate's name, such as `Developer ID Application: ... (TEAMID)` | `codesign --sign` |
| `MACOS_KEYCHAIN_PASSWORD` | A password for the `build.keychain` the job makes and deletes | `security create-keychain` |
| `APPLE_ID` | The Apple ID that submits for notarization | `notarytool` |
| `APPLE_ID_PASSWORD` | Its app-specific password | `notarytool` |
| `APPLE_TEAM_ID` | The developer team's id | `notarytool` |
| `AZURE_CLIENT_ID` | The Entra app registration that signs, trusted over OIDC | `aas-sign` |
| `AZURE_TENANT_ID` | Its tenant | `aas-sign` |
| `TRUSTED_SIGNING_ENDPOINT` | The Trusted Signing account's regional endpoint | `aas-sign` |
| `TRUSTED_SIGNING_ACCOUNT` | The Trusted Signing account's name | `aas-sign` |
| `CERTIFICATE_PROFILE` | The certificate profile that signs | `aas-sign` |

**Owner and renewal:** every credential is Justin's.
- **Developer ID certificate:** renews every five years, in Apple's developer account.
- **App-specific password:** stays valid until revoked.
- **Entra app registration:** `github-actions-release-signing`, which signs every Smoke Turner project as Smoke Turner, LLC through the `vouch-azure-prod` account's `vouch-prod` certificate profile. It uses federated credentials, not a secret, so nothing expires. `smoketurner/smoketurner-infra` manages this repository's credential (`environments/azure/code_signing.tf`) and copies the five Azure values into its secrets (`environments/github/main.tf`). The credential trusts exactly `repo:quakecstudio@343029298/editor@1420764495:workflow:release:ref_type:tag`, the subject GitHub issues under the organization's customized OIDC claims: `workflow` is the calling workflow's name, `release`, not the reusable one's, and `ref_type:tag` confines signing to tag runs. Renaming `release.yml`'s workflow, or signing from a branch, fails Azure's login with AADSTS700213, as the first run of the workflow then named Sign did from a branch ([#23](https://github.com/quakecstudio/editor/issues/23)). Such a failure goes to Justin; it is not worked around.

## Signing

The steps follow vouch-sh/vouch's `reusable-build.yml`. On macOS the job checks every secret
above is set, builds both architectures and joins them with `lipo`, lays out the app, imports
the certificate into `build.keychain` (made the default keychain, with `codesign` allowed to
use the key), signs each executable and then the app with `/usr/bin/codesign --options runtime
--timestamp` and verifies it, signs the disk image, stores the notarytool credentials as the
`notarytool-profile` keychain profile, submits the disk image with `--wait`, staples it, and
always deletes the keychain. On Windows, a tag's run signs the four executables with `skeeto/aas-sign`, which exchanges
the job's OIDC token for an Azure token itself, then checks each Authenticode signature before
the zip and the MSI are made. `aas-sign` signs PE files only, so the MSI itself is not signed;
the executables it installs are.

## Checking an attestation

Every package has a build provenance attestation from `actions/attest`, made in the job of
`reusable-build.yml` that built it and signed with that workflow's OIDC identity. Check a downloaded package with:

```bash
gh attestation verify quakec-studio-2026.11.0-windows-x64.msi --repo quakecstudio/editor \
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
