//! Error types for tabulosity write operations.
//!
//! `Error` is the single enum returned by all fallible mutations (insert,
//! update, upsert, remove). `DeserializeError` wraps a `Vec<Error>` for
//! bulk-loading scenarios where multiple problems must be reported at once.

/// All errors returned by tabulosity write operations.
#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    /// Attempted to insert a row with a primary key that already exists.
    DuplicateKey { table: &'static str, key: String },

    /// Attempted to update or remove a row that does not exist.
    NotFound { table: &'static str, key: String },

    /// Attempted to insert or update a row, but a foreign key field
    /// references a row that does not exist in the target table.
    FkTargetNotFound {
        table: &'static str,
        field: &'static str,
        referenced_table: &'static str,
        key: String,
    },

    /// Attempted to remove a row that is still referenced by foreign keys
    /// in other tables (restrict semantics).
    FkViolation {
        table: &'static str,
        key: String,
        /// Each entry: (referencing table name, FK field name, count of references).
        /// ALL referencing tables/fields are checked and all violations are
        /// collected — the check does NOT short-circuit on the first violation.
        referenced_by: Vec<(&'static str, &'static str, usize)>,
    },
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::DuplicateKey { table, key } => {
                write!(f, "duplicate key in {}: {}", table, key)
            }
            Error::NotFound { table, key } => write!(f, "not found in {}: {}", table, key),
            Error::FkTargetNotFound {
                table,
                field,
                referenced_table,
                key,
            } => write!(
                f,
                "FK target not found: {}.{} references {} key {}",
                table, field, referenced_table, key
            ),
            Error::FkViolation {
                table,
                key,
                referenced_by,
            } => {
                write!(f, "FK violation: {}.{} still referenced by", table, key)?;
                for (ref_table, ref_field, count) in referenced_by {
                    write!(f, " {}.{} ({} rows)", ref_table, ref_field, count)?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for Error {}

/// Error returned when deserializing a database fails validation.
#[derive(Debug, Clone, PartialEq)]
pub struct DeserializeError {
    /// All errors found during validation. Variants will be `DuplicateKey`
    /// and/or `FkTargetNotFound` — the only errors that can arise from
    /// loading serialized data.
    pub errors: Vec<Error>,
}

impl std::fmt::Display for DeserializeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "deserialization failed with {} errors:",
            self.errors.len()
        )?;
        for err in &self.errors {
            write!(f, "\n  - {}", err)?;
        }
        Ok(())
    }
}

impl std::error::Error for DeserializeError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_duplicate_key() {
        let err = Error::DuplicateKey {
            table: "creatures",
            key: "CreatureId(1)".into(),
        };
        assert_eq!(err.to_string(), "duplicate key in creatures: CreatureId(1)");
    }

    #[test]
    fn display_not_found() {
        let err = Error::NotFound {
            table: "tasks",
            key: "TaskId(42)".into(),
        };
        assert_eq!(err.to_string(), "not found in tasks: TaskId(42)");
    }

    #[test]
    fn display_fk_target_not_found() {
        let err = Error::FkTargetNotFound {
            table: "tasks",
            field: "assignee",
            referenced_table: "creatures",
            key: "CreatureId(99)".into(),
        };
        assert_eq!(
            err.to_string(),
            "FK target not found: tasks.assignee references creatures key CreatureId(99)"
        );
    }

    #[test]
    fn display_fk_violation() {
        let err = Error::FkViolation {
            table: "creatures",
            key: "CreatureId(7)".into(),
            referenced_by: vec![("tasks", "assignee", 3), ("friendships", "target", 1)],
        };
        assert_eq!(
            err.to_string(),
            "FK violation: creatures.CreatureId(7) still referenced by \
             tasks.assignee (3 rows) friendships.target (1 rows)"
        );
    }

    #[test]
    fn error_equality() {
        let a = Error::DuplicateKey {
            table: "t",
            key: "k".into(),
        };
        let b = Error::DuplicateKey {
            table: "t",
            key: "k".into(),
        };
        assert_eq!(a, b);

        let c = Error::NotFound {
            table: "t",
            key: "k".into(),
        };
        assert_ne!(a, c);
    }

    #[test]
    fn error_implements_std_error() {
        let err: Box<dyn std::error::Error> = Box::new(Error::NotFound {
            table: "t",
            key: "k".into(),
        });
        assert_eq!(err.to_string(), "not found in t: k");
    }

    #[test]
    fn display_deserialize_error() {
        let err = DeserializeError {
            errors: vec![
                Error::DuplicateKey {
                    table: "creatures",
                    key: "1".into(),
                },
                Error::FkTargetNotFound {
                    table: "tasks",
                    field: "assignee",
                    referenced_table: "creatures",
                    key: "99".into(),
                },
            ],
        };
        let s = err.to_string();
        assert!(s.starts_with("deserialization failed with 2 errors:"));
        assert!(s.contains("duplicate key in creatures: 1"));
        assert!(s.contains("FK target not found: tasks.assignee references creatures key 99"));
    }

    #[test]
    fn deserialize_error_implements_std_error() {
        let err: Box<dyn std::error::Error> = Box::new(DeserializeError {
            errors: vec![Error::NotFound {
                table: "t",
                key: "k".into(),
            }],
        });
        assert!(err.to_string().contains("1 errors"));
    }
}
