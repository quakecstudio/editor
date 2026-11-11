# Releasing

How a build of QuakeC Studio is signed. The release pipeline itself (packaging, the
release on GitHub) is P4-18; this page is the signing it reuses (P0-23, [#23](https://github.com/quakecstudio/editor/issues/23)).
Windows signing is deferred past the MVP (#23, moved to the post-MVP backlog on 2026-11-11):
the Sign workflow's Windows job fails at the Azure login until the app registration has a
federated credential for this repository, so the MVP's Windows packages are unsigned.

## The signing secrets

Justin entered these twelve secrets on 2026-10-16, following vouch and quack. They are
**repository-level**, by his choice, so only a workflow that signs may reference them:
today `.github/workflows/sign.yml`, started by hand (`workflow_dispatch`) or by pushing a `v*`
tag. It is a standalone workflow, never called from another, so that every attestation it
makes names `sign.yml` as its signer. Agents never handle their values; the workflow's first
job checks each is set and prints no value.

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
- **Entra app registration:** uses federated credentials, not a secret, so nothing expires. The organization customizes its OIDC subject claim, so the subject is not the default `repo:<owner>/<repo>:ref:<ref>`. A run of Sign presents `repo:quakecstudio@343029298/editor@1420764495:workflow:Sign:ref_type:branch` from a branch and the same ending in `ref_type:tag` from a `v*` tag, as Azure's AADSTS700213 error showed on the first run ([#23](https://github.com/quakecstudio/editor/issues/23)). Each needs a federated credential with issuer `https://token.actions.githubusercontent.com` and audience `api://AzureADTokenExchange`. If `aas-sign` fails to log in because none matches, the subject in the error goes to Justin; it is not worked around.

## Signing a build

The steps follow vouch-sh/vouch's `reusable-build.yml`. Run **Sign** from the Actions tab,
`gh workflow run sign.yml --ref <branch>`, or push a `v*` tag. It:

1. checks every secret above is set;
2. on macOS, builds the app, imports the certificate into `build.keychain` (made the default
   keychain, with `codesign` allowed to use the key), signs the binary with
   `/usr/bin/codesign --options runtime --timestamp` and verifies it, stores the notarytool
   credentials as the `notarytool-profile` keychain profile and submits the zipped binary
   with `--wait`, attests the signed binary, uploads it as `quakec-studio-macos`, and always
   deletes the keychain;
3. on Windows, builds the app, signs the `.exe` with `skeeto/aas-sign` (Azure Trusted
   Signing, logging in over OIDC with the client and tenant ids), checks its Authenticode
   signature is valid, attests it and uploads it as `quakec-studio-windows`.

## Checking an attestation

Each signed binary has a build provenance attestation from `actions/attest`, signed with the
workflow's OIDC identity. Download it from the run and check it with:

```bash
gh attestation verify quakec-studio.exe --repo quakecstudio/editor \
  --signer-workflow quakecstudio/editor/.github/workflows/sign.yml
```

## The compatibility list

Every release tag also runs the Sign workflow's **Regenerate the compatibility list** job
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
