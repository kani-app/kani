-- Private hosts an administrator has allowed one source to reach, as a JSON array of
-- "host" or "host:port" strings. NULL grants nothing.
ALTER TABLE sources ADD COLUMN local_hosts TEXT;
