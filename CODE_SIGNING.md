# Code signing policy

Free code signing provided by [SignPath.io](https://about.signpath.io), certificate by [SignPath Foundation](https://signpath.org).

Windows builds of Yonder (`yonder.exe`, the `x64-setup.exe` and `.msi` installers, and the `yonder.exe` inside the
`x64-portable.zip`) are signed through the SignPath Foundation program. Releases published before code signing was set
up are unsigned.

## What gets signed

Only files built from this repository's source by the [release workflow](.github/workflows/release.yml) on
GitHub-hosted runners. For every release, SignPath checks that the files came from that workflow run before it signs
them, and each signing request is approved by hand by an approver listed below. Nothing built on a personal computer is
signed.

The installers also contain unsigned helper files from upstream open-source projects (the NSIS uninstaller and plugin
DLLs that Tauri generates). Yonder's own code is signed.

## Team roles

Yonder is maintained by one person.

- Committers and reviewers: [RMoura98](https://github.com/RMoura98). Pull requests from anyone else are reviewed by a
  committer before they are merged.
- Approvers: [RMoura98](https://github.com/RMoura98)

All team members use multi-factor authentication on GitHub and SignPath.

## Privacy policy

Yonder has no accounts, analytics, telemetry or crash reporting. Your settings and break history stay on your device.

It makes one kind of network request by itself: **the update check**. While *Check for updates automatically* is on
(the default), Yonder asks GitHub every few hours whether a newer version exists by requesting
`https://api.github.com/repos/RMoura98/yonder/releases/latest`. Nothing else is sent: GitHub sees your IP address and
a `Yonder/<version>` user agent, as with any web request, and the
[GitHub privacy statement](https://docs.github.com/site-policy/privacy-policies/github-general-privacy-statement)
applies. To stop it, turn off **Settings → About → Updates → Check for updates automatically**. *Check now* only runs
when you click it.

Apart from that, this program will not transfer any information to other networked systems unless specifically
requested by the user or the person installing or operating it. For example, break hooks run the commands you type in
Settings, and links in the app open in your browser only when you click them.

## Reporting a problem

If you think a file signed with Yonder's certificate is malicious or wasn't built from this repository, please
[open an issue](https://github.com/RMoura98/yonder/issues/new) or email SignPath at support@signpath.io.

## For maintainers: setting it up

The workflow signs Windows builds only for tag pushes, and only when the `SIGNPATH_API_TOKEN` secret exists. Without it
(forks, pull requests, manual runs) everything builds unsigned exactly as before.

How a signed release is built (`build` job, Windows leg):

1. `tauri-action` builds everything and uploads the unsigned installers to the draft release.
2. `yonder.exe` is uploaded as a workflow artifact and sent to SignPath (artifact configuration `exe`).
3. `tauri bundle --no-binary-patching` rebuilds the NSIS and MSI installers around the signed `yonder.exe`.
4. Both installers are sent to SignPath (artifact configuration `installers`).
5. The signed installers replace the unsigned ones on the draft release, and the portable zip is built from the signed
   `yonder.exe`. The `checksums` job runs after this, so `SHA256SUMS.txt` matches the signed files.

Each release therefore needs **two approvals** in SignPath. The workflow waits up to an hour for each one; if it times
out, approve the request and re-run the failed job.

Repository settings (Settings → Secrets and variables → Actions):

| Kind | Name | Value |
|------|------|-------|
| Secret | `SIGNPATH_API_TOKEN` | API token of a SignPath user with *submitter* permission on the signing policy |
| Variable | `SIGNPATH_ORGANIZATION_ID` | SignPath organization ID |
| Variable | `SIGNPATH_PROJECT_SLUG` | optional, default `yonder` |
| Variable | `SIGNPATH_SIGNING_POLICY_SLUG` | optional, default `release-signing` |
| Variable | `SIGNPATH_EXE_ARTIFACT_CONFIGURATION_SLUG` | optional, default `exe` |
| Variable | `SIGNPATH_INSTALLERS_ARTIFACT_CONFIGURATION_SLUG` | optional, default `installers` |

The two artifact configurations live in [`.signpath/artifact-configurations/`](.signpath/artifact-configurations).
SignPath doesn't read them from the repository: paste each one into the SignPath project under the matching slug.

### macOS (optional)

The same workflow signs and notarizes the macOS build once these secrets exist: `APPLE_CERTIFICATE` (base64 of the
Developer ID Application `.p12`), `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, and for notarization
`APPLE_ID`, `APPLE_PASSWORD` (an app-specific password) and `APPLE_TEAM_ID`. Without them the app keeps its ad-hoc
signature. Once notarized releases ship, remove the quarantine `postflight` from `Casks/yonder.rb` (and from the copy in
[RMoura98/homebrew-tap](https://github.com/RMoura98/homebrew-tap)) and the matching note in the README.
