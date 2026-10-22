# Publishing and Distribution

This guide creates a signed static repository. YAML extensions declare their own identity, so
`publish` reads it from the file. WASM components cannot: reading a component's metadata means
instantiating it against async host imports, which `kani-cli` has no runtime for, so their
identity is supplied on the command line.

## Keys

A maintainer key signs the repository index. An author key signs an extension artifact. A sole
maintainer may use one key for both roles; a multi-author repository should keep them separate.

Generate named keypairs:

```bash
cargo run -p kani-cli -- keygen --out-dir ./keys --name maintainer
cargo run -p kani-cli -- keygen --out-dir ./keys --name author
```

Each command writes `<name>.pub` and `<name>.key`. The private file is a plaintext base64 Ed25519
seed. On Unix the CLI sets mode 0600, but it is not passphrase-encrypted. Keep it out of version
control, restrict backups, and do not put it in a public static repository.

Publish the maintainer fingerprint alongside the repository URL:

```bash
cargo run -p kani-cli -- repo show-fingerprint --key ./keys/maintainer.pub
```

## Initialize a repository

```bash
mkdir my-repo
cargo run -p kani-cli -- repo init \
  --repo-dir ./my-repo \
  --maintainer-key ./keys/maintainer.pub \
  --name "My Extensions"
```

Initialization creates `index.json` and `extensions/`. It does not create `index.json.sig`; the
first publish with a maintainer signing key does that.

## Publish a YAML extension

```bash
cargo run -p kani-cli -- publish ./my-source.yaml \
  --repo-dir ./my-repo \
  --sign-key ./keys/author.key \
  --repo-sign-key ./keys/maintainer.key \
  --min-kani-version <minimum-host-version>
```

The CLI validates YAML, hashes and signs the exact artifact bytes, copies the artifact and
signature into the repository, upserts its index entry, and signs the new index. Omit
`--min-kani-version` only after checking the CLI's current default behavior; an authoring build may
seed it from its own package version.

Publishing the same extension ID replaces its catalogue entry and writes the artifact under its
declared semantic version. Bump the YAML version before publishing an update.

## Publish a WASM component

A `.wasm` artifact needs `--ext-id`, `--ext-name` and `--ext-version`, plus optional
`--ext-description`, `--ext-language` and `--ext-nsfw`:

```bash
cargo run -p kani-cli -- publish ./kani-my-source.wasm \
  --ext-id my-source \
  --ext-name "My Source" \
  --ext-version 1.2.0 \
  --sign-key ./author.key \
  --repo-dir ./repo \
  --repo-sign-key ./maintainer.key
```

**These values must match the metadata the extension reports.** The server re-reads metadata from
the component at install time and registers the source under *that* identity, so a mismatch leaves
the catalogue describing something the server did not install — update detection compares the
index version against the installed one, and will misbehave. `publish` checks that `--ext-id`
appears somewhere in the binary and refuses if it does not, which catches a typo but cannot prove
the value is right. `--ext-version` must be valid semver for the same reason.

## Add an already signed YAML artifact

`repo add` accepts a YAML artifact whose `<filename>.sig` is beside it and verifies that signature
with the supplied author public key before adding it:

```bash
cargo run -p kani-cli -- repo add ./my-source.yaml \
  --repo-dir ./my-repo \
  --author-key ./keys/author.pub \
  --repo-sign-key ./keys/maintainer.key
```

## Repository layout

```text
my-repo/
├── index.json
├── index.json.sig
└── extensions/
    └── my-source/
        └── <extension-version>/
            ├── extension.yaml
            └── extension.yaml.sig
```

An index entry contains extension ID, name, version, `yaml` format, optional description/language
and content rating, optional minimum Kani version, relative artifact URL, SHA-256 digest, artifact
signature, and author public key. `index.json.sig` is the maintainer signature over the exact UTF-8
index bytes.

## Verify before deployment

```bash
cargo run -p kani-cli -- repo list --repo-dir ./my-repo
cargo run -p kani-cli -- repo verify \
  --repo-dir ./my-repo \
  --repo-key ./keys/maintainer.pub
```

Verification checks the index signature and every local artifact's digest and author signature.
It exits nonzero on failure and should run in CI after publication.

## Host the repository

Serve the directory as immutable or carefully invalidated static HTTPS content. The URL added to
Kani is the repository base from which `index.json`, `index.json.sig`, and relative artifact paths
resolve.

Use a deployment job that receives private keys from a secret store, publishes into a clean output
directory, verifies it, and uploads only public keys, signatures, the index, and artifacts. Never
deploy the workspace directory containing private keys.

## Rotation and removal

- For an author-key rotation, publish new versions signed by the new author and announce the
  change. Existing artifacts retain their historical signature.
- A maintainer-key rotation changes the TOFU trust anchor. Users receive a pinned-key conflict and
  must verify the new fingerprint out of band before trusting it.
- Removing an entry prevents new discovery but does not uninstall existing copies. Communicate
  deprecation before removal.
- If a key is compromised, preserve the affected index and artifacts for investigation, publish a
  signed incident notice through an independent channel, and do not tell users to bypass
  verification.
