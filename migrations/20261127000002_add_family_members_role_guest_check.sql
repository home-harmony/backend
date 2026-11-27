-- no-transaction
ALTER TABLE family_members ADD CONSTRAINT family_members_role_check CHECK (role IN ('owner','member','child','guest')) NOT VALID;
