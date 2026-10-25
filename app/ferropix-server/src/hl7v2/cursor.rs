// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The continuation pointers of ITI-21: where the next increment of a
//! query's answer starts.
//!
//! An answer that holds fewer records than match ends with a DSC whose
//! continuation pointer the Consumer echoes to ask for the next increment
//! (ITI TF-2 §3.21.4.2.2.7), and a cancel (QCN^J01) names the query by its
//! tag (§3.21.4.3.2.2). How long a Supplier keeps a query's place "is an
//! implementation decision" (§3.21.4.3): a pointer lives [`LIFETIME`], is
//! removed when its last increment is sent, when the query is cancelled or
//! when it is asked for after its lifetime, and at most [`CAPACITY`]
//! pointers are kept, the oldest given up first. The pointer holds the
//! search, never a value of the answer. No specification governs these
//! bounds: our own design.

use std::collections::BTreeMap;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use secrecy::SecretString;
use uuid::Uuid;

/// How long a continuation pointer may be asked for.
pub const LIFETIME: Duration = Duration::from_secs(600);

/// The most continuation pointers kept at once.
pub const CAPACITY: usize = 1024;

/// Where the next increment of one query starts.
#[derive(Clone)]
pub struct Place {
    /// The sender the query came from, as MSH-3 and MSH-4 name it.
    pub sender: String,
    /// QPD-2, the query tag.
    pub tag: String,
    /// The ITI-78 search the query was read as, without `_count`.
    pub search: SecretString,
    /// Whether QPD-8 named the domains returned.
    pub filtered: bool,
    /// The last person of the increments sent.
    pub after: Uuid,
    /// RCP-2, the records an increment holds.
    pub increment: u32,
}

impl std::fmt::Debug for Place {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Place")
            .field("increment", &self.increment)
            .finish_non_exhaustive()
    }
}

/// One kept pointer.
#[derive(Debug, Clone)]
struct Kept {
    /// The place.
    place: Place,
    /// When the pointer was handed out.
    since: Instant,
}

/// The continuation pointers of the face.
#[derive(Debug)]
pub struct Cursors {
    /// The pointers, by their value.
    kept: Mutex<BTreeMap<String, Kept>>,
    /// How long a pointer may be asked for.
    lifetime: Duration,
}

impl Default for Cursors {
    fn default() -> Self {
        Self::with_lifetime(LIFETIME)
    }
}

impl Cursors {
    /// Returns the pointers, each asked for at most `lifetime` after it was
    /// handed out.
    #[must_use]
    pub fn with_lifetime(lifetime: Duration) -> Self {
        Self {
            kept: Mutex::new(BTreeMap::new()),
            lifetime,
        }
    }

    /// Keeps `place` and returns its pointer: 32 hexadecimal digits of a
    /// random UUID, the "unique alphanumeric value" of §3.21.4.2.2.7.
    pub fn keep(&self, place: Place) -> String {
        let pointer = Uuid::new_v4().simple().to_string();
        let now = Instant::now();
        let mut kept = self.lock();
        let lifetime = self.lifetime;
        kept.retain(|_, kept| now.saturating_duration_since(kept.since) < lifetime);
        while kept.len() >= CAPACITY {
            let oldest = kept
                .iter()
                .min_by_key(|(_, kept)| kept.since)
                .map(|(pointer, _)| pointer.clone());
            match oldest {
                Some(oldest) => {
                    kept.remove(&oldest);
                }
                None => break,
            }
        }
        kept.insert(pointer.clone(), Kept { place, since: now });
        pointer
    }

    /// Takes the place of `pointer` that `sender` was handed for the query
    /// `tag`; `None` for a pointer unknown, of another sender or query, or
    /// past its lifetime, which is then removed.
    pub fn take(&self, pointer: &str, sender: &str, tag: &str) -> Option<Place> {
        let mut kept = self.lock();
        let found = kept.remove(pointer)?;
        let current = Instant::now().saturating_duration_since(found.since) < self.lifetime;
        if !current {
            return None;
        }
        if found.place.sender != sender || found.place.tag != tag {
            kept.insert(pointer.to_owned(), found);
            return None;
        }
        Some(found.place)
    }

    /// Removes every pointer of `sender`'s query `tag`, for a cancel, and
    /// returns how many were removed.
    pub fn cancel(&self, sender: &str, tag: &str) -> usize {
        let mut kept = self.lock();
        let before = kept.len();
        kept.retain(|_, kept| kept.place.sender != sender || kept.place.tag != tag);
        before.saturating_sub(kept.len())
    }

    /// Returns the guard of the pointers.
    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, Kept>> {
        // NOTE: no specification governs this: our own design; every update
        // leaves the map consistent, so a poisoned lock still holds it.
        self.kept.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use secrecy::SecretString;
    use uuid::Uuid;

    use super::{Cursors, Place};

    fn place(sender: &str, tag: &str) -> Place {
        Place {
            sender: sender.to_owned(),
            tag: tag.to_owned(),
            search: SecretString::from("family=Proefsma"),
            filtered: false,
            after: Uuid::nil(),
            increment: 2,
        }
    }

    #[test]
    fn a_pointer_is_taken_once_by_its_own_sender_and_query() {
        let cursors = Cursors::default();
        let pointer = cursors.keep(place("ADT|EAST", "T-1"));
        assert!(cursors.take(&pointer, "ADT|WEST", "T-1").is_none());
        assert!(cursors.take(&pointer, "ADT|EAST", "T-2").is_none());
        assert!(cursors.take(&pointer, "ADT|EAST", "T-1").is_some());
        assert!(cursors.take(&pointer, "ADT|EAST", "T-1").is_none());
    }

    #[test]
    fn a_pointer_past_its_lifetime_is_removed_when_asked_for() {
        let cursors = Cursors::with_lifetime(std::time::Duration::ZERO);
        let pointer = cursors.keep(place("ADT|EAST", "T-1"));
        assert!(cursors.take(&pointer, "ADT|EAST", "T-1").is_none());
        assert_eq!(cursors.cancel("ADT|EAST", "T-1"), 0, "the pointer is gone");
    }

    #[test]
    fn a_cancel_removes_the_query_s_pointers() {
        let cursors = Cursors::default();
        let pointer = cursors.keep(place("ADT|EAST", "T-1"));
        let other = cursors.keep(place("ADT|EAST", "T-2"));
        assert_eq!(cursors.cancel("ADT|EAST", "T-1"), 1);
        assert!(cursors.take(&pointer, "ADT|EAST", "T-1").is_none());
        assert!(cursors.take(&other, "ADT|EAST", "T-2").is_some());
    }
}
