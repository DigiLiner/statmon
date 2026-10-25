# Code signing (Windows)

StatMon ships as a native, **unsigned** executable. Windows 11 **Smart App
Control (SAC)** blocks unsigned/unknown-publisher binaries and offers no
allow-list or “Run anyway” override. The supported solutions are:

1. disable SAC on that PC (irreversible without reinstalling Windows), or
2. ship an **Authenticode-signed** build.

A self-signed certificate does **not** satisfy SAC. Use a publicly trusted code
signing service or certificate.

---

## SignPath Foundation

Recommended for this project: free code signing for open-source projects.

### Before approval

- Repository is public with an OSI-approved license (`MIT`, see `LICENSE`).
- No binaries are committed (`dist/`, `*.zip` are git-ignored).
- The release workflow builds the artifact from source in CI.
- Keep `main` green; SignPath validates the project.

### After the approval e-mail

1. **Sign in to SignPath** and open your organization. Connect the GitHub
   repository `DigiLiner/statmon` if it is not linked yet.
2. **Note the slugs** shown in the project:
   - organization ID (a GUID)
   - project slug (for example `statmon`)
   - signing policy slug (for example `release-signing`)
   - artifact configuration slug (for example `statmon-release-zip`)

3. **Create the artifact configuration.** Upload
   [`signpath-artifact-configuration.xml`](signpath-artifact-configuration.xml)
   or paste its content. It signs `statmon.exe` inside
   `statmon-<version>-win64.zip`; the ZIP layout must keep the exe at the root.

4. **Create a signing policy** (for example `release-signing`). Foundation
   projects usually require **manual approval** for each signing request; add
   at least one approver.

5. **Create an API token** (SignPath → user settings → API tokens) scoped to the
   project/policy. Copy it once; it is not shown again.

6. **Configure GitHub** → repository → *Settings → Secrets and variables →
   Actions*:

   | Kind | Name | Value |
   | --- | --- | --- |
   | Secret | `SIGNPATH_API_TOKEN` | API token from step 5 |
   | Variable | `SIGNPATH_ORGANIZATION_ID` | organization GUID |
   | Variable | `SIGNPATH_PROJECT_SLUG` | `statmon` (or your slug) |
   | Variable | `SIGNPATH_SIGNING_POLICY_SLUG` | `release-signing` |
   | Variable | `SIGNPATH_ARTIFACT_CONFIG_SLUG` | `statmon-release-zip` |

   The release workflow only calls SignPath when **all** of these are present;
   otherwise it publishes an unsigned build. If signing is configured but
   fails, the job fails and **no** release is published.

7. **Release.** Push a tag:

   ```powershell
   git tag v0.1.0
   git push origin v0.1.0
   ```

   The workflow builds `statmon-<version>-win64.zip`, uploads it as a workflow
   artifact, submits the signing request, and waits. If the policy requires
   manual approval, approve the request in the SignPath web UI. The workflow
   then downloads the **signed** ZIP (with `SHA256.txt`) and attaches it to the
   GitHub release.

8. **Verify**:

   ```powershell
   signtool verify /pa /v statmon.exe
   ```

   Or right-click `statmon.exe` → Properties → **Digital Signatures**. With
   SignPath Foundation the publisher is shown as **SignPath Foundation**.

9. Delete older unsigned releases or mark them as “unsigned” if users might
   still download them.

> SignPath’s project page also generates a “GitHub Actions” snippet with the
> exact action version and input names for your account. If SignPath changes
> the action inputs, prefer their snippet; the workflow in this repository uses
> the settings above.

### Notes

- SmartScreen reputation may still take a few downloads to settle, but SAC
  accepts validly signed binaries.
- SignPath signs the uploaded artifact; only the signed ZIP should be attached
  to the release. The workflow already does this automatically.
- Never commit the PFX/certificate or API token to the repository.

---

## Manual signing with your own certificate

If you obtain an OV/EV certificate instead:

```powershell
signtool sign /fd SHA256 /tr http://timestamp.digicert.com /td SHA256 `
  /f certificate.pfx /p "<password>" target\release\statmon.exe
signtool verify /pa target\release\statmon.exe
```

`signtool.exe` ships with the Windows SDK (Visual Studio Build Tools). The PFX
can be used locally or in CI via secrets; do not store it in the repository.
