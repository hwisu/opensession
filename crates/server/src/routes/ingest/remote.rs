use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use opensession_api::parse_preview_source::GitSource;

use crate::AppConfig;

use super::errors::PreviewRouteError;

pub(super) fn validate_remote_url(remote: &str) -> Result<reqwest::Url, PreviewRouteError> {
    let parsed = reqwest::Url::parse(remote)
        .map_err(|_| PreviewRouteError::invalid_source("remote must be an absolute http(s) URL"))?;

    let scheme = parsed.scheme().to_ascii_lowercase();
    if scheme != "https" {
        return Err(PreviewRouteError::invalid_source("remote must use https"));
    }

    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(PreviewRouteError::invalid_source(
            "remote cannot include credentials",
        ));
    }

    if parsed.query().is_some() || parsed.fragment().is_some() {
        return Err(PreviewRouteError::invalid_source(
            "remote cannot include query or fragment",
        ));
    }

    let host = parsed
        .host_str()
        .ok_or_else(|| PreviewRouteError::invalid_source("remote host is required"))?;

    if is_disallowed_remote_host(host) {
        return Err(PreviewRouteError::invalid_source(
            "remote host is not allowed",
        ));
    }

    let repo_path = parsed.path().trim_matches('/');
    if repo_path.is_empty() {
        return Err(PreviewRouteError::invalid_source(
            "remote must include repository path",
        ));
    }

    Ok(parsed)
}

fn is_disallowed_remote_host(host: &str) -> bool {
    let lowered = host.to_ascii_lowercase();
    if lowered == "localhost" || lowered.ends_with(".localhost") {
        return true;
    }

    match host.parse::<IpAddr>() {
        Ok(IpAddr::V4(v4)) => is_disallowed_ipv4(v4),
        Ok(IpAddr::V6(v6)) => is_disallowed_ipv6(v6),
        Err(_) => false,
    }
}

fn is_disallowed_ipv4(ip: Ipv4Addr) -> bool {
    ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_documentation()
        || ip.is_multicast()
        || ip.is_unspecified()
}

fn is_disallowed_ipv6(ip: Ipv6Addr) -> bool {
    ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_unique_local()
        || ip.is_unicast_link_local()
        || ip.is_multicast()
        || is_ipv6_documentation(ip)
}

fn is_ipv6_documentation(ip: Ipv6Addr) -> bool {
    let segments = ip.segments();
    segments[0] == 0x2001 && segments[1] == 0x0db8
}

pub(super) fn oauth_provider_host_from_url(raw: &str) -> Option<String> {
    reqwest::Url::parse(raw)
        .ok()
        .and_then(|url| url.host_str().map(|value| value.to_ascii_lowercase()))
}

pub(super) fn configured_gitlab_hosts(config: &AppConfig) -> HashSet<String> {
    config
        .oauth_providers
        .iter()
        .filter(|provider| provider.id == "gitlab")
        .filter_map(|provider| oauth_provider_host_from_url(&provider.token_url))
        .collect()
}

pub(super) fn is_gitlab_host(host: &str, gitlab_hosts: &HashSet<String>) -> bool {
    host == "gitlab.com" || gitlab_hosts.contains(host)
}

fn origin_from_url(url: &reqwest::Url) -> Result<String, PreviewRouteError> {
    let host = url
        .host_str()
        .ok_or_else(|| PreviewRouteError::invalid_source("remote host is required"))?;
    let mut origin = format!("{}://{host}", url.scheme());
    if let Some(port) = url.port() {
        origin.push(':');
        origin.push_str(&port.to_string());
    }
    Ok(origin)
}

fn encode_segments(value: &str) -> String {
    value
        .split('/')
        .map(|segment| urlencoding::encode(segment).into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

fn strip_git_suffix(value: &str) -> &str {
    value.strip_suffix(".git").unwrap_or(value)
}

fn repo_path_segments(url: &reqwest::Url) -> Result<Vec<String>, PreviewRouteError> {
    let mut segments: Vec<String> = url
        .path_segments()
        .ok_or_else(|| PreviewRouteError::invalid_source("remote repository path is invalid"))?
        .filter(|segment| !segment.is_empty())
        .map(|segment| {
            urlencoding::decode(segment)
                .map(|decoded| decoded.trim().to_string())
                .map_err(|_| {
                    PreviewRouteError::invalid_source(
                        "remote repository path contains invalid percent encoding",
                    )
                })
        })
        .collect::<Result<Vec<_>, _>>()?;

    if segments.len() < 2 {
        return Err(PreviewRouteError::invalid_source(
            "remote must include owner/group and repository",
        ));
    }

    if let Some(last) = segments.last_mut() {
        *last = strip_git_suffix(last).to_string();
    }

    if segments
        .iter()
        .any(|segment| segment.is_empty() || segment == "." || segment == "..")
    {
        return Err(PreviewRouteError::invalid_source(
            "remote repository path contains invalid segments",
        ));
    }

    Ok(segments)
}

fn build_github_raw_url(
    url: &reqwest::Url,
    r#ref: &str,
    path: &str,
) -> Result<String, PreviewRouteError> {
    let segments = repo_path_segments(url)?;
    if segments.len() != 2 {
        return Err(PreviewRouteError::invalid_source(
            "github remote must look like https://github.com/{owner}/{repo}",
        ));
    }

    Ok(format!(
        "https://raw.githubusercontent.com/{}/{}/{}/{}",
        segments[0],
        segments[1],
        encode_segments(r#ref),
        encode_segments(path)
    ))
}

fn build_gitlab_raw_url(
    url: &reqwest::Url,
    r#ref: &str,
    path: &str,
) -> Result<String, PreviewRouteError> {
    let project_path = repo_path_segments(url)?.join("/");
    let origin = origin_from_url(url)?;
    Ok(format!(
        "{}/{}/-/raw/{}/{}",
        origin,
        encode_segments(&project_path),
        encode_segments(r#ref),
        encode_segments(path)
    ))
}

fn build_generic_raw_url(
    url: &reqwest::Url,
    r#ref: &str,
    path: &str,
) -> Result<String, PreviewRouteError> {
    let repo_path = repo_path_segments(url)?.join("/");
    let origin = origin_from_url(url)?;
    Ok(format!(
        "{}/{}/raw/{}/{}",
        origin,
        encode_segments(&repo_path),
        encode_segments(r#ref),
        encode_segments(path)
    ))
}

pub(super) fn build_git_raw_url(
    source: &GitSource,
    gitlab_hosts: &HashSet<String>,
) -> Result<String, PreviewRouteError> {
    let remote = validate_remote_url(&source.remote)?;
    let host = remote
        .host_str()
        .ok_or_else(|| PreviewRouteError::invalid_source("remote host is required"))?
        .to_ascii_lowercase();

    if host == "github.com" {
        return build_github_raw_url(&remote, &source.r#ref, &source.path);
    }
    if is_gitlab_host(&host, gitlab_hosts) {
        return build_gitlab_raw_url(&remote, &source.r#ref, &source.path);
    }

    build_generic_raw_url(&remote, &source.r#ref, &source.path)
}

pub(super) fn provider_for_host(
    host: &str,
    gitlab_hosts: &HashSet<String>,
) -> Option<&'static str> {
    if host == "github.com" {
        return Some("github");
    }
    if is_gitlab_host(host, gitlab_hosts) {
        return Some("gitlab");
    }
    None
}

pub(super) fn repo_path_from_remote(url: &reqwest::Url) -> Result<String, PreviewRouteError> {
    Ok(repo_path_segments(url)?.join("/"))
}

pub(super) fn path_prefix_matches(repo_path: &str, prefix: &str) -> bool {
    if prefix.is_empty() {
        return true;
    }
    repo_path == prefix
        || repo_path
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.starts_with('/'))
}

pub(super) async fn ensure_remote_resolves_public(
    remote: &reqwest::Url,
) -> Result<(), PreviewRouteError> {
    let host = remote
        .host_str()
        .ok_or_else(|| PreviewRouteError::invalid_source("remote host is required"))?;
    if host.parse::<IpAddr>().is_ok() {
        return Ok(());
    }

    let port = remote.port_or_known_default().unwrap_or(443);
    let mut resolved = tokio::net::lookup_host((host, port))
        .await
        .map_err(|_| PreviewRouteError::fetch_failed("remote host DNS lookup failed"))?;
    let mut found_any = false;
    for addr in &mut resolved {
        found_any = true;
        let ip = addr.ip();
        let disallowed = match ip {
            IpAddr::V4(v4) => is_disallowed_ipv4(v4),
            IpAddr::V6(v6) => is_disallowed_ipv6(v6),
        };
        if disallowed {
            tracing::warn!(host = %host, ip = %ip, "blocked remote host resolving to disallowed IP");
            return Err(PreviewRouteError::invalid_source(
                "remote host resolves to a disallowed address",
            ));
        }
    }
    if !found_any {
        return Err(PreviewRouteError::fetch_failed(
            "remote host DNS lookup returned no addresses",
        ));
    }
    Ok(())
}
