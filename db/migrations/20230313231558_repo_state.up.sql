-- Add up migration script here
create table repo_state (
  integration text not null,
  repository text not null,
  revision text not null,
  repo_state blob not null,
  primary key (integration, repository, revision)
);
