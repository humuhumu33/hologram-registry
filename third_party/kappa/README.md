# Kappa store pin

Spike record (P0). The registry code will sit behind the cargo feature `oci`, off by default.

| | |
|---|---|
| Upstream | https://github.com/UOR-Foundation/kappa-registry |
| Route that worked | the plain upstream git dependency, pinned by `rev`. No fork was needed to build |
| Pin revision (must equal Cargo.lock) | 2af86560a177fc9651b6c0e92e7974140ed77dd5 |
| Crates used | kappa-core, kappa-store-redb. Nothing else from the workspace |

The plan expected the plain dependency to fail, because upstream's workspace root declares `[[test]]` in a virtual
manifest. It does not fail: cargo 1.97.1 reads only the member manifests it needs when the workspace is a git
dependency. The `[[test]]` section only breaks a build of the upstream workspace itself.

## Carried patches

None yet. A fork becomes necessary in P1 for the fixes below; it was not needed for the spike.

| Patch | What | Why | Upstream | State |
|---|---|---|---|---|
| (planned) optional-aead | put `rekindle-aead` (blob encryption at rest) behind a feature in `kappa-core` and `kappa-store-redb` | it pulls `aws-lc-rs` and `aws-lc-sys`, which this repository keeps out on purpose (`rustls` with the `ring` provider). Used in two files only: `kappa-core/src/crypto/aead.rs`, `kappa-store-redb/src/encrypted.rs` | not opened | required before P1 merges to the parent |
| (planned) durable-upload-sessions | persist upload sessions, stop wiping staging at open | FR-006: an interrupted upload resumes across a restart | not opened | P1 T6 |
| (planned) fsync-before-rename | sync blob data before the rename that publishes it | power loss | not opened | before a release |
| (planned) optional-codecs | put `zstd`, `xz2`, `bzip2` behind a feature | the registry path does not use them; they add `lzma-sys` and `bzip2-sys` | not opened | nice to have |

## Fork-branch dependencies

Upstream names two dependencies by branch on personal forks. `Cargo.lock` fixes both to a commit, so a `--locked`
build is reproducible while those commits stay fetchable. If a branch is deleted, the fetch fails even with the
lock; that is why P1 moves to a fork that names them by `rev` on mirrors we control.

| Crate | Upstream | Locked commit | Our mirror |
|---|---|---|---|
| dcbor, dcbor-derive | https://github.com/usrbinkat/bc-dcbor-rust (branch `feat/dcbor-derive`) | 2e5b901e8c9946794c5491cf92eeec2540b84261 | not yet |
| rekindle-aead | https://github.com/ScopeCreep-zip/rekindle (branch `feat/cli-tui-restructure-rewrite`) | 3fb5b80f2d5d3d5b5a59dded1a56b477cb9f23ca | not yet |
