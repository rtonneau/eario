# Publishing a dashboard

This page is for the author of a dashboard that lives in its own GitHub
repository and wants users to install and update it from the Eario hub. It
needs a public repository, one signing key and a short workflow file. The
design is in [ADR 0013](../adr/0013-github-install-and-update.md); how Eario
releases its own examples and the hub is in
[Publishing a release](guide.md#publishing-a-release) of the development guide.

## What the hub looks for

A hub installs a release only from its assets. A published release (not a
draft, not a pre-release) must carry:

| Asset | What it is |
| --- | --- |
| `<slug>-<target triple>` or `<slug>-<target triple>.exe` | The dashboard executable, one per target. Targets: `x86_64-pc-windows-msvc` (`.exe`), `x86_64-unknown-linux-gnu`, `aarch64-apple-darwin`, `x86_64-apple-darwin`. |
| `SHA256SUMS.txt` | One line per executable, as `sha256sum` writes it: `<sha256>  <asset>`. |
| `SHA256SUMS.txt.minisig` | The minisign signature of `SHA256SUMS.txt`. It must be pre-hashed (the default of current minisign); the hub refuses a legacy signature. |
| `minisign.pub` | The public key file that verifies the signature. Needed unless a catalog lists your repository with its key (see [Getting listed](#getting-listed-in-a-catalog)). |

The hub checks the signature, then the SHA-256 of the downloaded executable
against the signed list, and only then runs the file, with `--eario-manifest`,
to read its name, slug and version. The slug in the manifest must be the one
in the asset name.

You do not make these files by hand. The reusable workflow below builds,
names, hashes, signs and attaches them.

## Before the first release

1. Give the dashboard a version that follows the package:

   ```rust
   Dashboard::new("My Dashboard")
       .version(env!("CARGO_PKG_VERSION"))
   ```

   Without a version the hub cannot compare releases and refuses to update it.
2. Know your slug. It is the dashboard name reduced to lower-case letters and
   digits joined by single dashes: `Dashboard::new("Data Explorer")` has the
   slug `data-explorer`. The assets are named with it. The workflow takes the
   binary's name as the slug unless you give `slug`; the two must agree, and
   the workflow checks it by running the built executable.
3. Keep the package version in `Cargo.toml` equal to the tag you will push
   (see [Versions and tags](#versions-and-tags)).

## The signing key

Make the key pair once, on your own machine, with
[minisign](https://jedisct1.github.io/minisign/):

```text
minisign -G -p minisign.pub -s minisign.key
```

It asks for a password; add `-W` for a key without one. rsign2
(`cargo install rsign2`) writes the same files:

```text
rsign generate -p minisign.pub -s minisign.key
```

- Commit `minisign.pub` at the root of the repository. It is public.
- Never commit `minisign.key`. Keep it, and a backup, outside the repository.
  Whoever holds it can publish code that every hub that trusts your key will
  install.
- Users who installed your dashboard pinned the public key. If you lose the
  secret key, a release signed with a new one is refused until each user
  accepts the new key, so keep the backup.

Store the secret key as two repository secrets (Settings, Secrets and
variables, Actions), with the GitHub command line if you like:

| Secret | Content |
| --- | --- |
| `MINISIGN_SECRET_KEY` | The content of the `minisign.key` file. |
| `MINISIGN_PASSWORD` | Its password. Leave it unset or empty for a key made with `-W`. |

```text
gh secret set MINISIGN_SECRET_KEY < minisign.key
gh secret set MINISIGN_PASSWORD
```

The workflow writes the key to a file only the runner's user can read, uses it
once to sign, and deletes it. It then verifies the signature with the
committed `minisign.pub` before it uploads anything, so a secret that does not
match the public key fails the run instead of publishing a release no hub can
verify.

## Calling the reusable workflow

Add `.github/workflows/release.yml` to your repository:

```yaml
name: Release

on:
  push:
    tags: ['v*']

permissions:
  contents: read

jobs:
  release:
    permissions:
      contents: write
    uses: rtonneau/eario/.github/workflows/release-dashboard.yml@<ref>
    with:
      executables: '[{"package": "my-dashboard"}]'
      publish: true
    secrets:
      MINISIGN_SECRET_KEY: ${{ secrets.MINISIGN_SECRET_KEY }}
      MINISIGN_PASSWORD: ${{ secrets.MINISIGN_PASSWORD }}
```

Which `<ref>` to use: a full commit SHA of `rtonneau/eario` that contains the
workflow, so that what runs with your signing key never changes under you.
Once Eario publishes a release tag that contains
`.github/workflows/release-dashboard.yml`, a tag such as `v0.3.0` is fine too.
Do not use a branch name: the called workflow gets your secret key and write
access to your releases. The `v0.2.0` tag of Eario was made before the
workflow existed, so it does not work.

Inputs you will use most:

| Input | Meaning |
| --- | --- |
| `executables` | Required. A JSON array, one object per dashboard: `package` (the Cargo package), and optionally `binary` (default: the package), `slug` (default: the binary) and `probe`. |
| `publish` | `true` publishes the release when every file is attached. `false` (the default) leaves a draft that no hub sees; publish it yourself once it looks right. |
| `public-key` | Path of the committed public key file. Default `minisign.pub`. |
| `attach-public-key` | Attach that file to the release as `minisign.pub`. Default `true`. |
| `targets` | Space-separated target triples. Default: the four above. |
| `node-version`, `before-build` | For a dashboard whose build needs Node.js or a first step, such as `cargo xtask frontend`. |
| `working-directory` | The directory of the Cargo workspace, when it is not the repository root. |

The header of the workflow file lists every input, and what the workflow
refuses before it builds: a tag that is not `v<SemVer version>`, a package
whose version is not the tag's, an unknown target, a slug the hub would refuse,
and a missing or malformed public key file. After building, on the runners
that can run the result, it checks that `--eario-manifest` gives the slug and
the version of the tag. The x86_64 macOS executable is cross-compiled, so it is
not run. macOS builds are not signed or notarized by Apple.

The workflow creates the release from the tag with generated notes. If a draft
for the tag already exists, it adds the files to it. It refuses a release that
is already published.

## Versions and tags

- The tag is `v<version>`, and `<version>` is the package's version in
  `Cargo.toml`, in SemVer form: `v1.2.0`, `v1.2.0-rc.1`. The workflow refuses
  any other tag, or a package whose version is different.
- A tag with a pre-release part (`-rc.1`) makes a pre-release, which hubs do
  not offer. A hub follows the latest published release that is not a
  pre-release.
- The hub updates only to a strictly newer version, by SemVer precedence, and
  the version it compares is the one the new executable prints in its manifest,
  not the tag. A release whose executable is older or equal is refused, so an
  old signed release cannot be passed off as an update. A user goes back to the
  previous version only through Rollback, which keeps one.

## Several dashboards in one repository

One release can carry several dashboards. Give each its own entry in
`executables`:

```yaml
      executables: >-
        [
          {"package": "explorer"},
          {"package": "lab-app", "binary": "lab", "slug": "signal-lab"}
        ]
```

Each dashboard gets its asset for each target (`explorer-<triple>`,
`signal-lab-<triple>`), and one `SHA256SUMS.txt` lists them all, signed once.
Every package must have the tag's version. Slugs must differ.

Users choose one by slug: `eario-hub install owner/repo --slug signal-lab`,
or a catalog entry with a `slug`. Without a slug a release with several
dashboards for the user's machine is refused (`hub_ambiguous_asset`, which
names them). The repository has one key; a user who trusts it trusts it for all
its dashboards.

## What a user does

For a repository that no catalog lists, the user runs `eario-hub install
owner/repo` (or enters it under "Install from GitHub" in the hub view). The hub shows the key from
`minisign.pub` and its SHA-256, and asks the user to accept it before it runs
anything. Publish that SHA-256 where users can compare it, outside the
repository that serves the release (for example on your project page). Print
it with:

```text
sed -n 2p minisign.pub | tr -d '\r\n' | sha256sum
```

It is the SHA-256 of the key line, the second line of the file.

## Getting listed in a catalog

A catalog is a `catalog.json` at the root of the default branch of a public
repository. A user adds it under "Catalogs" in the hub view or with `eario-hub catalog add
owner/repo`. Listing a repository gives its dashboards a place in the hub's
"Catalogs" list and lets the user install without comparing the key by hand,
because the catalog's author vouches for the key.

To be listed, ask the catalog's author to add an entry with your repository,
your slug if you publish several dashboards, and the key line of your
`minisign.pub`:

```text
sed -n 2p minisign.pub
```

The file format is below. The default catalog the hub reads until the user
removes it is `rtonneau/eario-catalog`.

### The catalog format

```json
{
  "format_version": 1,
  "name": "Eario examples",
  "dashboards": [
    {
      "repo": "rtonneau/eario",
      "slug": "data-explorer",
      "name": "Data Explorer",
      "description": "Explore a CSV, TSV or XLSX file.",
      "public_key": "RW..."
    }
  ]
}
```

These are the fields of `eario_core::Catalog` and `CatalogEntry`:

| Field | Required | Meaning |
| --- | --- | --- |
| `format_version` | yes | `1`. A hub refuses a catalog in a newer format (`hub_catalog_unknown_version`). |
| `name` | yes | The catalog's name, shown to the user. |
| `dashboards` | yes | The entries. |
| `repo` | yes | `owner/repo` of the GitHub repository that publishes the dashboard. |
| `slug` | no | The dashboard's slug, for a repository that publishes several. Without it the repository must have one dashboard per release. |
| `name` (entry) | yes | The dashboard's name, shown to the user. |
| `description` | no | One line. |
| `public_key` | yes | The key line of the repository's `minisign.pub` (the second line: 56 characters starting with `RW`), with nothing around it. |

Rules the hub applies:

- The file is read from the repository's default branch, at most 2 MiB. A
  field that is missing or has the wrong type makes the whole catalog
  unreadable, and the hub says so. Fields it does not know are ignored.
- An entry whose `repo` is not `owner/repo`, or whose `slug` is not a slug, is
  skipped with a warning; the others are listed. An entry that repeats a
  repository and a slug is skipped; the first one counts.
- A repository may be listed once per slug. The key belongs to the
  repository: when it is listed under several slugs, the first entry's key is
  used.
- The key is read when a user previews an install from the repository, not when
  the catalog is listed. An entry whose `public_key` is not a key is listed, and
  its install is refused with `release_invalid_key`.
- A catalog key makes the key status `catalog` in the preview. A key pinned for
  an installed repository wins over the catalog's. If a release is signed with
  another key, the status is `changed` and the user must accept the new key by
  its SHA-256.

An example listing `data-explorer` and `signal-lab` from `rtonneau/eario` is
[`docs/catalog/catalog.json`](../catalog/catalog.json). Its `public_key` is a
placeholder, `REPLACE-WITH-THE-KEY-LINE-OF-THE-EARIO-RELEASE-KEY`, which is not
a key: the catalog lists with it, and installs from it are refused until you
replace it with the key line of the Eario release key. A test
(`crates/eario-hub/tests/catalog_example.rs`) reads the file with the same type
the hub uses, so the example cannot drift from the format.
