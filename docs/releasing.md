# Releasing

How a build of QuakeC Studio is signed. The release pipeline itself (packaging, the
release on GitHub) is P4-18; this page is the signing it reuses (P0-23, [#23](https://github.com/quakecstudio/editor/issues/23)).

## The signing secrets

Justin entered these twelve secrets on 2026-10-16, following vouch and quack. They are
**repository-level**, by his choice, so only a workflow that signs may reference them:
today `.github/workflows/sign.yml`, started by hand (`workflow_dispatch`) and reused by the
release pipeline (`workflow_call`). Agents never handle their values; the workflow's first
job checks each is set and prints no value.

| Secret | What it is | Used by |
| --- | --- | --- |
| `APPLE_CERTIFICATE_P12_BASE64` | The Developer ID Application certificate and its key, as a base64 PKCS#12 file | `security import` |
| `APPLE_CERTIFICATE_PASSWORD` | That file's password | `security import` |
| `APPLE_CERTIFICATE_NAME` | The certificate's name, such as `Developer ID Application: ... (TEAMID)` | `codesign --sign` |
| `MACOS_KEYCHAIN_PASSWORD` | A password for the temporary keychain the job makes and deletes | `security create-keychain` |
| `APPLE_ID` | The Apple ID that submits for notarization | `notarytool` |
| `APPLE_ID_PASSWORD` | Its app-specific password | `notarytool` |
| `APPLE_TEAM_ID` | The developer team's id | `notarytool` |
| `AZURE_CLIENT_ID` | The Entra app registration that signs, trusted over OIDC | `azure/login` |
| `AZURE_TENANT_ID` | Its tenant | `azure/login` |
| `TRUSTED_SIGNING_ENDPOINT` | The Trusted Signing account's regional endpoint | Trusted Signing |
| `TRUSTED_SIGNING_ACCOUNT` | The Trusted Signing account's name | Trusted Signing |
| `CERTIFICATE_PROFILE` | The certificate profile that signs | Trusted Signing |

**Owner and renewal:** every credential is Justin's.
- **Developer ID certificate:** renews every five years, in Apple's developer account.
- **App-specific password:** stays valid until revoked.
- **Entra app registration:** uses a federated credential, not a secret, so nothing expires. The credential must trust `repo:quakecstudio/editor` for the workflow's ref. If `azure/login` fails because none does, the subject to add goes to Justin; it is not worked around.

## Signing a build

Run **Sign** from the Actions tab, or `gh workflow run sign.yml --ref <branch>`. It:

1. checks every secret above is set;
2. on macOS, builds the app, imports the certificate into a temporary keychain, signs the
   binary with the hardened runtime and a timestamp, verifies it, submits it to notarytool
   and waits, then deletes the keychain;
3. on Windows, builds the app, logs in to Azure over OIDC, signs the `.exe` with Trusted
   Signing (SHA-256, RFC 3161 timestamp) and checks the Authenticode signature is valid.
