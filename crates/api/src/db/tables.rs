//! Compile-time–checked column identifiers for all tables.

use sea_query::Iden;

#[derive(Iden)]
pub enum Users {
    Table,
    Id,
    Nickname,
    ApiKey,
    CreatedAt,
    Email,
    PasswordHash,
    PasswordSalt,
}

#[derive(Iden)]
pub enum Sessions {
    Table,
    Id,
    UserId,
    TeamId,
    Tool,
    AgentProvider,
    AgentModel,
    Title,
    Description,
    Tags,
    CreatedAt,
    UploadedAt,
    MessageCount,
    TaskCount,
    EventCount,
    DurationSeconds,
    TotalInputTokens,
    TotalOutputTokens,
    BodyStorageKey,
    BodyUrl,
    GitRemote,
    GitBranch,
    GitCommit,
    GitRepoName,
    PrNumber,
    PrUrl,
    WorkingDirectory,
    FilesModified,
    FilesRead,
    HasErrors,
    MaxActiveAgents,
}

#[derive(Iden)]
pub enum Teams {
    Table,
    Id,
    Name,
    Description,
    IsPublic,
    CreatedBy,
    CreatedAt,
}

#[derive(Iden)]
pub enum TeamMembers {
    Table,
    TeamId,
    UserId,
    Role,
    JoinedAt,
}

#[derive(Iden)]
pub enum SessionLinks {
    Table,
    SessionId,
    LinkedSessionId,
    LinkType,
    CreatedAt,
}

#[derive(Iden)]
pub enum OauthIdentities {
    Table,
    UserId,
    Provider,
    ProviderUserId,
    ProviderUsername,
    AvatarUrl,
    InstanceUrl,
    CreatedAt,
}

#[derive(Iden)]
pub enum OauthStates {
    Table,
    State,
    Provider,
    CreatedAt,
    ExpiresAt,
    UserId,
}

#[derive(Iden)]
pub enum RefreshTokens {
    Table,
    Id,
    UserId,
    TokenHash,
    ExpiresAt,
    CreatedAt,
}

#[derive(Iden)]
pub enum TeamInvitations {
    Table,
    Id,
    TeamId,
    Email,
    OauthProvider,
    OauthProviderUsername,
    InvitedBy,
    Role,
    Status,
    CreatedAt,
    ExpiresAt,
}

#[derive(Iden)]
pub enum TeamInviteKeys {
    Table,
    Id,
    TeamId,
    KeyHash,
    Role,
    CreatedBy,
    CreatedAt,
    ExpiresAt,
    UsedBy,
    UsedAt,
    RevokedAt,
}

#[derive(Iden)]
pub enum SessionSync {
    Table,
    SessionId,
    SourcePath,
    SyncStatus,
    LastSyncedAt,
}

#[derive(Iden)]
pub enum SyncCursors {
    Table,
    TeamId,
    Cursor,
    UpdatedAt,
}

#[derive(Iden)]
pub enum BodyCache {
    Table,
    SessionId,
    Body,
    CachedAt,
}
