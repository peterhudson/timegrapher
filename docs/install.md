# Installing Timegrapher

Ready-made downloads are on the
[Releases page](https://github.com/peterhudson/timegrapher/releases/latest).
Each release has the desktop app and the `timegrapher` command line tool for
Linux, macOS and Windows, and `SHA256SUMS.txt` to check a download.

The builds are **not signed** with an Apple or Microsoft developer
certificate, so macOS and Windows warn the first time you open each new
version. The steps below get past that once; after that the app opens
normally.

## macOS

Download `timegrapher-<version>-macos.dmg`. It runs on Apple silicon and
Intel Macs with macOS 11 (Big Sur) or later.

1. Open the `.dmg` and drag **Timegrapher** onto **Applications**.
2. Open Timegrapher from Applications. macOS says *"Timegrapher" Not Opened*
   (or, on older versions, *cannot be opened because Apple cannot check it
   for malicious software*). Click **Done**.
3. Open **System Settings**, then **Privacy & Security**. Scroll down to
   *"Timegrapher" was blocked to protect your Mac* and click **Open Anyway**.
   Enter your password if asked, then click **Open Anyway** again.
4. When Timegrapher asks to use the microphone, click **Allow**. If you
   clicked Don't Allow by mistake, turn it on in System Settings, Privacy &
   Security, Microphone.

On macOS 14 and earlier, right-clicking the app, choosing **Open** and then
**Open** in the dialog does the same as steps 2 and 3.

The command line tool is inside the app at
`/Applications/Timegrapher.app/Contents/MacOS/timegrapher`, and on its own in
`timegrapher-<version>-macos-cli.tar.gz`. To run it from Terminal by name:

```sh
sudo ln -s /Applications/Timegrapher.app/Contents/MacOS/timegrapher /usr/local/bin/timegrapher
timegrapher --version
```

## Windows

Download `timegrapher-<version>-windows-x86_64.zip` (Windows 10 or 11,
64-bit).

1. Right-click the `.zip`, choose **Extract All…**, and extract it to a
   folder you keep, such as `Documents\Timegrapher`.
2. Double-click `timegrapher-app.exe`. Windows says *Windows protected your
   PC*. Click **More info**, then **Run anyway**.
3. If Windows asks whether Timegrapher may use the microphone, allow it. If
   no input is found later, check Settings, Privacy & security, Microphone,
   and turn on *Let desktop apps access your microphone*.

To pin it, right-click `timegrapher-app.exe` and choose **Pin to Start**.
`timegrapher.exe` in the same folder is the command line tool: open a
terminal there and run `.\timegrapher.exe --help`.

## Linux

Two downloads, for 64-bit Intel and AMD PCs with a distribution from 2022 or
later (Ubuntu 22.04, Debian 12, Fedora 36 or newer):

- `timegrapher-<version>-linux-x86_64.AppImage`: the desktop app in one
  file. Make it executable and open it:

  ```sh
  chmod +x timegrapher-*-linux-x86_64.AppImage
  ./timegrapher-*-linux-x86_64.AppImage
  ```

  Most file managers can do the same: Properties, Permissions, *Allow
  executing file as program*, then double-click. The command line tool is
  inside too: `./timegrapher-*.AppImage cli --help`. If it says FUSE is
  missing, install `libfuse2` (Ubuntu 22.04: `sudo apt install libfuse2`;
  Ubuntu 24.04: `sudo apt install libfuse2t64`) or run it with
  `--appimage-extract-and-run`.
- `timegrapher-<version>-linux-x86_64.tar.gz`: both programs, a desktop
  entry and an icon, to unpack wherever you like:

  ```sh
  tar -xzf timegrapher-*-linux-x86_64.tar.gz
  cd timegrapher-*-linux-x86_64
  ./timegrapher-app
  ./timegrapher --version
  ```

Sound input goes through ALSA (`libasound2`), which every desktop
distribution installs; PipeWire and PulseAudio systems reach the
microphone through their ALSA plugin.

## Updating

To update, download the new version and install it the same way as the
first, over the old one (macOS: replace the app in Applications; Windows:
extract over the old folder; Linux: replace the AppImage or tarball).
Settings and saved recordings are kept, since they live elsewhere.

To ask whether a newer version is published:

```sh
timegrapher update           # "Timegrapher 0.2.0 is available ..." or "up to date"
timegrapher update --json    # the same as JSON (schema timegrapher.update/1)
```

This makes one request to GitHub's public releases API
(`api.github.com/repos/peterhudson/timegrapher/releases/latest`). It sends
no account, token or identifier; GitHub sees the address it comes from, as
with any download. Nothing is downloaded or installed: it only names the
newer version and the page to get it from. Drafts and pre-releases are
never offered.

## Checking a download

```sh
sha256sum -c SHA256SUMS.txt --ignore-missing        # Linux
shasum -a 256 -c SHA256SUMS.txt --ignore-missing    # macOS
```

On Windows, in PowerShell: `Get-FileHash timegrapher-*.zip` and compare
with the line in `SHA256SUMS.txt`.

## Building from source

See the [README](../README.md): `cargo run --release -p timegrapher-app`
with a Rust toolchain (and `libasound2-dev` on Linux).

## Making a release

For maintainers. Releases are built by
[.github/workflows/release.yml](../.github/workflows/release.yml), never on
a personal computer.

1. The version in the root `Cargo.toml` (`[workspace.package]`, `version`)
   is the version released. Raise it in a pull request before each release
   after the first, following [semantic versioning](https://semver.org):
   0.1.0 to 0.1.1 for fixes, to 0.2.0 for new features.
2. Write the release notes, for watchmakers rather than developers, in
   `packaging/release-notes/X.Y.Z.md` (the new version), in the same pull
   request. They become the draft's text; without them the draft lists the
   pull requests merged since the last release.
3. Wait for **CI** on the exact main commit to pass, then start the build,
   either way:
   - On GitHub: **Actions**, **Release**, **Run workflow**, branch `main`,
     tick **Draft a release**, **Run workflow**.
   - Or from a clone: `git fetch origin && git tag v0.1.0 origin/main && git push origin v0.1.0`
     (the tag must be `v` plus the Cargo.toml version, or the build stops).
4. When the run is green, **Releases** shows a draft with every file. Try
   the download for your platform, edit the notes if needed, and click
   **Publish release**. Until then nobody sees it and the app's update check
   does not offer it.

Before any tag or **Draft a release** build, the release workflow checks the
newest CI run for that exact commit from this repository's main push or manual
CI dispatch. All five jobs must be completed successfully: the three platform
`test` jobs, `mcp-renderer`, and `regress`. The latter must also contain the
successful **Required real-recording analyze/session equivalence** step; an older
same-name regression job alone does not establish that coverage. The accepted
run URL, SHA, attempt and job results appear in the release gate's log and summary.

The gate waits up to ten minutes for pending CI; a failed, missing, cancelled or
incomplete latest run stops the build rather than reusing an older green result.
All five jobs must belong to the latest attempt: use **Re-run all jobs** after a
partial rerun. Markdown-only commits may have no automatic CI; run **Actions →
CI → Run workflow** on main first. Do not tag an untested branch or substitute
PR-head results for the candidate's main CI.

Pull-request packaging and manual runs without **Draft a release** remain
nonpublishing checks and explicitly report **gate not required**. Manual runs on
a tag also remain nonpublishing with either checkbox value; manual drafting
requires main. The gate does not add acoustic or MCP protocol tests of the extracted release artifacts; their
current checks remain the help/version and packaging checks in the workflow.
If CI job names or the required equivalence step change, update the centralized
contract in `.github/scripts/release_ci_gate.py` and its offline tests in the same
review. Those tests run in all three existing platform jobs, without network or
release operations.
