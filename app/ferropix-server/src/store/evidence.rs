// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The stored form of a score's evidence (`docs/architecture.md` §5.2), on
//! a candidate and on a probabilistic link, and the refill that moves the
//! evidence written as JSON before it into that form.
//!
//! The form is versioned by its first byte, so a later layout is a new
//! version and never a reading of these bytes another way. Version 1, every
//! number little-endian:
//!
//! - the version, 1; the grade; the blocking key, 0 for none; the number of
//!   conflicts, then one byte each;
//! - the total and the score, each an `f64`;
//! - the number of fields, then each field: its field, comparator and
//!   agreement, a byte of flags (1: a first value, 2: a second value, 4: a
//!   reading), its weight as an `f64`, the reading as an `f64` when flagged,
//!   and each value flagged as the count of the `•` it opens with (`u16`),
//!   then the byte length (`u8`) and the UTF-8 of the rest.
//!
//! Each value is kept exactly as the masking of §12.5 wrote it, so the
//! store holds no more of a value than the JSON did. The names the JSON
//! repeated in every entry are the codes of this module's tables. The
//! evidence the store held as JSON is moved by the server after the
//! migrations, a thousand rows at a time, and records the refill done in
//! `pix.evidence_refill`; a server that does not migrate refuses to start
//! until it is. No specification governs
//! the stored form: our own design.

use sqlx::PgPool;
use uuid::Uuid;

use crate::config::section::matching::{BlockingKey, Field};
use crate::matching::compare::{Agreement, Comparator};
use crate::matching::conflict::Conflict;
use crate::matching::scorer::{Evidence, FieldEvidence, Grade};
use crate::store::StoreError;

/// The version of the layout [`pack`] writes.
pub const VERSION: u8 = 1;

/// The rows one transaction of the refill moves.
pub const BATCH: i64 = 1000;

/// The character a masked value hides each character behind.
const HIDDEN: char = '•';

/// Stored evidence that does not read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum EvidenceError {
    /// The bytes name a layout version this server does not read.
    #[error("the stored evidence is of layout version {0}, and this server reads version 1")]
    Version(u8),
    /// The bytes end before the layout does, or go on after it.
    #[error("the stored evidence is not of its layout's length")]
    Length,
    /// A code names no value of its table.
    #[error("the stored evidence holds a {0} code no value has")]
    Code(&'static str),
    /// A value is too long for the layout or is no UTF-8.
    #[error("the stored evidence holds a value the layout cannot hold")]
    Value,
    /// The evidence stored as JSON before the layout does not read.
    #[error("the evidence stored as JSON does not read")]
    Json,
    /// A row still holds its evidence as JSON, which `db migrate` moves.
    #[error("a candidate's or a link's evidence is still stored as JSON; run db migrate")]
    Unpacked,
}

/// The code of each grade.
fn grade_code(grade: Grade) -> u8 {
    match grade {
        Grade::CertainlyNot => 0,
        Grade::Possible => 1,
        Grade::Probable => 2,
        Grade::Certain => 3,
    }
}

/// The grade of `code`.
fn grade_of(code: u8) -> Result<Grade, EvidenceError> {
    match code {
        0 => Ok(Grade::CertainlyNot),
        1 => Ok(Grade::Possible),
        2 => Ok(Grade::Probable),
        3 => Ok(Grade::Certain),
        _ => Err(EvidenceError::Code("grade")),
    }
}

/// The code of each blocking key, 0 standing for none.
fn key_code(key: Option<BlockingKey>) -> u8 {
    match key {
        None => 0,
        Some(BlockingKey::BirthYearFamilyPhonetic) => 1,
        Some(BlockingKey::PostalCodeBirthDate) => 2,
        Some(BlockingKey::SharedIdentifier) => 3,
    }
}

/// The blocking key of `code`.
fn key_of(code: u8) -> Result<Option<BlockingKey>, EvidenceError> {
    match code {
        0 => Ok(None),
        1 => Ok(Some(BlockingKey::BirthYearFamilyPhonetic)),
        2 => Ok(Some(BlockingKey::PostalCodeBirthDate)),
        3 => Ok(Some(BlockingKey::SharedIdentifier)),
        _ => Err(EvidenceError::Code("blocking key")),
    }
}

/// The code of each conflict.
fn conflict_code(conflict: Conflict) -> u8 {
    match conflict {
        Conflict::BirthYear => 0,
        Conflict::Gender => 1,
        Conflict::MasterIdentities => 2,
    }
}

/// The conflict of `code`.
fn conflict_of(code: u8) -> Result<Conflict, EvidenceError> {
    match code {
        0 => Ok(Conflict::BirthYear),
        1 => Ok(Conflict::Gender),
        2 => Ok(Conflict::MasterIdentities),
        _ => Err(EvidenceError::Code("conflict")),
    }
}

/// The code of each field.
fn field_code(field: Field) -> u8 {
    match field {
        Field::FamilyName => 0,
        Field::GivenNames => 1,
        Field::BirthDate => 2,
        Field::Gender => 3,
        Field::PostalCode => 4,
        Field::Address => 5,
        Field::Telephone => 6,
        Field::Email => 7,
        Field::MothersMaidenName => 8,
        Field::LocalIdentifier => 9,
    }
}

/// The field of `code`.
fn field_of(code: u8) -> Result<Field, EvidenceError> {
    match code {
        0 => Ok(Field::FamilyName),
        1 => Ok(Field::GivenNames),
        2 => Ok(Field::BirthDate),
        3 => Ok(Field::Gender),
        4 => Ok(Field::PostalCode),
        5 => Ok(Field::Address),
        6 => Ok(Field::Telephone),
        7 => Ok(Field::Email),
        8 => Ok(Field::MothersMaidenName),
        9 => Ok(Field::LocalIdentifier),
        _ => Err(EvidenceError::Code("field")),
    }
}

/// The code of each comparator.
fn comparator_code(comparator: Comparator) -> u8 {
    match comparator {
        Comparator::JaroWinkler => 0,
        Comparator::Exact => 1,
        Comparator::Prefix => 2,
        Comparator::TokenOverlap => 3,
        Comparator::Identifier => 4,
    }
}

/// The comparator of `code`.
fn comparator_of(code: u8) -> Result<Comparator, EvidenceError> {
    match code {
        0 => Ok(Comparator::JaroWinkler),
        1 => Ok(Comparator::Exact),
        2 => Ok(Comparator::Prefix),
        3 => Ok(Comparator::TokenOverlap),
        4 => Ok(Comparator::Identifier),
        _ => Err(EvidenceError::Code("comparator")),
    }
}

/// The code of each agreement.
fn agreement_code(agreement: Agreement) -> u8 {
    match agreement {
        Agreement::Absent => 0,
        Agreement::Disagree => 1,
        Agreement::Partial => 2,
        Agreement::Agree => 3,
    }
}

/// The agreement of `code`.
fn agreement_of(code: u8) -> Result<Agreement, EvidenceError> {
    match code {
        0 => Ok(Agreement::Absent),
        1 => Ok(Agreement::Disagree),
        2 => Ok(Agreement::Partial),
        3 => Ok(Agreement::Agree),
        _ => Err(EvidenceError::Code("agreement")),
    }
}

/// The flag of a first value.
const FIRST: u8 = 1;
/// The flag of a second value.
const SECOND: u8 = 2;
/// The flag of a reading.
const READING: u8 = 4;

/// Returns `evidence` in the stored form of [`VERSION`].
///
/// # Errors
///
/// Returns [`EvidenceError::Value`] for more conflicts or fields than a
/// byte counts, or a value with more leading `•` than a `u16` counts or a
/// rest longer than 255 bytes.
pub fn pack(evidence: &Evidence) -> Result<Vec<u8>, EvidenceError> {
    let mut out = Vec::with_capacity(32 + evidence.fields.len() * 32);
    out.push(VERSION);
    out.push(grade_code(evidence.grade));
    out.push(key_code(evidence.blocking_key));
    out.push(u8::try_from(evidence.conflicts.len()).map_err(|_many| EvidenceError::Value)?);
    out.extend(
        evidence
            .conflicts
            .iter()
            .map(|conflict| conflict_code(*conflict)),
    );
    out.extend_from_slice(&evidence.total.to_le_bytes());
    out.extend_from_slice(&evidence.score.to_le_bytes());
    out.push(u8::try_from(evidence.fields.len()).map_err(|_many| EvidenceError::Value)?);
    for field in &evidence.fields {
        let mut flags = 0;
        if field.a.is_some() {
            flags |= FIRST;
        }
        if field.b.is_some() {
            flags |= SECOND;
        }
        if field.reading.is_some() {
            flags |= READING;
        }
        out.push(field_code(field.field));
        out.push(comparator_code(field.comparator));
        out.push(agreement_code(field.agreement));
        out.push(flags);
        out.extend_from_slice(&field.weight.to_le_bytes());
        if let Some(reading) = field.reading {
            out.extend_from_slice(&reading.to_le_bytes());
        }
        for value in [&field.a, &field.b].into_iter().flatten() {
            masked(&mut out, value)?;
        }
    }
    Ok(out)
}

/// Writes `value` as the count of the `•` it opens with and the rest.
fn masked(out: &mut Vec<u8>, value: &str) -> Result<(), EvidenceError> {
    let hidden = value.chars().take_while(|c| *c == HIDDEN).count();
    let rest = value.trim_start_matches(HIDDEN);
    out.extend_from_slice(
        &u16::try_from(hidden)
            .map_err(|_long| EvidenceError::Value)?
            .to_le_bytes(),
    );
    out.push(u8::try_from(rest.len()).map_err(|_long| EvidenceError::Value)?);
    out.extend_from_slice(rest.as_bytes());
    Ok(())
}

/// A reader over stored evidence that refuses to read past its end.
struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    /// Takes the next `n` bytes.
    fn take(&mut self, n: usize) -> Result<&'a [u8], EvidenceError> {
        let (head, tail) = self.0.split_at_checked(n).ok_or(EvidenceError::Length)?;
        self.0 = tail;
        Ok(head)
    }

    /// Takes the next byte.
    fn byte(&mut self) -> Result<u8, EvidenceError> {
        let (head, tail) = self.0.split_first().ok_or(EvidenceError::Length)?;
        self.0 = tail;
        Ok(*head)
    }

    /// Takes the next `f64`.
    fn float(&mut self) -> Result<f64, EvidenceError> {
        let (head, tail) = self
            .0
            .split_first_chunk::<8>()
            .ok_or(EvidenceError::Length)?;
        self.0 = tail;
        Ok(f64::from_le_bytes(*head))
    }

    /// Takes the next masked value.
    fn masked(&mut self) -> Result<String, EvidenceError> {
        let (count, tail) = self
            .0
            .split_first_chunk::<2>()
            .ok_or(EvidenceError::Length)?;
        self.0 = tail;
        let hidden = usize::from(u16::from_le_bytes(*count));
        let length = usize::from(self.byte()?);
        let rest = std::str::from_utf8(self.take(length)?).map_err(|_utf8| EvidenceError::Value)?;
        let mut value: String = std::iter::repeat_n(HIDDEN, hidden).collect();
        value.push_str(rest);
        Ok(value)
    }
}

/// Reads stored evidence back.
///
/// # Errors
///
/// Returns [`EvidenceError::Version`] for a layout version other than
/// [`VERSION`], [`EvidenceError::Length`] for bytes that end early or run
/// on, [`EvidenceError::Code`] for a code no value has, and
/// [`EvidenceError::Value`] for a value that is no UTF-8.
pub fn unpack(stored: &[u8]) -> Result<Evidence, EvidenceError> {
    let mut reader = Reader(stored);
    let version = reader.byte()?;
    if version != VERSION {
        return Err(EvidenceError::Version(version));
    }
    let grade = grade_of(reader.byte()?)?;
    let blocking_key = key_of(reader.byte()?)?;
    let conflicts = (0..reader.byte()?)
        .map(|_| reader.byte().and_then(conflict_of))
        .collect::<Result<Vec<_>, _>>()?;
    let total = reader.float()?;
    let score = reader.float()?;
    let count = reader.byte()?;
    let mut fields = Vec::with_capacity(usize::from(count));
    for _ in 0..count {
        let field = field_of(reader.byte()?)?;
        let comparator = comparator_of(reader.byte()?)?;
        let agreement = agreement_of(reader.byte()?)?;
        let flags = reader.byte()?;
        let weight = reader.float()?;
        let reading = if flags & READING == 0 {
            None
        } else {
            Some(reader.float()?)
        };
        let a = if flags & FIRST == 0 {
            None
        } else {
            Some(reader.masked()?)
        };
        let b = if flags & SECOND == 0 {
            None
        } else {
            Some(reader.masked()?)
        };
        fields.push(FieldEvidence {
            field,
            comparator,
            a,
            b,
            agreement,
            reading,
            weight,
        });
    }
    if !reader.0.is_empty() {
        return Err(EvidenceError::Length);
    }
    Ok(Evidence {
        conflicts,
        blocking_key,
        total,
        score,
        grade,
        fields,
    })
}

/// The evidence stored as JSON before the layout, in the layout.
fn repacked(json: &str) -> Result<Vec<u8>, EvidenceError> {
    let evidence: Evidence = serde_json::from_str(json).map_err(|_unread| EvidenceError::Json)?;
    pack(&evidence)
}

/// The table and the statements of one store of evidence the refill moves.
struct Moved {
    /// The table, for the log.
    table: &'static str,
    /// The next rows still holding JSON after `$1`, at most `$2`.
    read: &'static str,
    /// Writes the packed evidence `$2` of the rows `$1`.
    write: &'static str,
}

/// The candidates, whose JSON waits in `evidence_json`, and the
/// probabilistic links, whose JSON waits as the bytes of its text in
/// `evidence`, which a packed form never opens with (its version byte is
/// not `{`).
const MOVED: [Moved; 2] = [
    Moved {
        table: "candidate",
        read: "SELECT id, evidence_json::text FROM pix.candidate
               WHERE evidence_json IS NOT NULL AND id > $1 ORDER BY id LIMIT $2",
        write: "UPDATE pix.candidate c SET evidence = u.packed, evidence_json = NULL
                FROM unnest($1::uuid[], $2::bytea[]) AS u (id, packed)
                WHERE c.id = u.id AND c.evidence_json IS NOT NULL",
    },
    Moved {
        table: "link",
        read: "SELECT id, convert_from(evidence, 'UTF8') FROM pix.link
               WHERE get_byte(evidence, 0) = 123 AND id > $1 ORDER BY id LIMIT $2",
        write: "UPDATE pix.link l SET evidence = u.packed
                FROM unnest($1::uuid[], $2::bytea[]) AS u (id, packed)
                WHERE l.id = u.id AND get_byte(l.evidence, 0) = 123",
    },
];

/// Moves every candidate's and probabilistic link's evidence still stored
/// as JSON in `pool` into the layout, `batch` rows per transaction, and
/// returns how many rows it moved.
///
/// # Errors
///
/// Returns [`StoreError::Query`] when a statement fails and
/// [`StoreError::Evidence`] when stored JSON does not read.
pub async fn refill(pool: &PgPool, batch: i64) -> Result<u64, StoreError> {
    let mut moved: u64 = 0;
    for store in &MOVED {
        let mut after = Uuid::nil();
        let mut rows: u64 = 0;
        loop {
            let mut transaction = pool.begin().await?;
            let found: Vec<(Uuid, String)> = sqlx::query_as(store.read)
                .bind(after)
                .bind(batch)
                .fetch_all(&mut *transaction)
                .await?;
            let Some(last) = found.last().map(|(id, _)| *id) else {
                break;
            };
            let mut ids = Vec::with_capacity(found.len());
            let mut packed = Vec::with_capacity(found.len());
            for (id, json) in found {
                ids.push(id);
                packed.push(repacked(&json)?);
            }
            let done = sqlx::query(store.write)
                .bind(&ids)
                .bind(&packed)
                .execute(&mut *transaction)
                .await?
                .rows_affected();
            transaction.commit().await?;
            rows = rows.saturating_add(done);
            after = last;
        }
        if rows > 0 {
            tracing::info!(table = store.table, rows, "the stored evidence was packed");
        }
        moved = moved.saturating_add(rows);
    }
    sqlx::query("UPDATE pix.evidence_refill SET done_at = now() WHERE done_at IS NULL")
        .execute(pool)
        .await?;
    Ok(moved)
}

/// Refuses the start of a server that does not migrate until the refill
/// in `pool` is recorded done.
///
/// # Errors
///
/// Returns [`StoreError::Evidence`] with [`EvidenceError::Unpacked`] while
/// a row is left, and [`StoreError::Query`] when the statement fails.
pub async fn check(pool: &PgPool) -> Result<(), StoreError> {
    let left: bool = sqlx::query_scalar("SELECT done_at IS NULL FROM pix.evidence_refill")
        .fetch_one(pool)
        .await?;
    if left {
        return Err(StoreError::Evidence(EvidenceError::Unpacked));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{EvidenceError, VERSION, pack, unpack};
    use crate::config::section::matching::{BlockingKey, Field};
    use crate::matching::compare::{Agreement, Comparator};
    use crate::matching::conflict::Conflict;
    use crate::matching::scorer::{Evidence, FieldEvidence, Grade};

    const FIELDS: [Field; 10] = [
        Field::FamilyName,
        Field::GivenNames,
        Field::BirthDate,
        Field::Gender,
        Field::PostalCode,
        Field::Address,
        Field::Telephone,
        Field::Email,
        Field::MothersMaidenName,
        Field::LocalIdentifier,
    ];
    const COMPARATORS: [Comparator; 5] = [
        Comparator::JaroWinkler,
        Comparator::Exact,
        Comparator::Prefix,
        Comparator::TokenOverlap,
        Comparator::Identifier,
    ];
    const AGREEMENTS: [Agreement; 4] = [
        Agreement::Absent,
        Agreement::Disagree,
        Agreement::Partial,
        Agreement::Agree,
    ];
    const GRADES: [Grade; 4] = [
        Grade::CertainlyNot,
        Grade::Possible,
        Grade::Probable,
        Grade::Certain,
    ];
    const KEYS: [Option<BlockingKey>; 4] = [
        None,
        Some(BlockingKey::BirthYearFamilyPhonetic),
        Some(BlockingKey::PostalCodeBirthDate),
        Some(BlockingKey::SharedIdentifier),
    ];

    /// The values a field may hold: none, masked, unmasked, empty, all
    /// hidden, multi-byte.
    const VALUES: [Option<&str>; 6] = [
        None,
        Some("•••••••tra"),
        Some("female"),
        Some(""),
        Some("••••"),
        Some("••••ëij"),
    ];

    fn field(index: usize) -> FieldEvidence {
        FieldEvidence {
            field: FIELDS[index % FIELDS.len()],
            comparator: COMPARATORS[index % COMPARATORS.len()],
            a: VALUES[index % VALUES.len()].map(str::to_owned),
            b: VALUES[(index * 5 + 1) % VALUES.len()].map(str::to_owned),
            agreement: AGREEMENTS[index % AGREEMENTS.len()],
            reading: index.is_multiple_of(2).then_some(0.858_076_923_076_923_2),
            weight: -3.292_781_749_227_846 + f64::from(u32::try_from(index).unwrap()),
        }
    }

    #[test]
    fn every_shape_of_evidence_reads_back_as_it_was() {
        let conflicts = [
            vec![],
            vec![Conflict::BirthYear],
            vec![Conflict::Gender, Conflict::MasterIdentities],
        ];
        let mut shapes = 0;
        for grade in GRADES {
            for key in KEYS {
                for conflicts in &conflicts {
                    for count in [0, 1, 10, 36] {
                        let evidence = Evidence {
                            conflicts: conflicts.clone(),
                            blocking_key: key,
                            total: 15.208_226_585_132_179,
                            score: 0.973_662_413_271_841_7,
                            grade,
                            fields: (0..count).map(field).collect(),
                        };
                        let packed = pack(&evidence).expect("it packs");
                        assert_eq!(Some(&VERSION), packed.first(), "the version leads");
                        assert_eq!(
                            unpack(&packed).expect("it reads"),
                            evidence,
                            "{grade:?} {key:?} {conflicts:?} {count}"
                        );
                        shapes += 1;
                    }
                }
            }
        }
        assert_eq!(4 * 4 * 3 * 4, shapes);
    }

    #[test]
    fn the_scorer_s_evidence_reads_back_as_it_was_and_shrinks() {
        let matching = crate::config::section::matching::Matching::default();
        let scorer = crate::matching::scorer::Scorer::new(&matching);
        let patient = |family: &str, given: &str, birth: &str| {
            let text = format!(
                r#"{{"resourceType":"Patient","name":[{{"family":"{family}","given":["{given}"]}}],"birthDate":"{birth}","gender":"female","address":[{{"line":["Voorbeeldstraat 1"],"postalCode":"1023 AW"}}]}}"#
            );
            crate::store::demographics::patient(&text).expect("a patient")
        };
        let subject = |family, given, birth| {
            crate::matching::subject::Subject::from_patient(
                &patient(family, given, birth),
                std::collections::BTreeSet::new(),
            )
        };
        let found = scorer.score(
            &subject("Proefsma", "Ada", "1961-04-12"),
            &subject("Proefsmaa", "Bram", "1961-04-21"),
            Some(BlockingKey::BirthYearFamilyPhonetic),
        );
        let packed = pack(&found.evidence).expect("it packs");
        assert_eq!(unpack(&packed).expect("it reads"), found.evidence);
        let json = serde_json::to_string(&found.evidence).expect("JSON");
        assert!(
            packed.len() * 3 < json.len(),
            "{} bytes packed against {} as JSON",
            packed.len(),
            json.len()
        );
    }

    #[test]
    fn the_layout_writes_these_bytes() {
        let evidence = Evidence {
            conflicts: vec![Conflict::Gender],
            blocking_key: Some(BlockingKey::PostalCodeBirthDate),
            total: 1.0,
            score: 0.5,
            grade: Grade::Probable,
            fields: vec![FieldEvidence {
                field: Field::BirthDate,
                comparator: Comparator::Exact,
                a: Some(String::from("•••12")),
                b: None,
                agreement: Agreement::Agree,
                reading: Some(2.0),
                weight: -1.5,
            }],
        };
        let golden: Vec<u8> = [
            vec![1, 2, 2, 1, 1],
            1.0_f64.to_le_bytes().to_vec(),
            0.5_f64.to_le_bytes().to_vec(),
            vec![1, 2, 1, 3, 5],
            (-1.5_f64).to_le_bytes().to_vec(),
            2.0_f64.to_le_bytes().to_vec(),
            vec![3, 0, 2, b'1', b'2'],
        ]
        .concat();
        assert_eq!(golden, pack(&evidence).expect("it packs"));
        assert_eq!(unpack(&golden).expect("it reads"), evidence);
    }

    #[test]
    fn bytes_off_the_layout_are_refused() {
        let evidence = Evidence {
            conflicts: vec![],
            blocking_key: None,
            total: 0.0,
            score: 0.0,
            grade: Grade::Possible,
            fields: vec![field(1)],
        };
        let packed = pack(&evidence).expect("it packs");
        let mut other = packed.clone();
        other[0] = 2;
        assert_eq!(Err(EvidenceError::Version(2)), unpack(&other).map(|_| ()));
        assert_eq!(
            Err(EvidenceError::Length),
            unpack(&packed[..packed.len() - 1]).map(|_| ())
        );
        let mut longer = packed.clone();
        longer.push(0);
        assert_eq!(Err(EvidenceError::Length), unpack(&longer).map(|_| ()));
        let mut graded = packed;
        graded[1] = 9;
        assert_eq!(
            Err(EvidenceError::Code("grade")),
            unpack(&graded).map(|_| ())
        );
        assert_eq!(Err(EvidenceError::Length), unpack(&[]).map(|_| ()));
    }
}
