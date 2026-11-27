-- no-transaction
ALTER TABLE family_invite_tokens ADD CONSTRAINT family_invite_tokens_role_check CHECK (role IN ('member','child','guest')) NOT VALID;
