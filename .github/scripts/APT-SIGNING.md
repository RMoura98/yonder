# APT repository signing (maintainer guide)

The `Deploy website` workflow publishes a signed APT repository at
`https://rmoura98.github.io/yonder/apt` using the secret `APT_GPG_PRIVATE_KEY`.
Without the secret the website still deploys, but the apt repo is skipped.

## 1. Create the key (once)

Use a temporary GNUPGHOME so your personal keyring is untouched. Run in Git Bash,
WSL or any Linux/macOS shell:

```bash
export GNUPGHOME="$(mktemp -d)"
chmod 700 "$GNUPGHOME"

cat > key-params <<'EOF'
%no-protection
Key-Type: RSA
Key-Length: 4096
Key-Usage: sign
Name-Real: Yonder APT signing
Name-Email: 27810563+RMoura98@users.noreply.github.com
Expire-Date: 0
%commit
EOF

gpg --batch --generate-key key-params
gpg --list-secret-keys --keyid-format long

# Export the armored private key to a file (treat it as a secret!)
gpg --armor --export-secret-keys "Yonder APT signing" > yonder-apt-private.asc
```

The key has no passphrase (required for unattended CI signing) and never expires.

## 2. Add it as a repository secret

1. Open `yonder-apt-private.asc` and copy the whole block, including the
   `-----BEGIN PGP PRIVATE KEY BLOCK-----` and `-----END ...-----` lines.
2. GitHub repo -> Settings -> Secrets and variables -> Actions -> New repository secret.
3. Name: `APT_GPG_PRIVATE_KEY`, value: the pasted block. Save.

## 3. Back it up

Store `yonder-apt-private.asc` in a password manager or an encrypted drive, then
delete the plain file and the temporary GNUPGHOME (`rm -rf "$GNUPGHOME" yonder-apt-private.asc key-params`).
If the key is lost, users must re-import a new public key, so keep a backup.

## 4. First deploy

GitHub repo -> Actions -> "Deploy website to GitHub Pages" -> Run workflow (branch `main`).
It downloads the latest release's amd64 `.deb` and publishes the repo. Later releases
trigger it automatically (`release: published`).

Check: `https://rmoura98.github.io/yonder/apt/dists/stable/InRelease` and
`https://rmoura98.github.io/yonder/apt/yonder.gpg` should load.

Note: a release created with the default `GITHUB_TOKEN` does not trigger other workflows.
If release.yml publishes using that token, run the deploy manually after a release (or
use a PAT / `workflow_run`). Also, the `.deb` must be attached before the release is
published, otherwise run the workflow again once assets are uploaded.
