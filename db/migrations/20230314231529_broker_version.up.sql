-- Add up migration script here
create table broker_version (
  name text not null primary key,
  version text not null
);
