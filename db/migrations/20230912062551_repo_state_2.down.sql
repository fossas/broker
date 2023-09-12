-- Add down migration script here
alter table repo_state drop column import_branches integer;
alter table repo_state drop column import_tags integer;