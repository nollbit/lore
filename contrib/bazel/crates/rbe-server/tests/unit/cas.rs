// SPDX-FileCopyrightText: 2026 Epic Games, Inc.
// SPDX-License-Identifier: MIT
use rbe_lore::digest;
use rbe_proto::reapi::compressor;
use rbe_server::cas::*;

const H: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn action_cache_misses_for_unverifiable_directory_entries() {
    use lore_base::test_util::TempDir;
    use rbe_lore::LoreBlobStore;
    use rbe_proto::reapi::ActionResult;
    use rbe_proto::reapi::Digest;
    use rbe_proto::reapi::OutputDirectory;
    use rbe_proto::reapi::OutputFile;

    let temporary = TempDir::new("rbe-invalid-outputs-");
    let store = LoreBlobStore::open(temporary.path().to_str().unwrap(), 0, None)
        .await
        .unwrap();
    let action = digest::of(b"invalid-action");
    for result in [
        ActionResult {
            output_files: vec![OutputFile {
                path: "missing-digest".into(),
                ..Default::default()
            }],
            ..Default::default()
        },
        ActionResult {
            output_directories: vec![OutputDirectory {
                path: "missing-tree".into(),
                ..Default::default()
            }],
            ..Default::default()
        },
        ActionResult {
            output_directories: vec![OutputDirectory {
                path: "directory-only".into(),
                root_directory_digest: Some(digest::of(b"")),
                ..Default::default()
            }],
            ..Default::default()
        },
        ActionResult {
            output_directories: vec![OutputDirectory {
                path: "oversized-tree".into(),
                tree_digest: Some(Digest {
                    hash: H.into(),
                    size_bytes: 16 * 1024 * 1024 + 1,
                }),
                ..Default::default()
            }],
            ..Default::default()
        },
    ] {
        rbe_server::ac::store_result(&store, &action, &result)
            .await
            .unwrap();
        assert!(
            rbe_server::ac::lookup(&store, &action, true)
                .await
                .is_none()
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn action_cache_misses_when_a_tree_file_is_missing() {
    use lore_base::test_util::TempDir;
    use prost::Message;
    use rbe_lore::LoreBlobStore;
    use rbe_lore::Ns;
    use rbe_proto::reapi::ActionResult;
    use rbe_proto::reapi::Digest;
    use rbe_proto::reapi::Directory;
    use rbe_proto::reapi::FileNode;
    use rbe_proto::reapi::OutputDirectory;
    use rbe_proto::reapi::Tree;

    let temporary = TempDir::new("rbe-tree-cache-");
    let store = LoreBlobStore::open(temporary.path().to_str().unwrap(), 0, None)
        .await
        .unwrap();
    let payload = b"missing-file-data";
    let file_digest = digest::of(payload);
    let tree = Tree {
        root: Some(Directory {
            files: vec![FileNode {
                name: "missing.bin".into(),
                digest: Some(file_digest.clone()),
                ..Default::default()
            }],
            ..Default::default()
        }),
        ..Default::default()
    };
    let bytes = tree.encode_to_vec();
    let tree_digest = Digest {
        hash: digest::sha256_hex(&bytes),
        size_bytes: bytes.len() as i64,
    };
    store
        .put(Ns::Cas, &tree_digest.hash, tree_digest.size_bytes, &bytes)
        .await
        .unwrap();
    let action_digest = Digest {
        hash: digest::sha256_hex(b"action"),
        size_bytes: 6,
    };
    let result = ActionResult {
        output_directories: vec![OutputDirectory {
            path: "out".into(),
            tree_digest: Some(tree_digest),
            ..Default::default()
        }],
        ..Default::default()
    };
    rbe_server::ac::store_result(&store, &action_digest, &result)
        .await
        .unwrap();
    assert!(
        rbe_server::ac::lookup(&store, &action_digest, true)
            .await
            .is_none()
    );
    store
        .put(Ns::Cas, &file_digest.hash, file_digest.size_bytes, payload)
        .await
        .unwrap();
    assert!(
        rbe_server::ac::lookup(&store, &action_digest, true)
            .await
            .is_some()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn action_cache_verifies_nested_tree_outputs() {
    use lore_base::test_util::TempDir;
    use prost::Message;
    use rbe_lore::LoreBlobStore;
    use rbe_lore::Ns;
    use rbe_proto::reapi::ActionResult;
    use rbe_proto::reapi::Directory;
    use rbe_proto::reapi::DirectoryNode;
    use rbe_proto::reapi::FileNode;
    use rbe_proto::reapi::OutputDirectory;
    use rbe_proto::reapi::Tree;

    let temporary = TempDir::new("rbe-nested-tree-");
    let store = LoreBlobStore::open(temporary.path().to_str().unwrap(), 0, None)
        .await
        .unwrap();
    let payload = b"nested-file";
    let file_digest = digest::of(payload);
    let child = Directory {
        files: vec![FileNode {
            name: "file.bin".into(),
            digest: Some(file_digest.clone()),
            ..Default::default()
        }],
        ..Default::default()
    };
    let root = Directory {
        directories: vec![DirectoryNode {
            name: "child".into(),
            digest: Some(digest::of(&child.encode_to_vec())),
        }],
        ..Default::default()
    };
    let action = digest::of(b"nested-action");
    for (children, expected) in [(vec![], false), (vec![child.clone()], false)] {
        let bytes = Tree {
            root: Some(root.clone()),
            children,
        }
        .encode_to_vec();
        let tree = digest::of(&bytes);
        store
            .put(Ns::Cas, &tree.hash, tree.size_bytes, &bytes)
            .await
            .unwrap();
        let result = ActionResult {
            output_directories: vec![OutputDirectory {
                path: "out".into(),
                tree_digest: Some(tree),
                ..Default::default()
            }],
            ..Default::default()
        };
        rbe_server::ac::store_result(&store, &action, &result)
            .await
            .unwrap();
        assert_eq!(
            rbe_server::ac::lookup(&store, &action, true)
                .await
                .is_some(),
            expected
        );
    }
    store
        .put(Ns::Cas, &file_digest.hash, file_digest.size_bytes, payload)
        .await
        .unwrap();
    assert!(
        rbe_server::ac::lookup(&store, &action, true)
            .await
            .is_some()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn action_cache_rejects_malformed_trees_and_accepts_empty_directories() {
    use lore_base::test_util::TempDir;
    use prost::Message;
    use rbe_lore::LoreBlobStore;
    use rbe_lore::Ns;
    use rbe_proto::reapi::ActionResult;
    use rbe_proto::reapi::Directory;
    use rbe_proto::reapi::OutputDirectory;
    use rbe_proto::reapi::Tree;

    let temporary = TempDir::new("rbe-malformed-tree-");
    let store = LoreBlobStore::open(temporary.path().to_str().unwrap(), 0, None)
        .await
        .unwrap();
    let action = digest::of(b"malformed-action");
    for (bytes, expected) in [
        (b"not a protobuf tree".to_vec(), false),
        (Tree::default().encode_to_vec(), false),
        (
            Tree {
                root: Some(Directory::default()),
                ..Default::default()
            }
            .encode_to_vec(),
            true,
        ),
    ] {
        let tree = digest::of(&bytes);
        store
            .put(Ns::Cas, &tree.hash, tree.size_bytes, &bytes)
            .await
            .unwrap();
        let result = ActionResult {
            output_directories: vec![OutputDirectory {
                path: "out".into(),
                tree_digest: Some(tree),
                ..Default::default()
            }],
            ..Default::default()
        };
        rbe_server::ac::store_result(&store, &action, &result)
            .await
            .unwrap();
        assert_eq!(
            rbe_server::ac::lookup(&store, &action, true)
                .await
                .is_some(),
            expected
        );
    }
}

#[test]
fn parses_plain_and_compressed_resources() {
    let r = parse_resource(&format!("blobs/{H}/42")).unwrap();
    assert_eq!((r.hash.as_str(), r.size, r.codec), (H, 42, Codec::Identity));

    let r = parse_resource(&format!("inst/uploads/u-1/blobs/sha256/{H}/7/meta")).unwrap();
    assert_eq!((r.size, r.codec), (7, Codec::Identity));

    let r = parse_resource(&format!("compressed-blobs/zstd/{H}/1000")).unwrap();
    assert_eq!((r.hash.as_str(), r.size, r.codec), (H, 1000, Codec::Zstd));

    let r = parse_resource(&format!("inst/uploads/u-2/compressed-blobs/zstd/{H}/5/x")).unwrap();
    assert_eq!((r.size, r.codec), (5, Codec::Zstd));
}

#[test]
fn rejects_compressors_it_does_not_offer() {
    let err = parse_resource(&format!("compressed-blobs/deflate/{H}/5")).unwrap_err();
    assert_eq!(err.code(), tonic::Code::Unimplemented);
    let err = parse_resource("compressed-blobs").unwrap_err();
    assert_eq!(err.code(), tonic::Code::InvalidArgument);
    assert!(Codec::from_proto(compressor::Value::Deflate as i32).is_err());
}

#[test]
fn zstd_round_trips_and_is_capped_at_the_declared_size() {
    let blob = b"cc -Iexternal/a -Iexternal/b -c foo.cc -o foo.o ".repeat(200);
    let wire = Codec::Zstd.encode(blob.clone()).unwrap();
    assert!(wire.len() < blob.len() / 10);
    assert_eq!(
        Codec::Zstd.decode(&wire, blob.len() as i64).unwrap(),
        &blob[..]
    );
    // A frame that expands past what the digest declares is refused, not allocated.
    assert!(Codec::Zstd.decode(&wire, blob.len() as i64 - 1).is_err());
    assert!(Codec::Zstd.decode(b"not zstd", 100).is_err());
    assert_eq!(Codec::Identity.decode(b"abc", 3).unwrap(), &b"abc"[..]);
}

#[tokio::test]
async fn codec_gives_the_same_answer_on_and_off_the_runtime() {
    // One blob under the blocking-pool threshold and one over it.
    for blob in [
        b"small blob ".repeat(10),
        b"large blob ".repeat(OFF_RUNTIME_BYTES),
    ] {
        let size = blob.len() as i64;
        let wire = Codec::Zstd.encode_async(blob.clone()).await.unwrap();
        let (back, hash) = Codec::Zstd.decode_and_hash(wire, size).await.unwrap();
        assert_eq!(
            (back.as_slice(), hash.as_str()),
            (&blob[..], digest::sha256_hex(&blob).as_str())
        );
        let (same, _) = Codec::Identity
            .decode_and_hash(blob.clone(), size)
            .await
            .unwrap();
        assert_eq!(same, blob);
        let err = Codec::Zstd
            .decode_and_hash(b"not zstd".to_vec(), size)
            .await
            .unwrap_err();
        assert_eq!(err.code(), tonic::Code::InvalidArgument);
    }
}
