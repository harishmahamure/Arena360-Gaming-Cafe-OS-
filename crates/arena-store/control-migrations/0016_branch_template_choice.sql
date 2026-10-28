-- Which branch of the tenant a new branch starts its setup from (Story 5.38), and how the new
-- Hub got on with the file. A row with no source is an owner who turned the option off; no row
-- is the default (on, from the first branch that has a stored setup file). The cloud-side file
-- itself lives in the object store (Story 5.37), not here.
CREATE TABLE branch_template_choice (
    branch_id TEXT PRIMARY KEY NOT NULL REFERENCES branches (id),
    source_branch_id TEXT REFERENCES branches (id),
    status TEXT CHECK (status IN ('pending', 'retry', 'applied', 'failed')),
    error TEXT,
    updated_at INTEGER NOT NULL
);
