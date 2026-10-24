// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The `[db]` section: the PostgreSQL connection, the pool, the migration
//! switch and the sealing key. No specification governs the store: our own
//! design.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::config::Profile;
use crate::config::error::Error;
use crate::config::secret::{self, Secret, SecretUrl};

/// The `[db]` section as written.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Db {
    /// The PostgreSQL connection URL, which may carry a password; refused
    /// under production, where `url_file` names it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<SecretUrl>,
    /// The file holding the connection URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url_file: Option<PathBuf>,
    /// The most connections the pool holds.
    pub pool_size: u32,
    /// Whether `serve` applies the pending migrations at start.
    pub migrate: bool,
    /// The file holding the key the identifier values are sealed under.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_key_file: Option<PathBuf>,
}

impl Default for Db {
    fn default() -> Self {
        Self {
            url: None,
            url_file: None,
            pool_size: 10,
            migrate: true,
            data_key_file: None,
        }
    }
}

/// The `[db]` section, resolved, with both secrets read.
#[derive(Debug, Clone)]
pub struct DbSettings {
    /// The PostgreSQL connection URL.
    pub url: SecretUrl,
    /// The most connections the pool holds.
    pub pool_size: u32,
    /// Whether `serve` applies the pending migrations at start.
    pub migrate: bool,
    /// The sealing key, as the file holds it.
    pub data_key: Secret,
}

impl Db {
    /// Resolves the section under `profile`, reading both secret files.
    ///
    /// # Errors
    /// Returns every [`Error`] of [`secret::read`] for the URL,
    /// [`Error::Missing`] for an unset URL or key file, [`Error::Value`] for a
    /// URL that is not `postgres://` or `postgresql://` or a pool of 0, and
    /// the errors of [`secret::read_file`] for the key file.
    pub fn resolve(&self, profile: Profile) -> Result<DbSettings, Error> {
        let inline = self.url.as_ref().map(|url| Secret::new(url.expose()));
        let url = secret::read("db.url", inline.as_ref(), self.url_file.as_deref(), profile)?
            .ok_or_else(|| Error::Missing {
                key: String::from("db.url"),
            })?;
        // NOTE: no specification governs this: our own design; the URL form of
        // a libpq connection string, which names its scheme.
        if !(url.expose().starts_with("postgres://") || url.expose().starts_with("postgresql://")) {
            return Err(Error::Value {
                key: String::from("db.url"),
                rule: "a postgres:// or postgresql:// URL",
            });
        }
        if self.pool_size == 0 {
            return Err(Error::Value {
                key: String::from("db.pool_size"),
                rule: "it must be above 0",
            });
        }
        let key_file = self
            .data_key_file
            .as_deref()
            .ok_or_else(|| Error::Missing {
                key: String::from("db.data_key_file"),
            })?;
        // TODO(#59): read the key in the form the sealing defines.
        let data_key = secret::read_file("db.data_key_file", key_file)?;
        Ok(DbSettings {
            url: SecretUrl::new(url.expose()),
            pool_size: self.pool_size,
            migrate: self.migrate,
            data_key,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::Db;
    use crate::config::Profile;
    use crate::config::error::Error;
    use crate::config::secret::SecretUrl;

    fn db(url: &str, key: &Path) -> Db {
        Db {
            url: Some(SecretUrl::new(url)),
            data_key_file: Some(key.to_path_buf()),
            ..Db::default()
        }
    }

    #[test]
    fn a_url_of_another_scheme_a_zero_pool_and_a_missing_key_file_are_refused() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let key = directory.path().join("key");
        std::fs::write(&key, "a-test-key-made-for-this-case").expect("the fixture is written");
        db("postgres://db.invalid/pix", &key)
            .resolve(Profile::Development)
            .expect("a PostgreSQL URL resolves");
        let error = db("mysql://db.invalid/pix", &key)
            .resolve(Profile::Development)
            .expect_err("only PostgreSQL is a store");
        assert!(matches!(error, Error::Value { .. }), "{error:?}");
        let mut zero = db("postgres://db.invalid/pix", &key);
        zero.pool_size = 0;
        assert!(matches!(
            zero.resolve(Profile::Development),
            Err(Error::Value { .. })
        ));
        let mut keyless = db("postgres://db.invalid/pix", &key);
        keyless.data_key_file = None;
        assert!(matches!(
            keyless.resolve(Profile::Development),
            Err(Error::Missing { .. })
        ));
        let unset = Db {
            data_key_file: Some(key.clone()),
            ..Db::default()
        };
        assert!(matches!(
            unset.resolve(Profile::Development),
            Err(Error::Missing { .. })
        ));
    }

    #[test]
    fn a_bare_url_is_refused_under_production_and_its_file_is_taken() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let key = directory.path().join("key");
        std::fs::write(&key, "a-test-key-made-for-this-case").expect("the fixture is written");
        let error = db("postgres://u:Qz7pw@db.invalid/pix", &key)
            .resolve(Profile::Production)
            .expect_err("production takes the URL from its file");
        assert!(
            matches!(&error, Error::InlineSecret { key } if key == "db.url"),
            "{error:?}"
        );
        assert!(!error.to_string().contains("Qz7pw"), "{error}");
        let url = directory.path().join("url");
        std::fs::write(&url, "postgres://u:Qz7pw@db.invalid/pix\n")
            .expect("the fixture is written");
        let from_file = Db {
            url_file: Some(url),
            data_key_file: Some(key),
            ..Db::default()
        }
        .resolve(Profile::Production)
        .expect("the URL file resolves");
        assert_eq!("postgres://***@db.invalid/pix", from_file.url.to_string());
        assert!(!format!("{from_file:?}").contains("Qz7pw"));
        assert!(!format!("{from_file:?}").contains("a-test-key"));
    }
}
