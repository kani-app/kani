-- The reaper was renamed for the worker it actually reaps. Both columns are
-- lookup keys the running code matches on, so a stale value would strand the
-- schedule row and render old history as an unknown job type.
UPDATE recurring_jobs SET kind = 'v8_process_reap' WHERE kind = 'browser_process_reap';
UPDATE jobs SET job_type = 'v8_process_reap' WHERE job_type = 'browser_process_reap';
