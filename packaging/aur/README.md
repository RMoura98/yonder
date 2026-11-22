# AUR package: yonder-bin

`yonder-bin` repackages the official `Yonder_<ver>_amd64.deb` release asset for
Arch Linux. Users install it with `yay -S yonder-bin` or `paru -S yonder-bin`.

The files in `yonder-bin/` are the source of truth; the AUR repository is a
mirror of them.

## First publication (one time)

1. Create an account at <https://aur.archlinux.org/register>.
2. Add an SSH public key under *My Account > SSH Public Key*
   (`ssh-keygen -t ed25519 -f ~/.ssh/aur`).
3. Clone the (empty) package repo; the first push creates the package:

   ```sh
   git clone ssh://aur@aur.archlinux.org/yonder-bin.git
   cp packaging/aur/yonder-bin/PKGBUILD packaging/aur/yonder-bin/.SRCINFO yonder-bin/
   cd yonder-bin
   git add PKGBUILD .SRCINFO
   git commit -m "Initial import: yonder-bin 1.0.1"
   git push
   ```

   The AUR only accepts the `master` branch.

Optionally test first on Arch: `cd packaging/aur/yonder-bin && makepkg -si`.

## Bumping for a new release

Manually:

```sh
cd packaging/aur/yonder-bin
# edit PKGBUILD: set pkgver=<new>, pkgrel=1
updpkgsums                              # from pacman-contrib
makepkg --printsrcinfo > .SRCINFO
```

Then copy `PKGBUILD` and `.SRCINFO` to your AUR clone, commit and push.

## Automation

`.github/workflows/aur.yml` runs when a release is published (or manually via
*Run workflow*). It rewrites `pkgver`, resets `pkgrel`, recomputes the
checksums and `.SRCINFO` in `packaging/aur/yonder-bin`, and commits that to
`main` as `github-actions[bot]`.

If the repository secret `AUR_SSH_PRIVATE_KEY` is set (the private half of the
key added to your AUR account), it also pushes to the AUR. Without the secret
it only updates the files here and never fails the release.

To enable: *Settings > Secrets and variables > Actions > New repository
secret*, named `AUR_SSH_PRIVATE_KEY`. Do the first AUR push manually (above).
