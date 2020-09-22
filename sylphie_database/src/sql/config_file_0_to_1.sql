CREATE TABLE sylphie_db_config_file (
    scope BLOB NOT NULL,
    key_name TEXT NOT NULL,
    data TEXT NOT NULL,
    revision INTEGER NOT NULL,
    PRIMARY KEY (scope, key_name)
) WITHOUT ROWID;

CREATE TABLE sylphie_db_config_file_history (
    scope BLOB NOT NULL,
    key_name TEXT NOT NULL,
    revision INTEGER NOT NULL,
    data TEXT,
    PRIMARY KEY (scope, key_name, revision)
) WITHOUT ROWID;
