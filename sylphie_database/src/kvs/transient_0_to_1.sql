CREATE TABLE transient.sylphie_db_kvs_info (
    module_path TEXT NOT NULL PRIMARY KEY,
    table_name TEXT NOT NULL NOT NULL UNIQUE, -- Not that the UNIQUE helps much, but why not.
    kvs_schema_version INTEGER NOT NULL,
    key_id INTEGER NOT NULL,
    key_version INTEGER NOT NULL
) WITHOUT ROWID;