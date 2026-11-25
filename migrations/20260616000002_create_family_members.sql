-- no-transaction
CREATE TABLE IF NOT EXISTS family_members (
  id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  family_id       UUID          NOT NULL REFERENCES family_families(id),
  user_id         UUID          NOT NULL,        -- Cognito sub
  display_name    VARCHAR(100)  NOT NULL,
  role            VARCHAR(10)   NOT NULL CHECK (role IN ('owner','member','child','other')),
  permissions     JSONB         NOT NULL DEFAULT '[]'::jsonb,  -- JSON array of Permission flags (used when role = 'other')
  relationship    VARCHAR(50),                    -- e.g. 'Grandma', 'Cousin'
  joined_at       TIMESTAMPTZ   NOT NULL DEFAULT now(),
  deleted_at      TIMESTAMPTZ,
  UNIQUE (family_id, user_id),
  UNIQUE (family_id, id)
);
