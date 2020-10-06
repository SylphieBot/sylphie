CREATE TABLE sylphie_db_configuration (
    scope INTEGER NOT NULL,
    key_id INTEGER NOT NULL,
    val BLOB,
    val_schema_id INTEGER,
    val_schema_version INTEGER,
    PRIMARY KEY (scope, key_id)
) WITHOUT ROWID;
