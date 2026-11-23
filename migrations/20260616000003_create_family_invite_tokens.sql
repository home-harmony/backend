-- no-transaction
CREATE TABLE IF NOT EXISTS family_invite_tokens (
  token           UUID          PRIMARY KEY DEFAULT gen_random_uuid(),
  family_id       UUID          NOT NULL REFERENCES family_families(id),
  role            VARCHAR(10)   NOT NULL CHECK (role IN ('member','child','other')),
  permissions     JSONB         NOT NULL DEFAULT '[]'::jsonb,  -- JSON array of Permission flags for Role::Other invites
  relationship    VARCHAR(50),
  created_by      UUID          NOT NULL REFERENCES family_members(id),
  created_at      TIMESTAMPTZ   NOT NULL DEFAULT now(),
  expires_at      TIMESTAMPTZ   NOT NULL,
  used            BOOLEAN       NOT NULL DEFAULT false
);
