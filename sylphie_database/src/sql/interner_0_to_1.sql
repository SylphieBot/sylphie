CREATE TABLE sylphie_db_interner (
    hive INTEGER NOT NULL,
    name BLOB NOT NULL,
    int_id BIGINT NOT NULL,
    PRIMARY KEY (hive, name),
    UNIQUE (hive, int_id)
) WITHOUT ROWID;
