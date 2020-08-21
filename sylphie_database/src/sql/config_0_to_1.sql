CREATE TABLE sylphie_db_configuration (
    scope BLOB NOT NULL,
    key_name TEXT NOT NULL,
    val BLOB,
    val_schema_id INTEGER,
    val_schema_version INTEGER,
    PRIMARY KEY (scope, key_name)
) WITHOUT ROWID;
