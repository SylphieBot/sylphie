CREATE TABLE sylphie_db_kvs_info(
    module_path TEXT NOT NULL PRIMARY KEY,
    kvs_schema_version INTEGER NOT NULL,
    key_id TEXT NOT NULL,
    key_version INTEGER NOT NULL
) WITHOUT ROWID;