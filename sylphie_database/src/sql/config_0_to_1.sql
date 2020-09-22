CREATE TABLE sylphie_db_configuration (
    scope INTEGER NOT NULL,
    key_name INTEGER NOT NULL,
    val BLOB,
    val_schema_id INTEGER,
    val_schema_version INTEGER,
    PRIMARY KEY (scope, key_name)
) WITHOUT ROWID;
