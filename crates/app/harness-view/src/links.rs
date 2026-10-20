//! The links this view opens and is opened by: a thread's place shown by its program, and
//! the work-record link `duck://<chain>/<script.calls>/<agent>/<place>` chat opens into this
//! view, for an agent whose script calls harness.

/// What a route handed to this view names: `<agent>[/<place>]`, delivered decoded and
/// joined, so `12/chat/general` is agent 12 at harness's place `chat/general`.
pub fn route_target(route: &str) -> Option<(u64, Option<String>)> {
    let (agent, place) = match route.split_once('/') {
        Some((agent, place)) => (agent, Some(place.to_owned())),
        None => (route, None),
    };
    Some((agent.parse().ok()?, place.filter(|place| !place.is_empty())))
}

/// Where a place is shown by the program it names: `chat/general` is
/// `duck://<chain>/chat/general`. None without a chain.
pub fn place_link(chain: &str, place: &str) -> Option<(String, String)> {
    let mut segments = place.split('/');
    let program = segments.next()?;
    let tail: Vec<&str> = segments.collect();
    let link = ducklink::mint(chain, program, &tail)?;
    Some((program.to_owned(), link))
}

/// The work-record link for agent `agent` at `place`, as chat mints it: what a conversation's
/// Copy link copies. None without a chain.
pub fn work_link(chain: &str, agent: u64, place: &str) -> Option<String> {
    let agent = agent.to_string();
    let mut tail = vec![agent.as_str()];
    tail.extend(place.split('/').filter(|part| !part.is_empty()));
    ducklink::mint(chain, harness::PROGRAM, &tail)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_work_link_lands_on_its_agent_and_place() {
        for channel in ["general", "dm-3-12", "forge:web:3", "보고서 #1"] {
            let place = format!("chat/{channel}");
            let link = work_link("garage#0a1b2c3d", 12, &place).unwrap();
            let route = ducklink::Link::parse(&link).unwrap().tail.join("/");
            assert_eq!(route_target(&route), Some((12, Some(place))));
        }
        assert_eq!(
            work_link("garage#0a1b2c3d", 12, "chat/general").as_deref(),
            Some("duck://garage-0a1b2c3d/harness/12/chat/general")
        );
        assert_eq!(route_target("12"), Some((12, None)));
        assert_eq!(route_target("scout/chat/general"), None);
    }

    #[test]
    fn a_place_opens_in_its_program() {
        assert_eq!(
            place_link("garage#0a1b2c3d", "chat/general"),
            Some((
                "chat".to_owned(),
                "duck://garage-0a1b2c3d/chat/general".to_owned()
            ))
        );
        assert_eq!(place_link("", "chat/general"), None);
    }
}
