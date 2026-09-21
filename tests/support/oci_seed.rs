//! Gate helper, never shipped: put one image into a registry volume through
//! the store adapter, so the OCI conformance suite's pull category has content
//! to read before the push routes exist. Once push lands (P4) the suite seeds
//! the registry itself and this helper goes.
//!
//! Usage: `oci_seed <volume> <repository> <tag>`, with no server running on
//! the volume. It prints the three variables the suite reads, one per line, in
//! the form `$GITHUB_ENV` takes.

use hologram_live::oci_store::{
    Digest, LinkKind, ManifestPlan, OciStore, OpenOptions, Reference, RepoName,
};
use std::time::Duration;

const MANIFEST_TYPE: &str = "application/vnd.oci.image.manifest.v1+json";

fn push(store: &OciStore, repo: &RepoName, bytes: &[u8]) -> Digest {
    let id = store.upload_begin(repo).expect("begin");
    store.upload_append(&id, 0, bytes).expect("append");
    store
        .upload_finish(&id, &Digest::sha256_of(bytes))
        .expect("finish")
}

fn main() {
    let mut args = std::env::args().skip(1);
    let usage = "usage: oci_seed <volume> <repository> <tag>";
    let root = args.next().expect(usage);
    let repo = RepoName::parse(&args.next().expect(usage)).expect("repository name");
    let tag = args.next().expect(usage);
    let options = OpenOptions {
        create: true,
        upload_max_age: Duration::from_hours(1),
    };
    let store = OciStore::open(std::path::Path::new(&root), options).expect("open");

    let config =
        br#"{"architecture":"amd64","os":"linux","rootfs":{"type":"layers","diff_ids":[]}}"#;
    let layer = b"hologram registry conformance layer";
    let config_digest = push(&store, &repo, config);
    let layer_digest = push(&store, &repo, layer);
    let manifest = format!(
        r#"{{"schemaVersion":2,"mediaType":"{MANIFEST_TYPE}","config":{{"mediaType":"application/vnd.oci.image.config.v1+json","digest":"{config_digest}","size":{}}},"layers":[{{"mediaType":"application/vnd.oci.image.layer.v1.tar+gzip","digest":"{layer_digest}","size":{}}}]}}"#,
        config.len(),
        layer.len()
    );
    let plan = ManifestPlan {
        kind: LinkKind::Manifest,
        must_exist: vec![config_digest.clone(), layer_digest],
        subject: None,
    };
    let manifest_digest = store
        .manifest_put(
            &repo,
            &Reference::parse(&tag).expect("tag"),
            MANIFEST_TYPE,
            manifest.as_bytes(),
            &plan,
        )
        .expect("manifest");

    println!("OCI_TAG_NAME={tag}");
    println!("OCI_BLOB_DIGEST={config_digest}");
    println!("OCI_MANIFEST_DIGEST={manifest_digest}");
}
