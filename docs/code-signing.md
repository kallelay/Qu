# Code signing, provenance and verifying a download

**Status (Qu 0.4.9): the Windows `.exe` files are not yet signed with a code-signing
certificate.** Windows SmartScreen may warn about them, and some antivirus engines treat
any new, unsigned executable with suspicion (machine-learning "generic" detections such as
`...!ml`). This page says what protects a download today, how to check one, and what is
being done about signing.

## Verify a download

Every release carries `SHA256SUMS` (the engine archives and the command-line installers).
From the release after 0.4.9 it also carries `SHA256SUMS-studio-<platform>` (the Qu Studio
installers) and signatures that do not depend on trusting the download page.

1. **Checksum** (all releases):

   ```powershell
   (Get-FileHash .\qu-0.4.9-windows-x86_64-setup.exe -Algorithm SHA256).Hash
   ```

   and compare with the matching line in `SHA256SUMS` on the release page.

2. **Build provenance** (from the first release built after 0.4.9): GitHub records which
   workflow, in which repository, from which commit built each file, and signs that record.

   ```bash
   gh attestation verify qu-<version>-windows-x86_64-setup.exe --repo kallelay/Qu
   ```

3. **Signed checksums** (same releases): `SHA256SUMS` is signed keylessly with Sigstore; the
   signer identity is the release workflow and the signature is in a public transparency log.

   ```bash
   cosign verify-blob --bundle SHA256SUMS.sigstore.json \
     --certificate-identity-regexp 'https://github.com/kallelay/Qu/.*' \
     --certificate-oidc-issuer https://token.actions.githubusercontent.com SHA256SUMS
   ```

4. **Build it yourself.** The source is public and the build is ordinary:
   `cd engine && cargo build --release -p qu-cli`.

None of these makes Windows or an antivirus trust the file: only a code-signing certificate
does that. They let *you* check that the file is the one this project built.

## What Qu does on your machine

Qu has no telemetry, no analytics, no crash reporting and no auto-updater (Qu Studio's
updater is switched off). The engine contacts the network only when a script does
(`http_get`, `tcp_*`, serial ports, `python_exec`, ...) and Qu Studio only when you use an
assistant feature with a provider you configured. The installers write to your user profile
(PATH, the `.qu` file type, optional editor plugins, the Jupyter kernel) and each uninstaller
removes exactly what its installer wrote. `qu run --sandbox` denies file writes, network
and process creation; `qu run --dry-run` reports file writes instead of performing them.

## Code signing policy

*(This section is the policy page that a free code-signing programme for open-source
projects requires. It describes how releases will be signed once a certificate exists.)*

- **Project:** Qu, <https://github.com/kallelay/Qu>, licence Apache-2.0.
- **What is signed:** the Windows executables and installers produced by the release workflow
  (`qu.exe`, `qu-jupyter.exe`, the CLI installer, Qu Studio and its installer). Nothing is
  signed from a developer's machine.
- **How:** only artifacts built by the GitHub Actions release workflow from a tagged commit of
  this repository are submitted for signing. The signing key never leaves the signing service.
- **Roles:** the project owner approves every signing request (Ahmed Yahia Kallel,
  <https://github.com/kallelay>). Contributors submit changes by pull request; only the owner
  merges and tags releases.
- **What is never signed:** builds from forks, pull requests, or untagged commits, and any
  binary that contains code not in this repository.
- **Privacy:** see "What Qu does on your machine" above. Signing does not add any data
  collection to the software.

## Getting a certificate: what is needed from the project owner

The identity check and the application must be made by the owner; they cannot be automated.

1. **SignPath Foundation** (free for open-source projects; the certificate is issued to the
   foundation and the signature names it as the publisher). Apply at
   <https://signpath.org/foundation> (the application is a web form). They look for: an
   OSI-approved licence (Apache-2.0: yes), a public repository with a documented build and
   release process (this repository: yes), an actively maintained project with some history
   and users, no malware or unwanted behaviour, and a code-signing policy page (the section
   above). Approval takes days to weeks.
2. After approval SignPath issues an organisation id and project/policy slugs. Then the
   release workflow gets one extra job: build unsigned, upload the artifact, call
   `signpath/github-action-submit-signing-request`, wait for the owner's approval in
   SignPath, download the signed files, and only then run the installer, checksum,
   provenance and cosign steps (so every hash and attestation covers the signed bytes).
   Order matters: `qu.exe` must be signed before it is packed into an installer, and the
   installer signed after.
3. **If the foundation declines**, the alternatives are Azure Trusted Signing (Microsoft;
   about 10 USD/month; individuals are only accepted in a few countries, organisations need
   a verifiable history), Certum's open-source code-signing certificate (inexpensive; the key
   lives on a card or in their cloud service), or a commercial OV/EV certificate (EV removes
   SmartScreen warnings immediately; OV builds reputation with downloads).
4. **macOS** needs an Apple Developer ID certificate (99 USD/year) to sign and notarize the
   `.dmg`; without it Gatekeeper asks the user to approve the app once. **Linux** packages
   rely on the checksums and Sigstore signature above.

## If an antivirus flags a Qu file

Please report it, with the exact detection name and the file's SHA-256, to the antivirus
vendor as a false positive (Microsoft Defender: <https://www.microsoft.com/wdsi/filesubmission>)
and open an issue in this repository with the same information. Version information and an
icon are embedded in `qu.exe` and `qu-jupyter.exe` from the release after 0.4.9, which makes generic detections less likely.
