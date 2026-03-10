//! Compile-time–checked column identifiers for all tables.

use sea_query::Iden;

#[derive(Iden)]
pub enum Users {
    Table,
    Id,
    Nickname,
    CreatedAt,
    Email,
    PasswordHash,
    PasswordSalt,
}

#[derive(Iden)]
pub enum ApiKeys {
    Table,
    Id,
    UserId,
    KeyHash,
    KeyPrefix,
    Status,
    CreatedAt,
    GraceUntil,
    RevokedAt,
    LastUsedAt,
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
    JobProtocol,
    JobSystem,
    JobId,
    JobTitle,
    JobRunId,
    JobAttempt,
    JobStage,
    JobReviewKind,
    JobStatus,
    JobThreadId,
    JobArtifactCount,
    SessionScore,
    ScorePlugin,
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
pub enum OauthProviderTokens {
    Table,
    Id,
    UserId,
    Provider,
    ProviderHost,
    AccessTokenEnc,
    ExpiresAt,
    CreatedAt,
    UpdatedAt,
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
pub enum GitCredentials {
    Table,
    Id,
    UserId,
    Label,
    Host,
    PathPrefix,
    HeaderName,
    HeaderValueEnc,
    CreatedAt,
    UpdatedAt,
    LastUsedAt,
}

#[derive(Iden)]
pub enum BodyCache {
    Table,
    SessionId,
    Body,
    CachedAt,
}
