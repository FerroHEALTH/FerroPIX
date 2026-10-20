// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The spool: bounded, ordered, durable on disk, private to its owner, and
//! never wedged by one message it cannot send.

use std::time::Duration;

use ihe_iti::atna::spool::{Bounds, Depth, QUARANTINE, Spool, SpoolError};
use secrecy::ExposeSecret;

use super::message;

/// A write bound no healthy disk comes near, however busy the host.
const WRITE: Duration = Duration::from_secs(10);

const ROOMY: Bounds = Bounds {
    max_messages: 16,
    max_bytes: 4096,
    write_timeout: WRITE,
};

/// The RFC 5425 frame of `text`.
fn framed(text: &str) -> secrecy::SecretSlice<u8> {
    message(&format!("{} {text}", text.len()))
}

/// The texts of the frames the spool gives up, oldest first, each removed
/// once read.
async fn drain(spool: &Spool) -> Vec<String> {
    let mut texts = Vec::new();
    while let Some(stored) = spool.oldest().await.expect("the spool reads") {
        let frame = String::from_utf8(stored.message.expose_secret().to_vec()).expect("UTF-8");
        let (_, text) = frame.split_once(' ').expect("a frame");
        texts.push(text.to_owned());
        spool
            .remove(stored.sequence)
            .await
            .expect("the spool removes");
    }
    texts
}

/// The name of message `sequence` on disk.
fn named(sequence: u64) -> String {
    format!("{sequence:020}.msg")
}

#[tokio::test]
async fn messages_leave_in_the_order_they_were_stored() {
    let directory = tempfile::tempdir().expect("a directory");
    let spool = Spool::open(&directory.path().join("spool"), ROOMY).expect("the spool opens");
    for text in ["first", "second", "third"] {
        spool.push(framed(text)).await.expect("stored");
    }
    assert_eq!(
        Depth {
            messages: 3,
            bytes: 22,
            quarantined: 0,
        },
        spool.depth()
    );
    assert_eq!(vec!["first", "second", "third"], drain(&spool).await);
    assert_eq!(Depth::default(), spool.depth());
}

#[tokio::test]
async fn a_stored_message_survives_a_restart() {
    let directory = tempfile::tempdir().expect("a directory");
    let path = directory.path().join("spool");
    {
        let spool = Spool::open(&path, ROOMY).expect("the spool opens");
        spool.push(framed("kept")).await.expect("stored");
        spool.push(framed("also kept")).await.expect("stored");
    }
    let reopened = Spool::open(&path, ROOMY).expect("the spool reopens");
    assert_eq!(2, reopened.depth().messages);
    reopened.push(framed("after")).await.expect("stored");
    assert_eq!(vec!["kept", "also kept", "after"], drain(&reopened).await);
}

#[tokio::test]
async fn a_message_past_either_bound_is_refused_and_nothing_is_dropped() {
    let directory = tempfile::tempdir().expect("a directory");
    let by_count = Spool::open(
        &directory.path().join("count"),
        Bounds {
            max_messages: 2,
            max_bytes: 4096,
            write_timeout: WRITE,
        },
    )
    .expect("the spool opens");
    by_count.push(framed("one")).await.expect("stored");
    by_count.push(framed("two")).await.expect("stored");
    assert!(matches!(
        by_count.push(framed("three")).await,
        Err(SpoolError::Full { messages: 2, .. })
    ));
    assert_eq!(vec!["one", "two"], drain(&by_count).await);

    let by_size = Spool::in_memory(Bounds {
        max_messages: 16,
        max_bytes: 8,
        write_timeout: WRITE,
    });
    by_size.push(framed("12345")).await.expect("stored");
    assert!(matches!(
        by_size.push(framed("6789")).await,
        Err(SpoolError::Full { bytes: 7, .. })
    ));
    assert_eq!(vec!["12345"], drain(&by_size).await);
}

#[tokio::test]
async fn a_message_that_is_no_frame_is_quarantined_and_the_next_one_given_up() {
    let directory = tempfile::tempdir().expect("a directory");
    let path = directory.path().join("spool");
    {
        let spool = Spool::open(&path, ROOMY).expect("the spool opens");
        for text in ["first", "second"] {
            spool.push(framed(text)).await.expect("stored");
        }
    }
    std::fs::write(path.join(named(0)), b"torn bytes").expect("a corrupt message");
    let spool = Spool::open(&path, ROOMY).expect("the spool reopens");

    assert_eq!(vec!["second"], drain(&spool).await, "the drain goes on");
    assert!(path.join(QUARANTINE).join(named(0)).is_file());
    assert!(!path.join(named(0)).exists());
    assert_eq!(
        Depth {
            messages: 1,
            bytes: 10,
            quarantined: 1,
        },
        spool.depth(),
        "a quarantined message stays counted"
    );

    let reopened = Spool::open(&path, ROOMY).expect("the spool reopens with its quarantine");
    assert_eq!(1, reopened.depth().quarantined);
    reopened.push(framed("third")).await.expect("stored");
    assert_eq!(
        vec!["third"],
        drain(&reopened).await,
        "a new message never reuses a quarantined sequence"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn a_message_that_cannot_be_read_is_quarantined() {
    use std::os::unix::fs::PermissionsExt as _;

    let directory = tempfile::tempdir().expect("a directory");
    let path = directory.path().join("spool");
    let spool = Spool::open(&path, ROOMY).expect("the spool opens");
    for text in ["unreadable", "readable"] {
        spool.push(framed(text)).await.expect("stored");
    }
    std::fs::set_permissions(path.join(named(0)), std::fs::Permissions::from_mode(0o000))
        .expect("chmod");
    assert_eq!(vec!["readable"], drain(&spool).await);
    assert_eq!(1, spool.depth().quarantined);
}

#[tokio::test]
async fn the_bounds_count_the_quarantine() {
    let directory = tempfile::tempdir().expect("a directory");
    let path = directory.path().join("spool");
    let bounds = Bounds {
        max_messages: 2,
        max_bytes: 4096,
        write_timeout: WRITE,
    };
    {
        let spool = Spool::open(&path, bounds).expect("the spool opens");
        spool.push(framed("doomed")).await.expect("stored");
    }
    std::fs::write(path.join(named(0)), b"garbage").expect("a corrupt message");
    let spool = Spool::open(&path, bounds).expect("the spool reopens");
    assert!(drain(&spool).await.is_empty());
    spool
        .push(framed("one"))
        .await
        .expect("stored beside the quarantine");
    assert!(
        matches!(
            spool.push(framed("two")).await,
            Err(SpoolError::Full { messages: 2, .. })
        ),
        "the quarantined message takes its place under the bound"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn the_directory_and_every_message_are_private_to_their_owner() {
    use std::os::unix::fs::PermissionsExt as _;

    let directory = tempfile::tempdir().expect("a directory");
    let path = directory.path().join("spool");
    let spool = Spool::open(&path, ROOMY).expect("the spool opens");
    spool.push(framed("private")).await.expect("stored");
    let mode = |path: &std::path::Path| {
        std::fs::metadata(path)
            .expect("metadata")
            .permissions()
            .mode()
            & 0o777
    };
    assert_eq!(0o700, mode(&path));
    assert_eq!(0o700, mode(&path.join(QUARANTINE)));
    for entry in std::fs::read_dir(&path).expect("the directory reads") {
        let entry = entry.expect("an entry").path();
        if entry.is_file() {
            assert_eq!(0o600, mode(&entry));
        }
    }

    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o750)).expect("chmod");
    assert!(matches!(
        Spool::open(&path, ROOMY),
        Err(SpoolError::Exposed(_))
    ));
}

#[cfg(unix)]
#[test]
fn a_directory_the_owner_cannot_write_is_refused_when_opened() {
    use std::os::unix::fs::PermissionsExt as _;

    let directory = tempfile::tempdir().expect("a directory");
    let path = directory.path().join("spool");
    drop(Spool::open(&path, ROOMY).expect("the spool opens"));
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o500)).expect("chmod");
    let refused = Spool::open(&path, ROOMY);
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).expect("chmod");
    assert!(
        matches!(
            refused,
            Err(SpoolError::Io {
                action: "created",
                ..
            })
        ),
        "{refused:?}"
    );
}

#[tokio::test]
async fn a_file_the_spool_did_not_write_refuses_the_start_naming_it() {
    let directory = tempfile::tempdir().expect("a directory");
    let path = directory.path().join("spool");
    let spool = Spool::open(&path, ROOMY).expect("the spool opens");
    spool.push(framed("whole")).await.expect("stored");
    drop(spool);
    std::fs::write(path.join("00000000000000000007.partial"), b"torn").expect("a partial");
    let reopened = Spool::open(&path, ROOMY).expect("the spool reopens");
    assert_eq!(vec!["whole"], drain(&reopened).await);

    let stray = path.join("notes.txt");
    std::fs::write(&stray, b"not a message").expect("a stray file");
    match Spool::open(&path, ROOMY) {
        Err(error @ SpoolError::Foreign { .. }) => {
            let text = error.to_string();
            assert!(text.contains(&stray.display().to_string()), "{text}");
            assert!(text.contains("move that file out"), "{text}");
        }
        other => panic!("a foreign file refuses the start: {other:?}"),
    }
}

/// The names in `directory`, sorted, or none when it does not exist.
fn listing(directory: &std::path::Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .map(|entry| {
            entry
                .expect("an entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    names
}

#[test]
fn inspecting_a_missing_spool_under_a_writable_parent_creates_nothing() {
    let directory = tempfile::tempdir().expect("a directory");
    let path = directory.path().join("spool");
    Spool::inspect(&path).expect("the spool would open");
    assert!(!path.exists());
    assert!(listing(directory.path()).is_empty());
}

#[test]
fn inspecting_a_spool_whose_parents_are_missing_reads_the_nearest_ancestor() {
    let directory = tempfile::tempdir().expect("a directory");
    let path = directory.path().join("missing").join("spool");
    Spool::inspect(&path).expect("opening creates the missing parents");
    assert!(listing(directory.path()).is_empty());

    let file = directory.path().join("a-file");
    std::fs::write(&file, b"a file").expect("a file");
    match Spool::inspect(&file.join("below").join("spool")) {
        Err(error @ SpoolError::NotADirectory(_)) => {
            assert!(error.to_string().contains(&file.display().to_string()));
        }
        other => panic!("a file above the spool is refused: {other:?}"),
    }
    assert_eq!(vec![String::from("a-file")], listing(directory.path()));
}

#[tokio::test]
async fn inspecting_a_spool_in_use_changes_nothing_in_it() {
    let directory = tempfile::tempdir().expect("a directory");
    let path = directory.path().join("spool");
    let spool = Spool::open(&path, ROOMY).expect("the spool opens");
    spool.push(framed("kept")).await.expect("stored");
    drop(spool);
    std::fs::write(path.join("00000000000000000007.partial"), b"torn").expect("a partial");
    let before = listing(&path);
    Spool::inspect(&path).expect("the spool would open");
    assert_eq!(before, listing(&path));
    assert!(before.contains(&String::from("00000000000000000007.partial")));
}

#[test]
fn inspecting_a_spool_with_a_foreign_file_names_it() {
    let directory = tempfile::tempdir().expect("a directory");
    let path = directory.path().join("spool");
    drop(Spool::open(&path, ROOMY).expect("the spool opens"));
    let stray = path.join(QUARANTINE).join("notes.txt");
    std::fs::write(&stray, b"not a message").expect("a stray file");
    match Spool::inspect(&path) {
        Err(error @ SpoolError::Foreign { .. }) => {
            assert!(error.to_string().contains(&stray.display().to_string()));
        }
        other => panic!("a foreign file is refused: {other:?}"),
    }
}

#[test]
fn inspecting_a_file_in_place_of_the_spool_is_refused() {
    let directory = tempfile::tempdir().expect("a directory");
    let path = directory.path().join("spool");
    std::fs::write(&path, b"a file").expect("a file");
    assert!(matches!(
        Spool::inspect(&path),
        Err(SpoolError::NotADirectory(refused)) if refused == path
    ));
}

#[cfg(unix)]
#[test]
fn inspecting_refuses_what_opening_refuses_by_mode() {
    use std::os::unix::fs::PermissionsExt as _;

    let directory = tempfile::tempdir().expect("a directory");
    let path = directory.path().join("spool");
    drop(Spool::open(&path, ROOMY).expect("the spool opens"));
    let chmod = |path: &std::path::Path, mode: u32| {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).expect("chmod");
    };

    chmod(&path, 0o750);
    assert!(matches!(Spool::inspect(&path), Err(SpoolError::Exposed(_))));
    chmod(&path, 0o500);
    let unwritable = Spool::inspect(&path);
    chmod(&path, 0o700);
    assert!(
        matches!(&unwritable, Err(SpoolError::Unwritable(refused)) if *refused == path),
        "{unwritable:?}"
    );

    let parent = directory.path().join("closed");
    std::fs::create_dir_all(&parent).expect("a parent");
    chmod(&parent, 0o555);
    let refused = Spool::inspect(&parent.join("missing").join("spool"));
    chmod(&parent, 0o700);
    assert!(
        matches!(&refused, Err(SpoolError::Unwritable(closed)) if *closed == parent),
        "{refused:?}"
    );
}
