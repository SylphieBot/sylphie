CREATE TABLE sylphie_db_configuration (
    scope BLOB NOT NULL,
    key_name TEXT NOT NULL,
    value BLOB NOT NULL,
    value_schema_id INTEGER NOT NULL,
    value_schema_version INTEGER NOT NULL,
    PRIMARY KEY (scope, key_name)
) WITHOUT ROWID;
