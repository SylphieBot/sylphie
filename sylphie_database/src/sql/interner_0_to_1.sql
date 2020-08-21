CREATE TABLE sylphie_db_interner (
    --hive INTEGER NOT NULL,
    name TEXT NOT NULL,
    int_id INTEGER UNIQUE NOT NULL,
    --PRIMARY KEY (hive, name)
    PRIMARY KEY (name)
) WITHOUT ROWID;