-- Add up migration script here
alter table repo_state add column import_branches integer;
alter table repo_state add column import_tags integer;