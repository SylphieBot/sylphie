CREATE TABLE sylphie_db_kvs_info (
    module_path TEXT NOT NULL PRIMARY KEY,
    table_name TEXT NOT NULL NOT NULL UNIQUE, -- Not that the UNIQUE helps much, but why not.
    kvs_schema_version INTEGER NOT NULL,
    key_id INTEGER NOT NULL,
    key_version INTEGER NOT NULL
) WITHOUT ROWID;

CREATE TABLE sylphie_db_kvs_schema_ids (
    schema_id_name TEXT PRIMARY KEY,
    schema_id_key INTEGER UNIQUE
) WITHOUT ROWID;