-- The escrow of each Hub's branch key (Story 9.2, AD-24). The Hub wraps its branch KEK for the
-- cloud escrow public key (an age file) and sends it on every tier, Free included: the one piece
-- of key material the cloud holds for a Free branch. `wrapped_kek` is opaque ciphertext; the
-- escrow private key is a SOPS-managed secret that never reaches this database.
--
-- One row per Hub instance of a branch, replaced when that Hub escrows a new key (a re-pair after
-- its keystore was lost). Only the control role reads or writes the table, no endpoint returns
-- `wrapped_kek`, and every access also writes an `admin_audit` row in the same transaction.
CREATE TABLE kek_escrow (
    branch_id TEXT NOT NULL REFERENCES branches (id),
    hub_instance_id TEXT NOT NULL,
    wrapped_kek TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (branch_id, hub_instance_id)
);
