//! P0 spike: the Kappa store, driven the way the registry adapter will drive it.
//! Not product code. Every test answers one question in the P0 verdict.

use kappa_core::clock::Clock;
use kappa_core::KappaStore;
use kappa_store_redb::{PersistentStore, PersistentStoreConfig};
use sha2::{Digest, Sha256};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

const MIB: usize = 1024 * 1024;
const FRAME: usize = 4 * MIB;

struct WallClock;

impl Clock for WallClock {
    fn now_ms(&self) -> u64 {
        let elapsed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after 1970");
        u64::try_from(elapsed.as_millis()).expect("milliseconds fit in u64")
    }
}

fn open(root: &Path) -> PersistentStore {
    let config =
        PersistentStoreConfig::new(root.join("kappa/blobs"), root.join("kappa/kappa.redb"));
    PersistentStore::new(config, Arc::new(WallClock)).expect("open store")
}

/// Deterministic bytes that do not compress, produced without holding them all.
fn frame(index: u64) -> Vec<u8> {
    let mut out = Vec::with_capacity(FRAME);
    let mut block = blake3::Hasher::new();
    block.update(&index.to_le_bytes());
    let mut reader = block.finalize_xof();
    out.resize(FRAME, 0);
    reader.fill(&mut out);
    out
}

fn stream_in(store: &PersistentStore, repo: &str, frames: u64) -> (String, String) {
    let namespace = store
        .namespace_resolve_or_create(repo, "spike", Some("oci"))
        .expect("namespace");
    let id = store.upload_begin(&namespace, 0).expect("begin");
    let mut sha = Sha256::new();
    let mut offset = 0_u64;
    for index in 0..frames {
        let bytes = frame(index);
        sha.update(&bytes);
        offset = store.upload_put_part(&id, offset, &bytes).expect("part");
    }
    let digest = format!("sha256:{:x}", sha.finalize());
    (id, digest)
}

#[test]
fn q1_a_64_mib_stream_lands_under_its_sha256() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open(dir.path());
    let (id, digest) = stream_in(&store, "library/alpine", 16);

    let result = store.upload_complete(&id, Some(&digest)).expect("complete");

    assert_eq!(
        result.kappa, digest,
        "the store's address is the client's sha256"
    );
    assert!(store.blob_exists(&digest).expect("exists"));
    assert_eq!(store.blob_size(&digest).expect("size"), (16 * FRAME) as u64);
}

#[test]
fn q2_a_range_read_seeks_without_reading_the_whole_blob() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open(dir.path());
    let (id, digest) = stream_in(&store, "library/alpine", 16);
    store.upload_complete(&id, Some(&digest)).expect("complete");

    let mut reader = store.blob_open(&digest).expect("open");
    reader.seek(SeekFrom::Start(10 * MIB as u64)).expect("seek");
    let mut got = vec![0_u8; MIB];
    reader.read_exact(&mut got).expect("read");

    // 10 MiB into the stream is 2 MiB into frame 2.
    let expected = &frame(2)[2 * MIB..3 * MIB];
    assert_eq!(got, expected);
}

#[test]
fn q3_a_wrong_digest_is_refused_and_leaves_nothing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open(dir.path());
    let (id, real) = stream_in(&store, "library/alpine", 1);
    let wrong = format!("sha256:{}", "0".repeat(64));

    let outcome = store.upload_complete(&id, Some(&wrong));

    assert!(outcome.is_err(), "hash on write: a lie is refused");
    assert!(!store.blob_exists(&wrong).expect("exists"));
    assert!(
        !store.blob_exists(&real).expect("exists"),
        "nothing reachable is left behind"
    );
}

#[test]
fn q4_tags_are_scoped_to_a_repository() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open(dir.path());
    let (id, digest) = stream_in(&store, "team/tags/app", 1);
    store.upload_complete(&id, Some(&digest)).expect("complete");
    let here = store
        .namespace_resolve_or_create("team/tags/app", "spike", Some("oci"))
        .expect("ns");
    let there = store
        .namespace_resolve_or_create("other/app", "spike", Some("oci"))
        .expect("ns");

    store.tag_set(&here, "latest", &digest).expect("tag");

    assert_eq!(store.tag_get(&here, "latest").expect("get").kappa, digest);
    assert!(
        store.tag_get(&there, "latest").is_err(),
        "a tag does not leak across repositories"
    );
    let names: Vec<String> = store
        .tag_list(&here)
        .expect("list")
        .into_iter()
        .map(|t| t.name)
        .collect();
    assert_eq!(names, ["latest"]);
}

#[test]
fn q5_the_store_does_not_make_a_blake3_address_for_a_sha256_upload() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open(dir.path());
    let mut blake = blake3::Hasher::new();
    blake.update(&frame(0));
    let blake_digest = format!("blake3:{}", blake.finalize().to_hex());
    let (id, digest) = stream_in(&store, "library/alpine", 1);
    let result = store.upload_complete(&id, Some(&digest)).expect("complete");

    // Records the fact ADR-030 stands on. If this starts failing, upstream
    // changed the default axes and the alias table can be simplified.
    assert!(result
        .additional_kappas
        .iter()
        .all(|k| !k.starts_with("blake3:")));
    assert!(!store.blob_exists(&blake_digest).expect("exists"));
}

#[test]
fn q6_what_survives_a_restart() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (finished, half_done) = {
        let store = open(dir.path());
        let (id, digest) = stream_in(&store, "library/alpine", 2);
        store.upload_complete(&id, Some(&digest)).expect("complete");
        let namespace = store
            .namespace_resolve_or_create("library/alpine", "spike", Some("oci"))
            .expect("ns");
        store.tag_set(&namespace, "v1", &digest).expect("tag");
        let (open_id, _) = stream_in(&store, "library/alpine", 1);
        (digest, open_id)
    }; // store dropped: the redb lock is released

    let store = open(dir.path());
    let namespace = store
        .namespace_resolve_or_create("library/alpine", "spike", Some("oci"))
        .expect("ns");

    assert!(
        store.blob_exists(&finished).expect("exists"),
        "finished blobs survive"
    );
    assert_eq!(
        store.tag_get(&namespace, "v1").expect("tag").kappa,
        finished,
        "tags survive"
    );
    // Today this is None: sessions are an in-memory map and staging is wiped
    // at open (research K5). The verdict records the observed value; patch
    // 0003 in P1 T6 turns it into Some(4 MiB).
    println!(
        "VERDICT q6 half-done upload after restart: {:?}",
        store.upload_bytes_received(&half_done)
    );
}

#[test]
fn q7_a_second_opener_fails_fast() {
    let dir = tempfile::tempdir().expect("tempdir");
    let _first = open(dir.path());
    let started = std::time::Instant::now();
    let config = PersistentStoreConfig::new(
        dir.path().join("kappa/blobs"),
        dir.path().join("kappa/kappa.redb"),
    );

    let second = PersistentStore::new(config, Arc::new(WallClock));

    assert!(
        second.is_err(),
        "garbage collection relies on this to refuse beside a live server"
    );
    assert!(started.elapsed().as_secs() < 2, "it must fail, not wait");
}

/// Run alone, in release, under a memory meter. See Step 5.
#[test]
#[ignore = "2 GiB; run by hand and by the CI job"]
fn q8_two_gib_streams_with_flat_memory() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open(dir.path());
    let started = std::time::Instant::now();
    let (id, digest) = stream_in(&store, "big/layer", 512);
    let streamed = started.elapsed();
    store.upload_complete(&id, Some(&digest)).expect("complete");
    println!(
        "VERDICT q8 stream {streamed:?}, complete {:?}",
        started.elapsed() - streamed
    );
}
