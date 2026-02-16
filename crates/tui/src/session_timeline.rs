use opensession_core::trace::{Event, EventType, Session};
use std::collections::{BTreeSet, HashMap};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaneMarker {
    None,
    Fork,
    Merge,
}

#[derive(Debug, Clone)]
pub struct LaneEventRef<'a> {
    pub event: &'a Event,
    pub source_index: usize,
    pub lane: usize,
    pub marker: LaneMarker,
    pub active_lanes: Vec<usize>,
}

/// Build lane-aware events for the session.
///
/// Lane 0 is the main session lane. Sub-task lanes are allocated from 1..N.
/// TaskStart/TaskEnd create fork/merge markers so TUI can render branch boundaries.
#[allow(dead_code)]
pub fn build_lane_events<'a, F>(session: &'a Session, mut include: F) -> Vec<LaneEventRef<'a>>
where
    F: FnMut(&EventType) -> bool,
{
    build_lane_events_with_filter(session, |_| true, |event_type| include(event_type))
}

/// Build lane-aware events for the session with an additional event-level filter.
///
/// `include_event` controls whether an event participates in lane state at all.
/// Returning `false` removes that event from both rendering and lane bookkeeping.
pub fn build_lane_events_with_filter<'a, E, F>(
    session: &'a Session,
    mut include_event: E,
    mut include: F,
) -> Vec<LaneEventRef<'a>>
where
    E: FnMut(&Event) -> bool,
    F: FnMut(&EventType) -> bool,
{
    let mut out = Vec::new();
    let mut task_lane: HashMap<String, usize> = HashMap::new();
    let mut active_lanes: BTreeSet<usize> = BTreeSet::new();
    let mut reusable_lanes: BTreeSet<usize> = BTreeSet::new();
    let mut next_lane = 1usize;

    for (source_index, event) in session.events.iter().enumerate() {
        if !include_event(event) {
            continue;
        }
        let mut lane = 0usize;
        let mut marker = LaneMarker::None;
        let task_id = event.task_id.as_ref();

        match &event.event_type {
            EventType::TaskStart { .. } => {
                if let Some(task_id) = task_id {
                    lane = task_lane.get(task_id).copied().unwrap_or_else(|| {
                        allocate_lane_for_task(
                            &mut task_lane,
                            &mut reusable_lanes,
                            &mut next_lane,
                            task_id,
                        )
                    });
                    let was_active = active_lanes.contains(&lane);
                    if lane > 0 {
                        active_lanes.insert(lane);
                        if !was_active {
                            marker = LaneMarker::Fork;
                        }
                    }
                }
            }
            EventType::TaskEnd { .. } => {
                if let Some(task_id) = task_id {
                    lane = task_lane.get(task_id).copied().unwrap_or_else(|| {
                        allocate_lane_for_task(
                            &mut task_lane,
                            &mut reusable_lanes,
                            &mut next_lane,
                            task_id,
                        )
                    });
                    if lane > 0 {
                        marker = LaneMarker::Merge;
                    }
                }
            }
            _ => {
                if let Some(task_id) = task_id {
                    if let Some(existing) = task_lane.get(task_id).copied() {
                        lane = existing;
                    } else {
                        lane = allocate_lane_for_task(
                            &mut task_lane,
                            &mut reusable_lanes,
                            &mut next_lane,
                            task_id,
                        );
                        if lane > 0 {
                            active_lanes.insert(lane);
                            marker = LaneMarker::Fork;
                        }
                    }
                }
            }
        }

        let mut lanes_snapshot = Vec::with_capacity(active_lanes.len() + 1);
        lanes_snapshot.push(0);
        lanes_snapshot.extend(active_lanes.iter().copied());
        if lane > 0 && !lanes_snapshot.contains(&lane) {
            lanes_snapshot.push(lane);
            lanes_snapshot.sort_unstable();
        }

        if include(&event.event_type) {
            out.push(LaneEventRef {
                event,
                source_index,
                lane,
                marker,
                active_lanes: lanes_snapshot,
            });
        }

        if matches!(event.event_type, EventType::TaskEnd { .. }) {
            if let Some(task_id) = task_id {
                if let Some(ended_lane) = task_lane.remove(task_id) {
                    active_lanes.remove(&ended_lane);
                    if ended_lane > 0 {
                        reusable_lanes.insert(ended_lane);
                    }
                }
            }
        }
    }

    out
}

/// Pair ToolCall events with ToolResult events using the same heuristic as Web UI:
/// 1) exact call_id/semantic.call_id match, then
/// 2) nearby same-name ToolResult fallback.
#[allow(dead_code)]
pub fn pair_tool_call_results(events: &[Event]) -> HashMap<usize, usize> {
    let mut pairs = HashMap::new();
    let mut result_by_call_id: HashMap<String, usize> = HashMap::new();

    for (idx, event) in events.iter().enumerate() {
        if !matches!(event.event_type, EventType::ToolResult { .. }) {
            continue;
        }
        if let Some(call_id) = event.semantic_call_id() {
            result_by_call_id.insert(call_id.to_string(), idx);
        }
    }

    for (idx, event) in events.iter().enumerate() {
        let EventType::ToolCall { name } = &event.event_type else {
            continue;
        };
        let call_id = event
            .semantic_call_id()
            .map(str::to_string)
            .unwrap_or_else(|| event.event_id.clone());
        if let Some(result_idx) = result_by_call_id.get(&call_id).copied() {
            pairs.insert(idx, result_idx);
            continue;
        }

        for (j, candidate) in events
            .iter()
            .enumerate()
            .take(std::cmp::min(events.len(), idx + 8))
            .skip(idx + 1)
        {
            match &candidate.event_type {
                EventType::ToolCall { .. } => break,
                EventType::ToolResult {
                    name: result_name, ..
                } if result_name == name => {
                    pairs.insert(idx, j);
                    break;
                }
                _ => {}
            }
        }
    }

    pairs
}

fn allocate_lane_for_task(
    task_lane: &mut HashMap<String, usize>,
    reusable_lanes: &mut BTreeSet<usize>,
    next_lane: &mut usize,
    task_id: &str,
) -> usize {
    let allocated = if let Some(reused) = reusable_lanes.iter().next().copied() {
        reusable_lanes.remove(&reused);
        reused
    } else {
        let value = *next_lane;
        *next_lane += 1;
        value
    };
    task_lane.insert(task_id.to_string(), allocated);
    allocated
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use opensession_core::trace::{Agent, Content, Event, Session, ATTR_SEMANTIC_CALL_ID};
    use std::collections::HashMap;

    fn mk_event(id: &str, event_type: EventType, task_id: Option<&str>) -> Event {
        Event {
            event_id: id.to_string(),
            timestamp: Utc::now(),
            event_type,
            task_id: task_id.map(|v| v.to_string()),
            content: Content::empty(),
            duration_ms: None,
            attributes: HashMap::new(),
        }
    }

    #[test]
    fn lane_assignment_handles_nested_tasks() {
        let mut session = Session::new(
            "s1".to_string(),
            Agent {
                provider: "anthropic".to_string(),
                model: "m".to_string(),
                tool: "codex".to_string(),
                tool_version: None,
            },
        );

        session.events = vec![
            mk_event("e1", EventType::UserMessage, None),
            mk_event("e2", EventType::TaskStart { title: None }, Some("t1")),
            mk_event("e3", EventType::AgentMessage, Some("t1")),
            mk_event("e4", EventType::TaskStart { title: None }, Some("t2")),
            mk_event(
                "e5",
                EventType::ToolCall {
                    name: "Read".to_string(),
                },
                Some("t2"),
            ),
            mk_event("e6", EventType::TaskEnd { summary: None }, Some("t2")),
            mk_event("e7", EventType::TaskEnd { summary: None }, Some("t1")),
        ];

        let lanes = build_lane_events(&session, |_| true);
        assert_eq!(lanes.len(), 7);
        assert_eq!(lanes[0].lane, 0);
        assert_eq!(lanes[1].marker, LaneMarker::Fork);
        assert_eq!(lanes[1].lane, 1);
        assert_eq!(lanes[3].marker, LaneMarker::Fork);
        assert_eq!(lanes[3].lane, 2);
        assert_eq!(lanes[5].marker, LaneMarker::Merge);
        assert_eq!(lanes[5].lane, 2);
        assert_eq!(lanes[6].marker, LaneMarker::Merge);
        assert_eq!(lanes[6].lane, 1);
    }

    #[test]
    fn ended_lanes_are_reused_for_later_tasks() {
        let mut session = Session::new(
            "s2".to_string(),
            Agent {
                provider: "anthropic".to_string(),
                model: "m".to_string(),
                tool: "claude-code".to_string(),
                tool_version: None,
            },
        );

        session.events = vec![
            mk_event("e1", EventType::TaskStart { title: None }, Some("t1")),
            mk_event("e2", EventType::AgentMessage, Some("t1")),
            mk_event("e3", EventType::TaskEnd { summary: None }, Some("t1")),
            mk_event("e4", EventType::TaskStart { title: None }, Some("t2")),
            mk_event("e5", EventType::AgentMessage, Some("t2")),
            mk_event("e6", EventType::TaskEnd { summary: None }, Some("t2")),
        ];

        let lanes = build_lane_events(&session, |_| true);
        assert_eq!(lanes[0].lane, 1);
        assert_eq!(lanes[2].lane, 1);
        assert_eq!(lanes[3].lane, 1);
        assert_eq!(lanes[5].lane, 1);
    }

    #[test]
    fn lane_is_lazily_allocated_for_task_id_without_task_start() {
        let mut session = Session::new(
            "s3".to_string(),
            Agent {
                provider: "openai".to_string(),
                model: "gpt-5".to_string(),
                tool: "codex".to_string(),
                tool_version: None,
            },
        );

        session.events = vec![
            mk_event("e1", EventType::AgentMessage, Some("spawn-1")),
            mk_event(
                "e2",
                EventType::ToolCall {
                    name: "exec".to_string(),
                },
                Some("spawn-1"),
            ),
            mk_event("e3", EventType::TaskEnd { summary: None }, Some("spawn-1")),
        ];

        let lanes = build_lane_events(&session, |_| true);
        assert_eq!(lanes.len(), 3);
        assert_eq!(lanes[0].lane, 1);
        assert_eq!(lanes[0].marker, LaneMarker::Fork);
        assert_eq!(lanes[1].lane, 1);
        assert_eq!(lanes[1].marker, LaneMarker::None);
        assert_eq!(lanes[2].lane, 1);
        assert_eq!(lanes[2].marker, LaneMarker::Merge);
    }

    #[test]
    fn late_task_start_does_not_double_fork_active_lane() {
        let mut session = Session::new(
            "s4".to_string(),
            Agent {
                provider: "openai".to_string(),
                model: "gpt-5".to_string(),
                tool: "codex".to_string(),
                tool_version: None,
            },
        );

        session.events = vec![
            mk_event("e1", EventType::AgentMessage, Some("spawn-2")),
            mk_event("e2", EventType::TaskStart { title: None }, Some("spawn-2")),
            mk_event("e3", EventType::TaskEnd { summary: None }, Some("spawn-2")),
        ];

        let lanes = build_lane_events(&session, |_| true);
        assert_eq!(lanes.len(), 3);
        assert_eq!(lanes[0].marker, LaneMarker::Fork);
        assert_eq!(lanes[1].marker, LaneMarker::None);
        assert_eq!(lanes[2].marker, LaneMarker::Merge);
    }

    #[test]
    fn pair_tool_call_results_matches_by_semantic_call_id() {
        let mut call = mk_event(
            "call-1",
            EventType::ToolCall {
                name: "Read".to_string(),
            },
            None,
        );
        call.attributes.insert(
            ATTR_SEMANTIC_CALL_ID.to_string(),
            serde_json::Value::String("cid-1".to_string()),
        );

        let result = mk_event(
            "result-1",
            EventType::ToolResult {
                name: "Read".to_string(),
                is_error: false,
                call_id: Some("cid-1".to_string()),
            },
            None,
        );

        let events = vec![call, result];
        let pairs = pair_tool_call_results(&events);
        assert_eq!(pairs.get(&0), Some(&1));
    }

    #[test]
    fn pair_tool_call_results_falls_back_to_nearby_same_tool_name() {
        let call = mk_event(
            "call-2",
            EventType::ToolCall {
                name: "WebSearch".to_string(),
            },
            None,
        );
        let middle = mk_event("a1", EventType::AgentMessage, None);
        let result = mk_event(
            "result-2",
            EventType::ToolResult {
                name: "WebSearch".to_string(),
                is_error: false,
                call_id: None,
            },
            None,
        );

        let events = vec![call, middle, result];
        let pairs = pair_tool_call_results(&events);
        assert_eq!(pairs.get(&0), Some(&2));
    }
}
