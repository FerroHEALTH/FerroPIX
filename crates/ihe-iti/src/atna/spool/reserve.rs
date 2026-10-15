// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The place of a message taken under a spool's bounds before the message
//! exists, so an exchange whose record the spool could not take is never
//! made. No specification governs it: our own design.

use secrecy::{ExposeSecret, SecretSlice};

use super::{Place, Spool, SpoolError, lock};

/// The place of one message under a spool's bounds, taken by
/// [`Spool::reserve`] before the message exists.
///
/// It counts as a message queued for a write until it is filled with
/// [`Reservation::push`] or dropped, which gives the place back.
pub struct Reservation {
    spool: Spool,
    place: Place,
}

impl std::fmt::Debug for Reservation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Reservation")
            .field("bytes", &self.place.size)
            .finish_non_exhaustive()
    }
}

impl Reservation {
    /// Stores `message` in this place, as [`Spool::push`] stores a message
    /// it admits.
    ///
    /// A message no larger than the place always fits under the bounds; a
    /// larger one needs room for its excess.
    ///
    /// # Errors
    ///
    /// [`SpoolError::Full`] for a message larger than the place when the
    /// bounds have no room for the excess,
    /// [`SpoolError::Late`] past [`Bounds::write_timeout`](super::Bounds::write_timeout),
    /// and [`SpoolError::Io`] when the message cannot be written.
    pub async fn push(self, message: SecretSlice<u8>) -> Result<(), SpoolError> {
        let size = u64::try_from(message.expose_secret().len()).unwrap_or(u64::MAX);
        let place = self.spool.resize(self.place, size)?;
        self.spool.push_in(message, place).await
    }
}

impl Spool {
    /// Takes the place of one message of at most `bytes` bytes under the
    /// bounds before the message exists, so the exchange it will record is
    /// made only once the spool has room for its record.
    ///
    /// The place counts as a message queued for a write until the
    /// [`Reservation`] is filled or dropped, so two callers never both take
    /// the spool's last place.
    ///
    /// # Errors
    ///
    /// [`SpoolError::Full`] when the spool and the writes queued for it have
    /// no room for one more message of `bytes` bytes.
    pub fn reserve(&self, bytes: u64) -> Result<Reservation, SpoolError> {
        let place = self.admit(bytes)?;
        Ok(Reservation {
            spool: self.clone(),
            place,
        })
    }

    /// Makes `place` hold `size` bytes, refusing a message larger than the
    /// place when the bounds have no room for the excess.
    fn resize(&self, mut place: Place, size: u64) -> Result<Place, SpoolError> {
        let mut ledger = lock(&self.shown);
        if size > place.size {
            let excess = size.saturating_sub(place.size);
            let messages = ledger.depth.messages.saturating_add(ledger.queued);
            let bytes = ledger.depth.bytes.saturating_add(ledger.queued_bytes);
            if bytes.saturating_add(excess) > self.bounds.max_bytes {
                let refused = ledger.refuse();
                drop(ledger);
                tracing::warn!(
                    refused,
                    messages,
                    bytes,
                    "a reserved audit message was refused: it is larger than its place and the spool has no room for the rest"
                );
                return Err(SpoolError::Full { messages, bytes });
            }
            ledger.queued_bytes = ledger.queued_bytes.saturating_add(excess);
        } else {
            let spare = place.size.saturating_sub(size);
            ledger.queued_bytes = ledger.queued_bytes.saturating_sub(spare);
        }
        place.size = size;
        Ok(place)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use secrecy::SecretSlice;

    use super::super::{Bounds, Spool, SpoolError};

    fn spool(max_messages: usize, max_bytes: u64) -> Spool {
        Spool::in_memory(Bounds {
            max_messages,
            max_bytes,
            write_timeout: Duration::from_secs(5),
        })
    }

    #[tokio::test]
    async fn a_reserved_place_is_refused_to_every_other_message() {
        let spool = spool(1, 1024);
        let reservation = spool.reserve(16).expect("the one place");
        assert!(matches!(spool.reserve(16), Err(SpoolError::Full { .. })));
        assert!(matches!(
            spool.push(SecretSlice::from(b"other".to_vec())).await,
            Err(SpoolError::Full { .. })
        ));
        reservation
            .push(SecretSlice::from(b"kept".to_vec()))
            .await
            .expect("stored in its place");
        assert_eq!(1, spool.depth().messages);
        assert_eq!(4, spool.depth().bytes);
        assert_eq!(0, spool.queued());
    }

    #[tokio::test]
    async fn a_dropped_reservation_gives_its_place_back() {
        let spool = spool(1, 1024);
        drop(spool.reserve(16).expect("the one place"));
        assert_eq!(0, spool.queued());
        spool
            .push(SecretSlice::from(b"next".to_vec()))
            .await
            .expect("the place is free again");
        assert_eq!(1, spool.depth().messages);
    }

    #[tokio::test]
    async fn a_message_larger_than_its_place_needs_room_for_the_excess() {
        let spool = spool(4, 10);
        let fits = spool.reserve(2).expect("room");
        fits.push(SecretSlice::from(b"12345678".to_vec()))
            .await
            .expect("the excess fits");
        let over = spool.reserve(1).expect("room");
        assert!(matches!(
            over.push(SecretSlice::from(b"123".to_vec())).await,
            Err(SpoolError::Full { .. })
        ));
        assert_eq!(0, spool.queued());
        assert_eq!(8, spool.depth().bytes);
    }

    #[test]
    fn concurrent_reservations_take_each_free_place_once() {
        let spool = spool(3, 1024);
        let taken: Vec<_> = std::thread::scope(|scope| {
            let threads: Vec<_> = (0..8).map(|_| scope.spawn(|| spool.reserve(8))).collect();
            threads
                .into_iter()
                .map(|thread| thread.join().expect("the thread ends"))
                .collect()
        });
        assert_eq!(3, taken.iter().filter(|taken| taken.is_ok()).count());
        assert_eq!(3, spool.queued());
    }
}
