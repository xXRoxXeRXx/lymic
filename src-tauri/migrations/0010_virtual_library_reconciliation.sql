CREATE TABLE virtual_library_bindings (
    asset_id TEXT NOT NULL,
    projection_path TEXT NOT NULL,
    version TEXT NOT NULL,
    PRIMARY KEY (asset_id, projection_path)
);

CREATE TABLE virtual_library_reconciliation (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    watermark TEXT NOT NULL
);
