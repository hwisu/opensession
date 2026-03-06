use opensession_api::DesktopSessionListQuery;
use opensession_local_db::{LocalSessionFilter, LocalSortOrder, LocalTimeRange};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SearchMode {
    Keyword,
    Vector,
}

fn normalize_non_empty(value: Option<String>) -> Option<String> {
    value
        .map(|raw| raw.trim().to_string())
        .and_then(|trimmed| (!trimmed.is_empty()).then_some(trimmed))
}

fn parse_positive_u32(raw: Option<String>, fallback: u32, max: u32) -> u32 {
    let parsed = raw
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(fallback);
    parsed.min(max).max(1)
}

fn map_sort_order(sort: Option<&str>) -> LocalSortOrder {
    match sort.unwrap_or_default() {
        "popular" => LocalSortOrder::Popular,
        "longest" => LocalSortOrder::Longest,
        _ => LocalSortOrder::Recent,
    }
}

fn map_time_range(time_range: Option<&str>) -> LocalTimeRange {
    match time_range.unwrap_or_default() {
        "24h" => LocalTimeRange::Hours24,
        "7d" => LocalTimeRange::Days7,
        "30d" => LocalTimeRange::Days30,
        _ => LocalTimeRange::All,
    }
}

pub(crate) fn split_search_mode(raw: Option<String>) -> (Option<String>, SearchMode) {
    let normalized = normalize_non_empty(raw);
    let Some(value) = normalized else {
        return (None, SearchMode::Keyword);
    };
    let lower = value.to_ascii_lowercase();
    for prefix in ["vector:", "vec:"] {
        if lower.starts_with(prefix) {
            let query = value[prefix.len()..].trim().to_string();
            return ((!query.is_empty()).then_some(query), SearchMode::Vector);
        }
    }
    (Some(value), SearchMode::Keyword)
}

pub(crate) fn build_local_filter_with_mode(
    query: DesktopSessionListQuery,
) -> (LocalSessionFilter, u32, u32, SearchMode) {
    let page = parse_positive_u32(query.page, 1, 10_000);
    let per_page = parse_positive_u32(query.per_page, 20, 200);
    let offset = (page.saturating_sub(1)).saturating_mul(per_page);
    let (search_query, search_mode) = split_search_mode(query.search);

    let filter = LocalSessionFilter {
        search: search_query,
        tool: normalize_non_empty(query.tool),
        git_repo_name: normalize_non_empty(query.git_repo_name),
        exclude_low_signal: true,
        sort: map_sort_order(query.sort.as_deref()),
        time_range: map_time_range(query.time_range.as_deref()),
        limit: Some(per_page),
        offset: Some(offset),
        ..Default::default()
    };

    (filter, page, per_page, search_mode)
}

#[cfg(test)]
mod tests {
    use super::{build_local_filter_with_mode, split_search_mode, SearchMode};
    use opensession_api::DesktopSessionListQuery;
    use opensession_local_db::{LocalSortOrder, LocalTimeRange};

    #[test]
    fn query_mapping_trims_inputs_and_clamps_large_pages() {
        let (filter, page, per_page, mode) =
            build_local_filter_with_mode(DesktopSessionListQuery {
                page: Some("0".to_string()),
                per_page: Some("999".to_string()),
                search: Some("  fix auth  ".to_string()),
                tool: Some(" codex ".to_string()),
                git_repo_name: Some(" org/repo ".to_string()),
                sort: Some("longest".to_string()),
                time_range: Some("30d".to_string()),
                force_refresh: None,
            });

        assert_eq!(page, 1);
        assert_eq!(per_page, 200);
        assert_eq!(mode, SearchMode::Keyword);
        assert_eq!(filter.search.as_deref(), Some("fix auth"));
        assert_eq!(filter.tool.as_deref(), Some("codex"));
        assert_eq!(filter.git_repo_name.as_deref(), Some("org/repo"));
        assert_eq!(filter.sort, LocalSortOrder::Longest);
        assert_eq!(filter.time_range, LocalTimeRange::Days30);
        assert_eq!(filter.offset, Some(0));
    }

    #[test]
    fn vector_prefix_without_query_keeps_vector_mode_but_clears_search_text() {
        let (query, mode) = split_search_mode(Some("vector:   ".to_string()));

        assert_eq!(query, None);
        assert_eq!(mode, SearchMode::Vector);
    }
}
