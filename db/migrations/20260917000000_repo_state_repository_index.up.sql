-- Add up migration script here
-- `delete_states` filters on `repository` and `is_branch`, neither of which is the
-- leading column of the primary key (integration, repository, revision). Without this
-- index, that delete does a full table scan; at startup Broker runs it once per
-- configured integration, so on large `gitlab_group` deployments (thousands of
-- repositories) this scan cost is paid thousands of times before polling begins.
create index repo_state_repository_is_branch on repo_state (repository, is_branch);
